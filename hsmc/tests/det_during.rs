#![allow(dead_code)]
//! Deterministic-flow tests for `during:` activities.
//!
//! Spec section "during: activities":
//!   A `during:` is an async function that runs while its owning state is on
//!   the active path. It is started after the entry actions of that state
//!   finish, and it is cancelled before the exit actions of that state begin.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};
use core::time::Duration;

#[derive(Default)]
pub struct Ctx {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Done,
    Halt,
}

statechart! {
    Du {
        context: Ctx;
        events:  Ev;
        default(Working);
        terminate(Halt);

        state Working {
            entry: w_in;
            exit:  w_out;
            during: tick_a(a);
            during: tick_b(b);
            on(Done) => Idle;
        }
        state Idle {
            entry: i_in;
            exit:  i_out;
        }
    }
}

impl DuActions for DuActionContext<'_> {
    async fn w_in(&mut self) {}
    async fn w_out(&mut self) {}
    async fn i_in(&mut self) {}
    async fn i_out(&mut self) {}
}

async fn tick_a(_a: &mut u32) -> Ev {
    // Pause for a long time; this future will likely be cancelled before
    // completing so we exercise the cancel path.
    tokio::time::sleep(Duration::from_secs(60)).await;
    Ev::Done
}

async fn tick_b(_b: &mut u32) -> Ev {
    tokio::time::sleep(Duration::from_secs(60)).await;
    Ev::Done
}

const SR: u16 = 0;
const SW: u16 = 1;
const SI: u16 = 2;

#[tokio::test(flavor = "current_thread")]
async fn det_during_started_after_entries() {
    // Spec: started AFTER entry actions of the owning state finish.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.take_journal();
        // Find Working's entry action invocation.
        let w_entry_idx = j.iter().position(|e| matches!(
            e, TraceEvent::ActionInvoked { state: SW, kind: ActionKind::Entry, .. }
        ));
        // Find first DuringStarted on Working.
        let d_started_idx = j.iter().position(|e| matches!(
            e, TraceEvent::DuringStarted { state: SW, .. }
        ));
        assert!(w_entry_idx.is_some());
        assert!(d_started_idx.is_some());
        assert!(w_entry_idx.unwrap() < d_started_idx.unwrap(),
            "DuringStarted must come AFTER entry actions");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_cancelled_before_exits() {
    // Spec: cancelled BEFORE exit actions of the owning state begin.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.take_journal();
        let d_cancelled_idx = j.iter().position(|e| matches!(
            e, TraceEvent::DuringCancelled { state: SW, .. }
        ));
        let w_exit_idx = j.iter().position(|e| matches!(
            e, TraceEvent::ActionInvoked { state: SW, kind: ActionKind::Exit, .. }
        ));
        assert!(d_cancelled_idx.is_some(), "DuringCancelled must appear");
        assert!(w_exit_idx.is_some(), "exit actions must run");
        assert!(d_cancelled_idx.unwrap() < w_exit_idx.unwrap(),
            "DuringCancelled must come BEFORE exit actions");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_two_durings_two_started_two_cancelled() {
    // Two durings declared on Working — should see two DuringStarted and two DuringCancelled.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.take_journal();
        let started = j.iter().filter(|e| matches!(
            e, TraceEvent::DuringStarted { state: SW, .. }
        )).count();
        let cancelled = j.iter().filter(|e| matches!(
            e, TraceEvent::DuringCancelled { state: SW, .. }
        )).count();
        assert_eq!(started, 2);
        assert_eq!(cancelled, 2);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_indices_are_declaration_order() {
    // Indices 0, 1 in declaration order.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let started_indices: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::DuringStarted { during, .. } => Some(*during),
            _ => None,
        }).collect();
        assert_eq!(started_indices, vec![0, 1]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_cancelled_indices_match() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let cancelled_indices: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::DuringCancelled { during, .. } => Some(*during),
            _ => None,
        }).collect();
        // Cancellation also walks declaration order in the current implementation.
        assert_eq!(cancelled_indices, vec![0, 1]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_state_id_matches() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Halt).await;
        let started_states: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::DuringStarted { state, .. } => Some(*state),
            _ => None,
        }).collect();
        // Both DuringStarted events refer to Working (id 1).
        assert!(started_states.iter().all(|&s| s == SW));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_no_durings_on_idle() {
    // Idle has no `during:` declarations. After we transition into Idle, no
    // DuringStarted for Idle.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Du::new(Ctx::default());
        let _ = m.dispatch(Ev::Done).await;
        let on_idle: usize = m.journal().iter().filter(|e| matches!(
            e, TraceEvent::DuringStarted { state: SI, .. }
        )).count();
        assert_eq!(on_idle, 0);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_during_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Du::new(Ctx::default());
            let _ = m.dispatch(Ev::Done).await;
            let _ = m.dispatch(Ev::Halt).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}
