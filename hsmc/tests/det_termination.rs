#![allow(dead_code)]
//! Deterministic-flow tests for TERMINATION.
//!
//! Spec section "Termination":
//!   When a `terminate` event is received, the machine exits the entire active
//!   path bottom-up: innermost first, then each parent, up to and including
//!   the root. After that, the machine is done — `is_terminated()` is true,
//!   `run()` returns `Ok(())`, further `step()` calls are no-ops returning
//!   `None`. Any pending events in the queue are dropped. Any in-flight
//!   durings are dropped at their next `.await`.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, HsmcError, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Halt,
    Other,
}

statechart! {
    Term {
        context: Ctx;
        events:  Ev;
        default(Mid);
        terminate(Halt);

        entry: r_in;
        exit:  r_out;

        state Mid {
            entry: m_in;
            exit:  m_out;
            default(Leaf);
            on(Other) => log_other;
            state Leaf {
                entry: l_in;
                exit:  l_out;
            }
        }
    }
}

impl TermActions for TermActionContext<'_> {
    async fn r_in(&mut self)     {}
    async fn r_out(&mut self)    {}
    async fn m_in(&mut self)     {}
    async fn m_out(&mut self)    {}
    async fn l_in(&mut self)     {}
    async fn l_out(&mut self)    {}
    async fn log_other(&mut self){}
}

const SR: u16 = 0;
const SM: u16 = 1;
const SL: u16 = 2;
// Halt is interned via terminate(Halt). Other is used in `on(Other)` first.
// IR build order: handlers processed first per body. Mid has on(Other) → 0.
// Then Halt is interned at the end of build_ir → 1.
const E_OTHER: u16 = 0;
const E_HALT: u16 = 1;

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_records_terminate_requested() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.take_journal();
        // TerminateRequested fires before the bottom-up exits.
        let req_idx = j.iter().position(|e| matches!(
            e, TraceEvent::TerminateRequested { event: E_HALT }
        ));
        assert!(req_idx.is_some(), "TerminateRequested must appear, got: {:?}", j);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_exits_bottom_up() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let exits: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::Exited { state } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(exits, vec![SL, SM, SR], "termination exits inner-to-outer");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_terminated_is_last() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        assert!(matches!(m.journal().last(), Some(TraceEvent::Terminated)));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_dispatch_after_returns_already_terminated() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let res = m.dispatch(Ev::Other).await;
        assert!(matches!(res, Err(HsmcError::AlreadyTerminated)),
            "dispatch after terminate must return AlreadyTerminated, got {:?}", res);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_journal_unchanged_by_post_term_dispatch() {
    // Calling dispatch after terminate must not journal anything new.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let len_before = m.journal().len();
        let _ = m.dispatch(Ev::Other).await;
        let len_after = m.journal().len();
        assert_eq!(len_before, len_after,
            "journal length must not change after termination");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_exit_actions_run_in_order() {
    // Spec: exit actions inner-to-outer.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let exit_actions: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state, kind: ActionKind::Exit, .. } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(exit_actions, vec![SL, SM, SR]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_other_event_processed_before_halt() {
    // Other is dispatched first, then Halt. Other is delivered, then Halt
    // triggers termination. EventDelivered for Other appears, but no
    // EventDelivered for Halt — instead, TerminateRequested.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Term::new(Ctx);
        let _ = m.dispatch(Ev::Other).await;
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.take_journal();
        let other_delivered = j.iter().any(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_OTHER, .. }
        ));
        let halt_delivered = j.iter().any(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_HALT, .. }
        ));
        let halt_requested = j.iter().any(|e| matches!(
            e, TraceEvent::TerminateRequested { event: E_HALT }
        ));
        assert!(other_delivered, "Other must be delivered");
        assert!(!halt_delivered, "Halt does not go through EventDelivered");
        assert!(halt_requested, "Halt goes through TerminateRequested");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminate_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Term::new(Ctx);
            let _ = m.dispatch(Ev::Other).await;
            let _ = m.dispatch(Ev::Halt).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}
