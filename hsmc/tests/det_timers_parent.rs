#![allow(dead_code)]
//! Deterministic-flow tests for parent-vs-child timer interaction.
//!
//! Spec: "Descending into a child state is not an exit — the parent's
//! timers keep running."

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use core::time::Duration;
use hsmc::{statechart, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Halt,
}

// Parent has a 100ms timer. Default-descend into ChildA. ChildA has no timer.
statechart! {
    Pt {
        context: Ctx;
        events:  Ev;
        default(Parent);
        terminate(Halt);

        state Parent {
            entry: p_in;
            exit:  p_out;
            default(ChildA);
            on(after Duration::from_millis(100)) => Done;
            state ChildA {
                entry: ca_in;
                exit:  ca_out;
            }
        }
        state Done {
            entry: d_in;
        }
    }
}

impl PtActions for PtActionContext<'_> {
    async fn p_in(&mut self) {}
    async fn p_out(&mut self) {}
    async fn ca_in(&mut self) {}
    async fn ca_out(&mut self) {}
    async fn d_in(&mut self) {}
}

const SP: u16 = 1;
const T_PARENT: u16 = 0;

#[tokio::test(flavor = "current_thread")]
async fn det_parent_timer_armed_before_child_entered() {
    // Parent's timer arms during Parent's entry; THEN default-descends into ChildA.
    // So the order is: TimerArmed(Parent) before Entered(ChildA).
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Pt::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            let j = m.take_journal();
            let armed_idx = j
                .iter()
                .position(|e| matches!(e, TraceEvent::TimerArmed { state: SP, .. }))
                .unwrap();
            let child_entered_idx = j
                .iter()
                .position(|e| {
                    matches!(
                        e,
                        TraceEvent::Entered { state: 2 } // ChildA = id 2
                    )
                })
                .unwrap();
            assert!(
                armed_idx < child_entered_idx,
                "TimerArmed (Parent) at {} must precede Entered (ChildA) at {}",
                armed_idx,
                child_entered_idx
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_parent_timer_only_one_arm_in_initial_descent() {
    // Only Parent has a timer. ChildA has none.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Pt::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            let armed: Vec<u16> = m
                .journal()
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::TimerArmed { state, .. } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(armed, vec![SP]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_parent_timer_cancelled_on_termination() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Pt::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            let cancels: Vec<&TraceEvent> = m
                .journal()
                .iter()
                .filter(|e| matches!(e, TraceEvent::TimerCancelled { .. }))
                .collect();
            // Termination unwinds all states. Parent's timer is cancelled when Parent exits.
            assert_eq!(
                cancels.len(),
                1,
                "expected exactly 1 TimerCancelled (Parent's), got {:?}",
                cancels
            );
            assert!(matches!(
                cancels[0],
                TraceEvent::TimerCancelled {
                    state: SP,
                    timer: T_PARENT
                }
            ));
        })
        .await;
}
