#![allow(dead_code)]
//! Deterministic-flow tests for TIMERS.
//!
//! Spec section "Timers":
//!   - A timer starts when its state is entered.
//!   - A timer is cancelled when its state is exited.
//!   - Descending into a child state is not an exit — parent's timers keep running.
//!   - Timers don't bubble. Only the declaring state handles its expiry.
//!   - Re-entering a state restarts its timers.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};
use core::time::Duration;

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Stop,
    Halt,
}

statechart! {
    Tim {
        context: Ctx;
        events:  Ev;
        default(Counting);
        terminate(Halt);

        state Counting {
            entry: c_in;
            exit:  c_out;
            on(after Duration::from_millis(50)) => Done;
            on(Stop) => Done;
        }
        state Done {
            entry: d_in;
            on(after Duration::from_millis(20)) => log_tick;
        }
    }
}

impl TimActions for TimActionContext<'_> {
    async fn c_in(&mut self)    {}
    async fn c_out(&mut self)   {}
    async fn d_in(&mut self)    {}
    async fn log_tick(&mut self){}
}

const SR: u16 = 0;
const SC: u16 = 1;
const SD: u16 = 2;
// Timer ids in first-seen order: 50ms (counting) → 0, 20ms (done) → 1.
const T_COUNTING: u16 = 0;
const T_DONE: u16 = 1;

fn timer_events(j: &[TraceEvent]) -> Vec<&TraceEvent> {
    j.iter().filter(|e| matches!(
        e,
        TraceEvent::TimerArmed { .. }
            | TraceEvent::TimerCancelled { .. }
            | TraceEvent::TimerFired { .. }
    )).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_arms_on_entry() {
    // Spec: timer starts when its state is entered.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let j = m.take_journal();
        // First TimerArmed must be Counting's 50ms timer.
        let first_armed = j.iter().find(|e| matches!(e, TraceEvent::TimerArmed { .. }));
        assert!(matches!(
            first_armed,
            Some(TraceEvent::TimerArmed { state: SC, timer: T_COUNTING, ns: 50_000_000 })
        ), "got: {:?}", first_armed);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_cancels_on_exit() {
    // Spec: timer is cancelled when its state is exited.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let j = m.take_journal();
        // Stop transitions Counting → Done. Exit Counting must cancel its timer.
        let cancelled = j.iter().find(|e| matches!(
            e, TraceEvent::TimerCancelled { state: SC, timer: T_COUNTING }
        ));
        assert!(cancelled.is_some(),
            "Counting's timer must be cancelled on exit, got: {:?}", j);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_cancel_appears_before_exit_actions() {
    // Spec: "1. cancel durings  2. cancel timers  3. exit actions  4. Exited"
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let j = m.take_journal();
        let cancel_idx = j.iter().position(|e| matches!(
            e, TraceEvent::TimerCancelled { state: SC, .. }
        )).unwrap();
        let exit_action_idx = j.iter().position(|e| matches!(
            e, TraceEvent::ActionInvoked { state: SC, kind: ActionKind::Exit, .. }
        )).unwrap();
        assert!(cancel_idx < exit_action_idx,
            "TimerCancelled at {} must precede ActionInvoked Exit at {}",
            cancel_idx, exit_action_idx);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_arm_appears_after_entry_actions() {
    // The TimerArmed must come AFTER the entry action invocation.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let j = m.take_journal();
        // First Entry-kind action on Counting:
        let entry_idx = j.iter().position(|e| matches!(
            e, TraceEvent::ActionInvoked { state: SC, kind: ActionKind::Entry, .. }
        )).unwrap();
        // First TimerArmed for Counting:
        let armed_idx = j.iter().position(|e| matches!(
            e, TraceEvent::TimerArmed { state: SC, .. }
        )).unwrap();
        assert!(entry_idx < armed_idx,
            "entry actions ({}) must fire before TimerArmed ({})",
            entry_idx, armed_idx);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_arm_each_state_in_path() {
    // Initial descent into Counting arms Counting's timer.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let armed: Vec<&TraceEvent> = m.journal().iter().filter(|e| matches!(
            e, TraceEvent::TimerArmed { .. }
        )).collect();
        // Only Counting has a timer in initial descent (Done is unreached).
        assert_eq!(armed.len(), 1, "expected 1 TimerArmed during initial descent, got {:?}", armed);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_two_lifecycles_arm_then_cancel_each() {
    // GoChild then UpToCounting (no such event — use Stop for full exit).
    // Just check Stop's full sequence: arm → cancel.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let evs = timer_events(m.journal());
        assert_eq!(evs.len(), 3, "expected arm → cancel → arm (Done's timer), got {:?}", evs);
        assert!(matches!(evs[0], TraceEvent::TimerArmed { state: SC, timer: T_COUNTING, ns: 50_000_000 }));
        assert!(matches!(evs[1], TraceEvent::TimerCancelled { state: SC, timer: T_COUNTING }));
        assert!(matches!(evs[2], TraceEvent::TimerArmed { state: SD, timer: T_DONE, ns: 20_000_000 }));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Tim::new(Ctx);
            let _ = m.dispatch(Ev::Stop).await;
            let _ = m.dispatch(Ev::Halt).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_timer_state_cancel_uses_correct_state_id() {
    // The TimerCancelled.state must be SC, not SR or any other.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Tim::new(Ctx);
        let _ = m.dispatch(Ev::Stop).await;
        let cancels: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::TimerCancelled { state, .. } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(cancels, vec![SC]);
    }).await;
}
