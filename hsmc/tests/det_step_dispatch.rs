#![allow(dead_code)]
//! Deterministic-flow tests for `step()` vs `dispatch()` equivalence.
//!
//! Spec section "step() vs run() vs dispatch()":
//!   For the same external event sequence and the same elapsed-time inputs,
//!   the three produce the same observable trace.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Tick,
    Halt,
}

statechart! {
    Eq {
        context: Ctx;
        events:  Ev;
        default(A);
        terminate(Halt);

        state A {
            entry: a_in;
            exit:  a_out;
            on(Tick) => B;
        }
        state B {
            entry: b_in;
            exit:  b_out;
            on(Tick) => A;
        }
    }
}

impl EqActions for EqActionContext<'_> {
    async fn a_in(&mut self) {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self) {}
    async fn b_out(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn det_dispatch_drains_to_quiescence() {
    // dispatch(ev) processes ev plus any events emitted while handling ev.
    // After the await returns, the queue is empty.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Eq::new(Ctx);
            let _ = m.dispatch(Ev::Tick).await;
            assert!(!m.has_pending_events());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_step_and_dispatch_produce_same_journal() {
    // Drive one machine via dispatch, another via send + step until queue empty.
    // The journals must match.
    tokio::task::LocalSet::new()
        .run_until(async {
            // Path 1: dispatch.
            let mut m1 = Eq::new(Ctx);
            let _ = m1.dispatch(Ev::Tick).await;
            let _ = m1.dispatch(Ev::Tick).await;
            let _ = m1.dispatch(Ev::Halt).await;
            let j_dispatch = m1.take_journal();

            // Path 2: send + step.
            let mut m2 = Eq::new(Ctx);
            m2.send(Ev::Tick).unwrap();
            m2.send(Ev::Tick).unwrap();
            m2.send(Ev::Halt).unwrap();
            // Drive to quiescence with explicit steps.
            for _ in 0..20 {
                let _ = m2.step(::hsmc::Duration::ZERO).await;
                if m2.is_terminated() {
                    break;
                }
            }
            let j_step = m2.take_journal();

            assert_eq!(
                j_dispatch, j_step,
                "step and dispatch must produce identical journals for the same event sequence"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_step_is_at_most_one_unit_of_work() {
    // Each step() processes at most one event or one timer.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Eq::new(Ctx);
            m.send(Ev::Tick).unwrap();
            m.send(Ev::Tick).unwrap();
            m.send(Ev::Halt).unwrap();

            // After the first step (initial enter): chart enters root → A.
            let _ = m.step(::hsmc::Duration::ZERO).await;
            // After the second step: process Tick #1 (A → B).
            let len_after_2 = {
                let _ = m.step(::hsmc::Duration::ZERO).await;
                m.journal().len()
            };
            // After the third step: process Tick #2 (B → A).
            let _ = m.step(::hsmc::Duration::ZERO).await;
            let len_after_3 = m.journal().len();

            assert!(
                len_after_3 > len_after_2,
                "step processes additional events"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_step_repeated_idempotent_when_queue_empty() {
    // Once initial-enter is done and queue is empty, additional steps
    // (with elapsed=ZERO) are no-ops.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Eq::new(Ctx);
            let _ = m.step(::hsmc::Duration::ZERO).await; // initial entry
            let len_after_first = m.journal().len();
            for _ in 0..10 {
                let _ = m.step(::hsmc::Duration::ZERO).await;
            }
            // After the first step, no events are pending, so additional
            // step(ZERO) calls do nothing.
            assert_eq!(
                m.journal().len(),
                len_after_first,
                "step(ZERO) on empty queue must not change journal"
            );
        })
        .await;
}
