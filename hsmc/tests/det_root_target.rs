#![allow(dead_code)]
//! Pins down that the ROOT state is targetable by the chart's own name.
//!
//! Spec section "What a chart contains":
//!   State — a named node. The root state (the machine itself) is just a
//!   state. Every state is just a state.
//!
//! Spec section "How transitions work":
//!   The target can be any state in the entire chart. ... state names must
//!   be unique across the chart.
//!
//! From these two together: `on(Trig) => <ChartName>;` is a valid transition
//! whose target is the root. From a leaf, this is an up-transition; from any
//! state inside the chart it's an up-transition (root is always on the active
//! path) — exiting only the states strictly between current and root.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent, TransitionReason};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    UpToRoot,
    Halt,
}

statechart! {
    Sub {
        context: Ctx;
        events:  Ev;
        default(A);
        terminate(Halt);

        entry: r_in;
        exit:  r_out;

        state A {
            entry: a_in;
            exit:  a_out;
            default(B);
            state B {
                entry: b_in;
                exit:  b_out;
                on(UpToRoot) => Sub;  // ← target is the chart name = root
            }
        }
    }
}

impl SubActions for SubActionContext<'_> {
    async fn r_in(&mut self) {}
    async fn r_out(&mut self) {}
    async fn a_in(&mut self) {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self) {}
    async fn b_out(&mut self) {}
}

const SR: u16 = 0;
const SA: u16 = 1;
const SB: u16 = 2;
const A_R_IN: u16 = 0;
const A_R_OUT: u16 = 1;
const A_A_IN: u16 = 2;
const A_A_OUT: u16 = 3;
const A_B_IN: u16 = 4;
const A_B_OUT: u16 = 5;
const E_UPTOROOT: u16 = 0;

fn entry(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked {
        state,
        action,
        kind: ActionKind::Entry,
    }
}
fn exit_(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked {
        state,
        action,
        kind: ActionKind::Exit,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_root_targetable_by_chart_name() {
    // Chart compiles. Targeting root by chart name resolves correctly.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Sub::new(Ctx);
            let _ = m.dispatch(Ev::UpToRoot).await;
            let actual = m.take_journal();

            // Up-transition to root from B: exit B and A. Root is target,
            // not exited, not re-entered, no default-descent.
            let expected = vec![
                TraceEvent::Started {
                    chart_hash: Sub::<8>::CHART_HASH,
                },
                TraceEvent::EnterBegan { state: SR },
                entry(SR, A_R_IN),
                TraceEvent::Entered { state: SR },
                TraceEvent::EnterBegan { state: SA },
                entry(SA, A_A_IN),
                TraceEvent::Entered { state: SA },
                TraceEvent::EnterBegan { state: SB },
                entry(SB, A_B_IN),
                TraceEvent::Entered { state: SB },
                TraceEvent::EventReceived { event: E_UPTOROOT },
                TraceEvent::EventDelivered {
                    handler_state: SB,
                    event: E_UPTOROOT,
                },
                TraceEvent::TransitionFired {
                    from: Some(SB),
                    to: SR,
                    reason: TransitionReason::Event { event: E_UPTOROOT },
                },
                TraceEvent::ExitBegan { state: SB },
                exit_(SB, A_B_OUT),
                TraceEvent::Exited { state: SB },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::TransitionComplete {
                    from: Some(SB),
                    to: SR,
                },
            ];

            assert_eq!(
                actual, expected,
                "root targetable; up-transition to root exits A and B only"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_root_target_does_not_re_enter_root() {
    // After the up-to-root, root must NOT have re-entered.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Sub::new(Ctx);
            let _ = m.dispatch(Ev::UpToRoot).await;
            let j = m.take_journal();
            // After the TransitionFired, no Entered event should fire.
            let split = j
                .iter()
                .position(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .unwrap();
            let entered_after = j[split..]
                .iter()
                .filter(|e| matches!(e, TraceEvent::Entered { .. }))
                .count();
            assert_eq!(
                entered_after, 0,
                "up-to-root must not re-enter root or default-descend"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_root_target_then_terminate() {
    // After up-to-root, root is the innermost active state. Terminate exits root only.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Sub::new(Ctx);
            let _ = m.dispatch(Ev::UpToRoot).await;
            let _ = m.dispatch(Ev::Halt).await;
            let j = m.take_journal();
            // After UpToRoot we're in root only. Halt exits root only.
            let exits: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(
                exits,
                vec![SB, SA, SR],
                "up-to-root exits B, A; terminate exits root"
            );
            assert!(matches!(j.last(), Some(TraceEvent::Terminated)));
        })
        .await;
}
