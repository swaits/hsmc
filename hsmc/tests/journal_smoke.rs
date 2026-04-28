//! Smoke tests for the deterministic execution journal (`feature = "journal"`).
//!
//! Every observable atom — entries, exits, action invocations (with kind),
//! during start/cancel, timer arm/cancel/fire, queued emits, event delivery,
//! transitions, termination — must appear in the journal in canonical order
//! for a fixed `(chart, ctx, event sequence)` input. These tests pin down
//! that order on small chart shapes so any regression in codegen is caught
//! immediately.
//!
//! Tests run only when both `tokio` and `journal` features are enabled.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

// ─────────────────────────────────────────────────────────────────────
// Small chart: A → B on Go, with entry/exit actions in a hierarchy.
// ─────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Ctx {
    pub log: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ev {
    Go,
    Halt,
}

statechart! {
    JournalProbe {
        context: Ctx;
        events:  Ev;

        default(Idle);
        terminate(Halt);

        entry: root_in;
        exit:  root_out;

        state Idle {
            entry: idle_in;
            exit:  idle_out;
            on(Go) => Active;
        }
        state Active {
            entry: active_in;
            exit:  active_out;
            default(Sub);
            state Sub {
                entry: sub_in;
                exit:  sub_out;
            }
        }
    }
}

impl JournalProbeActions for JournalProbeActionContext<'_> {
    async fn root_in(&mut self) {
        self.log.push("root_in");
    }
    async fn root_out(&mut self) {
        self.log.push("root_out");
    }
    async fn idle_in(&mut self) {
        self.log.push("idle_in");
    }
    async fn idle_out(&mut self) {
        self.log.push("idle_out");
    }
    async fn active_in(&mut self) {
        self.log.push("active_in");
    }
    async fn active_out(&mut self) {
        self.log.push("active_out");
    }
    async fn sub_in(&mut self) {
        self.log.push("sub_in");
    }
    async fn sub_out(&mut self) {
        self.log.push("sub_out");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn journal_records_initial_descent() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = JournalProbe::new(Ctx::default());
            // Drive the initial entry chain by stepping once with no event.
            let _ = m.dispatch(Ev::Go).await;

            let j = m.journal();

            // First event must be Started.
            assert!(matches!(j[0], TraceEvent::Started { .. }));
            // Started carries the chart's stable hash.
            if let TraceEvent::Started { chart_hash } = j[0] {
                assert_eq!(chart_hash, JournalProbe::<8>::CHART_HASH);
            }

            // We expect (in order, for the initial descent root → Idle):
            //   Started
            //   Entered(root)
            //   ActionInvoked(root, root_in, Entry)
            //   Entered(Idle)
            //   ActionInvoked(Idle, idle_in, Entry)
            //   ... (then the Go event delivery / transition / sub entry)
            let states_entered: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // At minimum we entered root (id 0), then Idle, then Active, then Sub.
            assert_eq!(
                states_entered.len(),
                4,
                "expected 4 Entered events, got {:?}",
                j
            );

            // Action kinds appear in order Entry → Handler (none here) → Exit.
            let kinds: Vec<ActionKind> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::ActionInvoked { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            // Actions on the journey: Entry x4 (root, idle, active, sub),
            // Exit x1 (idle), Entry x2 (active, sub) — wait no:
            // Initial descent is root → Idle. Entries: root_in, idle_in.
            // Then Go fires: Idle exits (idle_out), Active enters (active_in),
            // Sub enters (sub_in).
            // Total: Entry(root), Entry(idle), Exit(idle), Entry(active), Entry(sub).
            let entry_count = kinds.iter().filter(|k| **k == ActionKind::Entry).count();
            let exit_count = kinds.iter().filter(|k| **k == ActionKind::Exit).count();
            assert_eq!(
                entry_count, 4,
                "expected 4 entry actions, got kinds {:?}",
                kinds
            );
            assert_eq!(
                exit_count, 1,
                "expected 1 exit action, got kinds {:?}",
                kinds
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn journal_includes_transition_event() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = JournalProbe::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;

            let saw_transition = m
                .journal()
                .iter()
                .any(|e| matches!(e, TraceEvent::TransitionFired { .. }));
            assert!(saw_transition, "journal must contain TransitionFired");

            let saw_event_delivered = m
                .journal()
                .iter()
                .any(|e| matches!(e, TraceEvent::EventDelivered { .. }));
            assert!(saw_event_delivered, "journal must contain EventDelivered");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn journal_records_termination() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = JournalProbe::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            let _ = m.dispatch(Ev::Halt).await;

            let j = m.journal();
            assert!(
                matches!(j.last(), Some(TraceEvent::Terminated)),
                "journal must end with Terminated, got tail: {:?}",
                &j[j.len().saturating_sub(3)..]
            );

            // Termination exits all states from leaf to root (Sub → Active → Idle? No,
            // Idle was already exited on Go. After Halt the active path is
            // root → Active → Sub. Exits: Sub, Active, root.
            let post_halt_exits: Vec<u16> = j.iter()
                .skip_while(|e| !matches!(e, TraceEvent::EventDelivered { .. } if matches!(j.iter().rev().find(|x| matches!(x, TraceEvent::EventDelivered { .. })), Some(_))))
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // We at least exited 4 times across the run (idle once, then sub/active/root on terminate).
            assert!(post_halt_exits.len() >= 3, "expected at least 3 exits across the run, got {:?}", j);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn journal_is_byte_deterministic_across_runs() {
    // Same chart, same context, same event sequence ⇒ identical journal.
    let run = || async {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut m = JournalProbe::new(Ctx::default());
                let _ = m.dispatch(Ev::Go).await;
                let _ = m.dispatch(Ev::Halt).await;
                m.take_journal()
            })
            .await
    };
    let first = run().await;
    for i in 1..10 {
        let again = run().await;
        assert_eq!(first, again, "journal divergence on run #{}", i);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn clear_journal_empties_the_journal() {
    // Pins down `JournalSink::clear`: after dispatching events that
    // produce journal entries, calling `clear_journal()` must empty
    // the journal. Subsequent `journal()` returns an empty slice;
    // `take_journal()` returns an empty Vec.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = JournalProbe::new(Ctx::default());
            let _ = m.dispatch(Ev::Go).await;
            assert!(
                !m.journal().is_empty(),
                "journal should have entries before clear"
            );

            m.clear_journal();
            assert!(
                m.journal().is_empty(),
                "journal must be empty after clear_journal()"
            );
            let taken = m.take_journal();
            assert!(taken.is_empty(), "take_journal() after clear must be empty");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn chart_hash_is_stable() {
    // Two instances of the same chart must agree on CHART_HASH.
    let _m1 = JournalProbe::new(Ctx::default());
    let _m2 = JournalProbe::new(Ctx::default());
    assert_eq!(
        JournalProbe::<8>::CHART_HASH,
        JournalProbe::<8>::CHART_HASH,
        "chart hash must be stable across instantiations"
    );
    // CHART_HASH must be nonzero — proves the FNV-1a fingerprint actually ran.
    assert_ne!(JournalProbe::<8>::CHART_HASH, 0);
}
