//! Verifies the textual format of the unified observation pipeline's
//! `trace-log` sink line-for-line.
//!
//! Each `__chart_observe!` arm has a stable logfmt-style format:
//!
//!     `[statechart:<Name>] <verb> key=value key=value …`
//!
//! These tests pin every verb's exact rendering so a real-life log capture
//! can be diffed against expected behavior. The chart is exercised through
//! both the journal sink (so we can assert atom-by-atom equivalence) and
//! the trace-log sink (so we can assert the rendered strings).

#![cfg(all(feature = "tokio", feature = "journal", feature = "trace-log"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, Duration, TraceEvent, TransitionReason};
use std::sync::{Mutex, MutexGuard, OnceLock};

// ── Capture sink ─────────────────────────────────────────────────────
//
// log::set_logger can only be installed once per process. The capture
// holds a single Vec<String> shared across all tests, plus a global
// `TEST_LOCK` that each test holds across its drive-then-assert
// section so concurrent tests don't interleave their captured lines.

struct Capture {
    lines: Mutex<Vec<String>>,
}

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if let Ok(mut g) = self.lines.lock() {
            g.push(format!("{}", record.args()));
        }
    }
    fn flush(&self) {}
}

static CAPTURE: OnceLock<&'static Capture> = OnceLock::new();
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn install_capture() -> &'static Capture {
    *CAPTURE.get_or_init(|| {
        let leaked: &'static Capture = Box::leak(Box::new(Capture {
            lines: Mutex::new(Vec::new()),
        }));
        let _ = log::set_logger(leaked);
        log::set_max_level(log::LevelFilter::Trace);
        leaked
    })
}

/// RAII helper. Acquire before driving the chart, drop after assertions
/// — keeps `tests::*` from interleaving captured log lines.
fn lock_for_test() -> MutexGuard<'static, ()> {
    install_capture();
    let g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // Clear any lines left by a previous test that paniced before
    // draining (poison case).
    let cap = CAPTURE.get().unwrap();
    cap.lines.lock().unwrap().clear();
    g
}

fn current_lines() -> Vec<String> {
    let cap = install_capture();
    cap.lines.lock().unwrap().clone()
}

// ── Test chart ───────────────────────────────────────────────────────
//
// Covers every observation verb the macro can emit. Two states with
// entry/exit actions, a handler that emits and transitions, a timer,
// a during, and a terminate event.

#[derive(Default)]
pub struct Ctx {
    pub buf: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Step,
    Halt,
}

statechart! {
    TraceFmt {
        context: Ctx;
        events:  Ev;

        default(Idle);
        terminate(Halt);

        state Idle {
            entry: idle_in;
            exit:  idle_out;
            during: tick(buf);
            on(after Duration::from_secs(60)) => Idle;
            on(Go) => Active;
        }
        state Active {
            entry: active_in;
            exit:  active_out;
            on(Step) => bump;
            on(Halt) => Idle;
        }
    }
}

async fn tick(_buf: &mut u32) -> Ev {
    Ev::Step
}

impl TraceFmtActions for TraceFmtActionContext<'_> {
    async fn idle_in(&mut self) {}
    async fn idle_out(&mut self) {}
    async fn active_in(&mut self) {}
    async fn active_out(&mut self) {}
    async fn bump(&mut self) {
        let _ = self.emit(Ev::Halt);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Returns true iff `line` matches the expected `[statechart:TraceFmt] verb …`
/// shape. Every line emitted by the dispatcher must.
fn well_formed(line: &str) -> bool {
    line.starts_with("[statechart:TraceFmt] ")
}

/// Filter `lines` to only those whose verb (the first token after the
/// prefix) equals `verb`. Useful for asserting on a single verb's lines.
fn lines_with_verb<'a>(lines: &'a [String], verb: &str) -> Vec<&'a str> {
    let prefix = "[statechart:TraceFmt] ";
    lines
        .iter()
        .filter_map(|l| l.strip_prefix(prefix))
        .filter(|rest| rest.split_whitespace().next() == Some(verb))
        .map(|s| s.as_ref())
        .collect()
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn every_line_uses_statechart_prefix() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            let _ = m.dispatch(Ev::Halt).await;
        })
        .await;

    let lines = current_lines();
    assert!(!lines.is_empty(), "no log lines captured");
    for l in &lines {
        assert!(
            well_formed(l),
            "line missing [statechart:TraceFmt] prefix: {l:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn enter_and_exit_emit_begin_end_pairs() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let entering = lines_with_verb(&lines, "entering");
    let entered = lines_with_verb(&lines, "entered");
    assert_eq!(
        entering.len(),
        entered.len(),
        "every `entering` must have a matching `entered`"
    );
    assert!(entering.contains(&"entering state=TraceFmt"));
    assert!(entered.contains(&"entered state=TraceFmt"));
    assert!(entering.contains(&"entering state=Idle"));
    assert!(entered.contains(&"entered state=Idle"));
}

#[tokio::test(flavor = "current_thread")]
async fn transition_records_event_reason() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let begins = lines_with_verb(&lines, "transition-begin");
    assert!(!begins.is_empty(), "no transition-begin lines captured");
    let line = begins[0];
    assert!(
        line.contains("from=Idle"),
        "transition-begin missing from=Idle: {line:?}"
    );
    assert!(
        line.contains("to=Active"),
        "transition-begin missing to=Active: {line:?}"
    );
    assert!(
        line.contains("reason=event:Go"),
        "transition-begin missing reason=event:Go: {line:?}"
    );

    let completes = lines_with_verb(&lines, "transition-complete");
    assert!(
        !completes.is_empty(),
        "transition-complete must follow transition-begin"
    );
    assert!(completes[0].contains("from=Idle") && completes[0].contains("to=Active"));
}

