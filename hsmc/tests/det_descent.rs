#![allow(dead_code)]
//! Deterministic-flow tests for DEFAULT-DESCENT into composite targets.
//!
//! Spec section "Default child":
//!   Whenever a state with children is entered, the default child is entered
//!   immediately after that state's entry actions finish — it's effectively
//!   an automatic transition into the child.
//!
//! Pins down: entries fire outer-to-inner, default-descent unrolls the chain,
//! the leaf is the innermost active state at the end.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Halt,
}

// Hierarchy: Root → Idle, Root → Tower → L1 → L2 → L3
statechart! {
    Desc {
        context: Ctx;
        events:  Ev;
        default(Idle);
        terminate(Halt);

        state Idle {
            entry: idle_in;
            exit:  idle_out;
            on(Go) => Tower;
        }
        state Tower {
            entry: tower_in;
            exit:  tower_out;
            default(L1);
            state L1 {
                entry: l1_in;
                exit:  l1_out;
                default(L2);
                state L2 {
                    entry: l2_in;
                    exit:  l2_out;
                    default(L3);
                    state L3 {
                        entry: l3_in;
                        exit:  l3_out;
                    }
                }
            }
        }
    }
}

impl DescActions for DescActionContext<'_> {
    async fn idle_in(&mut self) {}
    async fn idle_out(&mut self) {}
    async fn tower_in(&mut self) {}
    async fn tower_out(&mut self) {}
    async fn l1_in(&mut self) {}
    async fn l1_out(&mut self) {}
    async fn l2_in(&mut self) {}
    async fn l2_out(&mut self) {}
    async fn l3_in(&mut self) {}
    async fn l3_out(&mut self) {}
}

const SR: u16 = 0;
const SI: u16 = 1;
const ST: u16 = 2;
const SL1: u16 = 3;
const SL2: u16 = 4;
const SL3: u16 = 5;
const A_IDLE_IN: u16 = 0;
const A_IDLE_OUT: u16 = 1;
const A_TOWER_IN: u16 = 2;
const A_TOWER_OUT: u16 = 3;
const A_L1_IN: u16 = 4;
const A_L1_OUT: u16 = 5;
const A_L2_IN: u16 = 6;
const A_L2_OUT: u16 = 7;
const A_L3_IN: u16 = 8;
const A_L3_OUT: u16 = 9;
const E_GO: u16 = 0;

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
async fn det_descent_target_with_three_default_levels() {
    // Idle → Tower: Tower has default chain L1 → L2 → L3.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Desc::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            let actual = m.take_journal();

            let expected = vec![
                TraceEvent::Started {
                    chart_hash: Desc::<8>::CHART_HASH,
                },
                TraceEvent::Entered { state: SR },
                TraceEvent::Entered { state: SI },
                entry(SI, A_IDLE_IN),
                TraceEvent::EventDelivered {
                    handler_state: SI,
                    event: E_GO,
                },
                TraceEvent::TransitionFired {
                    from: Some(SI),
                    to: ST,
                },
                exit_(SI, A_IDLE_OUT),
                TraceEvent::Exited { state: SI },
                TraceEvent::Entered { state: ST },
                entry(ST, A_TOWER_IN),
                TraceEvent::Entered { state: SL1 },
                entry(SL1, A_L1_IN),
                TraceEvent::Entered { state: SL2 },
                entry(SL2, A_L2_IN),
                TraceEvent::Entered { state: SL3 },
                entry(SL3, A_L3_IN),
            ];

            assert_eq!(actual, expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_descent_entries_fire_outer_to_inner() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Desc::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            // After the GO transition: order of Entered events should be Tower → L1 → L2 → L3.
            let split = m
                .journal()
                .iter()
                .position(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .unwrap();
            let entered: Vec<u16> = m.journal()[split..]
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(entered, vec![ST, SL1, SL2, SL3]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_descent_termination_exits_full_chain() {
    // After full descent into L3, terminate exits L3 → L2 → L1 → Tower → Root.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Desc::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            let _ = m.dispatch(Ev::Halt).await;
            let exits: Vec<u16> = m
                .journal()
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(exits, vec![SI, SL3, SL2, SL1, ST, SR]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_descent_action_kind_sequence() {
    // After GO: Tower entry kind, L1 entry, L2 entry, L3 entry. All Entry.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Desc::new(Ctx);
            let _ = m.dispatch(Ev::Go).await;
            let split = m
                .journal()
                .iter()
                .position(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .unwrap();
            // After the Idle exit, all subsequent ActionInvoked must be Entry kind.
            let kinds_after_exit: Vec<ActionKind> = m.journal()[split + 3..]
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::ActionInvoked { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            assert!(
                kinds_after_exit.iter().all(|k| *k == ActionKind::Entry),
                "all post-transition actions must be Entry kind, got {:?}",
                kinds_after_exit
            );
            assert_eq!(kinds_after_exit.len(), 4, "Tower + L1 + L2 + L3 entries");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_descent_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut m = Desc::new(Ctx);
                let _ = m.dispatch(Ev::Go).await;
                let _ = m.dispatch(Ev::Halt).await;
                m.take_journal()
            })
            .await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_descent_initial_descent_is_idle_only() {
    // Initial default chain is Root → Idle. Idle has no children, so only one Entered.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Desc::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            let entered: Vec<u16> = m
                .journal()
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(entered, vec![SR, SI]);
        })
        .await;
}
