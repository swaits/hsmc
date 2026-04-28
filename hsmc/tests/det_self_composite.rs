#![allow(dead_code)]
//! Deterministic-flow tests for SELF-TRANSITION on a composite (re-enters with default-descend).
//!
//! Spec section "Transitioning to yourself":
//!   If the target happens to be a composite (it has children, but no child is
//!   active right now because you got there via an up-transition), the same
//!   default-child rule still applies on re-entry: exit the composite,
//!   re-enter it, then descend through its default chain to a leaf.
//!
//! To get a composite as the innermost active state, this chart uses:
//!   1. UpToParent (D → Parent) → Parent is now innermost composite.
//!   2. SelfMid (Parent → Parent) → exit Parent, re-enter Parent, default-descend.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    UpToParent, // D → Parent (up-transition)
    SelfParent, // Parent → Parent (self-transition on composite)
    Halt,
}

statechart! {
    Sc {
        context: Ctx;
        events:  Ev;
        default(Parent);
        terminate(Halt);

        state Parent {
            entry: p_in;
            exit:  p_out;
            default(Child);
            on(SelfParent) => Parent;
            state Child {
                entry: c_in;
                exit:  c_out;
                default(Leaf);
                state Leaf {
                    entry: l_in;
                    exit:  l_out;
                    on(UpToParent) => Parent;
                }
            }
        }
    }
}

impl ScActions for ScActionContext<'_> {
    async fn p_in(&mut self) {}
    async fn p_out(&mut self) {}
    async fn c_in(&mut self) {}
    async fn c_out(&mut self) {}
    async fn l_in(&mut self) {}
    async fn l_out(&mut self) {}
}

const SR: u16 = 0;
const SP: u16 = 1;
const SC: u16 = 2;
const SL: u16 = 3;

#[tokio::test(flavor = "current_thread")]
async fn det_self_composite_default_descends_after_re_entry() {
    // Path: initial → Leaf. UpToParent → Parent (up). SelfParent → exit Parent,
    // re-enter Parent, default-descend (Parent → Child → Leaf).
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Sc::new(Ctx);
            let _ = m.dispatch(Ev::UpToParent).await;
            let _ = m.dispatch(Ev::SelfParent).await;
            let j = m.take_journal();

            // After SelfParent, expect: exit Parent → re-enter Parent → enter Child → enter Leaf.
            let self_idx = j
                .iter()
                .rposition(|e| {
                    matches!(
                        e,
                        TraceEvent::TransitionFired {
                            from: Some(SP),
                            to: SP,
                            ..
                        }
                    )
                })
                .unwrap();
            let after = &j[self_idx..];

            let exits: Vec<u16> = after
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            let entered: Vec<u16> = after
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();

            assert_eq!(exits, vec![SP], "self-transition exits target only");
            assert_eq!(
                entered,
                vec![SP, SC, SL],
                "re-enter target then default-descend"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_composite_action_kinds_in_order() {
    // After the SelfParent transition: Exit p_out, Entry p_in, Entry c_in, Entry l_in.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Sc::new(Ctx);
            let _ = m.dispatch(Ev::UpToParent).await;
            let _ = m.dispatch(Ev::SelfParent).await;
            let j = m.take_journal();
            let self_idx = j
                .iter()
                .rposition(|e| {
                    matches!(
                        e,
                        TraceEvent::TransitionFired {
                            from: Some(SP),
                            to: SP,
                            ..
                        }
                    )
                })
                .unwrap();
            let kinds: Vec<ActionKind> = j[self_idx..]
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::ActionInvoked { kind, .. } => Some(*kind),
                    _ => None,
                })
                .collect();
            assert_eq!(
                kinds,
                vec![
                    ActionKind::Exit,
                    ActionKind::Entry,
                    ActionKind::Entry,
                    ActionKind::Entry
                ],
                "exit Parent, then enter Parent, Child, Leaf"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_composite_byte_deterministic() {
    let run = || async {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut m = Sc::new(Ctx);
                let _ = m.dispatch(Ev::UpToParent).await;
                let _ = m.dispatch(Ev::SelfParent).await;
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