#[tokio::test(flavor = "current_thread")]
async fn event_lifecycle_received_then_outcome() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let received = lines_with_verb(&lines, "event-received");
    let delivered = lines_with_verb(&lines, "event-delivered");
    assert!(received.iter().any(|l| l.contains("name=Go")));
    assert!(delivered
        .iter()
        .any(|l| l.contains("name=Go") && l.contains("handler=Idle")));
}

#[tokio::test(flavor = "current_thread")]
async fn during_emits_started_and_cancelled() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            // Go enters Active, which causes Idle's during to be cancelled.
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let started = lines_with_verb(&lines, "during-started");
    let cancelled = lines_with_verb(&lines, "during-cancelled");
    assert!(started.iter().any(|l| l.contains("during=tick")));
    assert!(cancelled.iter().any(|l| l.contains("during=tick")));
}

#[tokio::test(flavor = "current_thread")]
async fn timer_arm_then_cancel_lines() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let armed = lines_with_verb(&lines, "timer-armed");
    let cancelled = lines_with_verb(&lines, "timer-cancelled");
    assert!(
        armed.iter().any(|l| l.contains("state=Idle")),
        "timer should arm when Idle is entered"
    );
    assert!(
        cancelled.iter().any(|l| l.contains("state=Idle")),
        "timer should be cancelled when Idle is exited (Go transition)"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn emit_queued_appears_for_handler_emit() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            // Step's handler calls emit(Halt).
            let _ = m.dispatch(Ev::Step).await;
        })
        .await;

    let lines = current_lines();
    let queued = lines_with_verb(&lines, "emit-queued");
    assert!(
        queued.iter().any(|l| l.contains("event=Halt")),
        "expected emit-queued event=Halt line: {lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn terminate_records_request_then_terminated() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            // Halt from Active is a transition to Idle (no terminate);
            // dispatch Halt twice via Idle to actually terminate.
            // First Go landed us in Active; Halt → Idle (handler).
            let _ = m.dispatch(Ev::Halt).await; // back to Idle
            let _ = m.dispatch(Ev::Halt).await; // now terminate
        })
        .await;

    let lines = current_lines();
    let req = lines_with_verb(&lines, "terminate-requested");
    let term = lines_with_verb(&lines, "terminated");
    assert!(
        req.iter().any(|l| l.contains("event=Halt")),
        "expected terminate-requested event=Halt: {lines:?}"
    );
    assert!(
        !term.is_empty(),
        "expected a final `terminated` line: {lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn started_records_chart_hash() {
    let _g = lock_for_test();

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
        })
        .await;

    let lines = current_lines();
    let started = lines_with_verb(&lines, "started");
    assert!(!started.is_empty(), "expected one `started` line");
    assert!(
        started[0].contains("chart_hash=0x"),
        "started must include chart_hash in hex: {:?}",
        started[0]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trace_and_journal_have_matching_atom_count() {
    // The "one journal, multiple outputs" contract: every TraceEvent
    // recorded in the journal corresponds to exactly one log line in
    // the trace, modulo verb-name differences.
    let _g = lock_for_test();

    let actual = tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            m.take_journal()
        })
        .await;

    let lines = current_lines();
    assert_eq!(
        actual.len(),
        lines.len(),
        "journal/trace atom count diverged ({} vs {} lines)",
        actual.len(),
        lines.len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn transition_reason_in_journal_matches_trace() {
    // The journal records the structured `TransitionReason::Event { event }`
    // and the trace renders it as `event:<VariantName>`. They must agree.
    let _g = lock_for_test();

    let actual = tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = TraceFmt::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            m.take_journal()
        })
        .await;

    let lines = current_lines();

    let journal_reason = actual.iter().find_map(|e| match e {
        TraceEvent::TransitionFired { reason, .. } => Some(*reason),
        _ => None,
    });
    let trace_reason_line = lines_with_verb(&lines, "transition-begin")
        .first()
        .copied()
        .map(|s| s.to_string());

    assert_eq!(
        journal_reason,
        Some(TransitionReason::Event { event: 0 }),
        "expected first transition driven by Ev::Go (event id 0)"
    );
    assert!(
        trace_reason_line
            .as_deref()
            .is_some_and(|l| l.contains("reason=event:Go")),
        "trace reason should match journal reason: {trace_reason_line:?}"
    );
}
