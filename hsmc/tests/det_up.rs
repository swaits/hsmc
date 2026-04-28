#![allow(dead_code)]
//! Deterministic-flow tests for UP-TRANSITIONS (target is on active path, target ≠ I).
//!
//! Spec section "Transitioning to one of your parents (the important one)":
//!   When you transition to an ancestor of the current state, you exit the
//!   current state and any states between, but you do NOT re-enter the
//!   ancestor. ... You cannot enter a state you never left.
//!
//! Spec invariants for up-transitions:
//!   - The ancestor's entry actions don't fire.
//!   - The ancestor's `default` is not followed.
//!   - The ancestor's timers don't restart.
//!   - The ancestor's durings keep running.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    UpToA,
    UpToB,
    UpToC,
    Halt,
}

// Hierarchy: Root → A → B → C → D
// Up-transitions on D and intermediate states fire to ancestors.
statechart! {
    Up {
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
                default(C);
                state C {
                    entry: c_in;
                    exit:  c_out;
                    default(D);
                    state D {
                        entry: d_in;
                        exit:  d_out;
                        on(UpToA) => A;
                        on(UpToB) => B;
                        on(UpToC) => C;
                    }
                }
            }
        }
    }
}

impl UpActions for UpActionContext<'_> {
    async fn r_in(&mut self) {}
    async fn r_out(&mut self) {}
    async fn a_in(&mut self) {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self) {}
    async fn b_out(&mut self) {}
    async fn c_in(&mut self) {}
    async fn c_out(&mut self) {}
    async fn d_in(&mut self) {}
    async fn d_out(&mut self) {}
}

const SR: u16 = 0;
const SA: u16 = 1;
const SB: u16 = 2;
const SC: u16 = 3;
const SD: u16 = 4;
const A_R_IN: u16 = 0;
const A_R_OUT: u16 = 1;
const A_A_IN: u16 = 2;
const A_A_OUT: u16 = 3;
const A_B_IN: u16 = 4;
const A_B_OUT: u16 = 5;
const A_C_IN: u16 = 6;
const A_C_OUT: u16 = 7;
const A_D_IN: u16 = 8;
const A_D_OUT: u16 = 9;
// Event ids in first-seen order during IR build (D's handlers, in declaration order).
const E_UPTOA: u16 = 0;
const E_UPTOB: u16 = 1;
const E_UPTOC: u16 = 2;

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
fn initial_descent() -> Vec<TraceEvent> {
    vec![
        TraceEvent::Started {
            chart_hash: Up::<8>::CHART_HASH,
        },
        TraceEvent::Entered { state: SR },
        entry(SR, A_R_IN),
        TraceEvent::Entered { state: SA },
        entry(SA, A_A_IN),
        TraceEvent::Entered { state: SB },
        entry(SB, A_B_IN),
        TraceEvent::Entered { state: SC },
        entry(SC, A_C_IN),
        TraceEvent::Entered { state: SD },
        entry(SD, A_D_IN),
    ]
}
fn assert_journal(actual: &[TraceEvent], expected: &[TraceEvent]) {
    let mismatch = actual.iter().zip(expected.iter()).position(|(a, e)| a != e);
    if let Some(i) = mismatch {
        panic!(
            "mismatch at index {}\n  actual:   {:?}\n  expected: {:?}",
            i,
            actual.get(i),
            expected.get(i)
        );
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "length mismatch: actual {} vs expected {}",
        actual.len(),
        expected.len()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_to_immediate_grandparent() {
    // D → C: exit D only. C is target, not exited, not re-entered.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToC).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventDelivered {
                    handler_state: SD,
                    event: E_UPTOC,
                },
                TraceEvent::TransitionFired {
                    from: Some(SD),
                    to: SC,
                },
                exit_(SD, A_D_OUT),
                TraceEvent::Exited { state: SD },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_two_levels() {
    // D → B: exit D, exit C. B is target, not exited.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToB).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventDelivered {
                    handler_state: SD,
                    event: E_UPTOB,
                },
                TraceEvent::TransitionFired {
                    from: Some(SD),
                    to: SB,
                },
                exit_(SD, A_D_OUT),
                TraceEvent::Exited { state: SD },
                exit_(SC, A_C_OUT),
                TraceEvent::Exited { state: SC },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_three_levels_to_a() {
    // D → A: exit D, C, B. A is target, not exited.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToA).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventDelivered {
                    handler_state: SD,
                    event: E_UPTOA,
                },
                TraceEvent::TransitionFired {
                    from: Some(SD),
                    to: SA,
                },
                exit_(SD, A_D_OUT),
                TraceEvent::Exited { state: SD },
                exit_(SC, A_C_OUT),
                TraceEvent::Exited { state: SC },
                exit_(SB, A_B_OUT),
                TraceEvent::Exited { state: SB },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_target_not_re_entered() {
    // After D → A: A must NOT appear in any Entered event after the initial descent.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToA).await;
            let j = m.take_journal();
            let split = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EventDelivered { event: E_UPTOA, .. }))
                .unwrap();
            let a_re_entered = j[split..]
                .iter()
                .filter(|e| matches!(e, TraceEvent::Entered { state: SA }))
                .count();
            assert_eq!(
                a_re_entered, 0,
                "target A must not re-enter on up-transition"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_target_action_not_fired() {
    // After D → A: a_in must NOT fire after the initial descent.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToA).await;
            let j = m.take_journal();
            let split = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EventDelivered { event: E_UPTOA, .. }))
                .unwrap();
            let a_in_after = j[split..]
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        TraceEvent::ActionInvoked {
                            state: SA,
                            action: A_A_IN,
                            ..
                        }
                    )
                })
                .count();
            assert_eq!(
                a_in_after, 0,
                "target's entry action must not fire on up-transition"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_no_default_descent() {
    // After D → A: no default-descent. The journal stops with Exited(B).
    // A is the new innermost active state, with no active child.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToA).await;
            let j = m.take_journal();
            let split = j
                .iter()
                .position(|e| matches!(e, TraceEvent::EventDelivered { event: E_UPTOA, .. }))
                .unwrap();
            // Nothing after this slice should be Entered (no descent).
            let entered_after = j[split..]
                .iter()
                .filter(|e| matches!(e, TraceEvent::Entered { .. }))
                .count();
            assert_eq!(entered_after, 0, "up-transition: no default-descent");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_current_state_is_target_after() {
    // After D → B, the innermost active state is B (composite, no active child).
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToB).await;
            // current_state() should return B's public variant.
            // Indirect check: the most recent Entered (B's, from initial descent) is the
            // last Entered before the up-transition; nothing after.
            let j = m.take_journal();
            let last_entered = j
                .iter()
                .enumerate()
                .rev()
                .find(|(_, e)| matches!(e, TraceEvent::Entered { .. }))
                .map(|(i, _)| i);
            let split = j
                .iter()
                .position(|e| matches!(e, TraceEvent::TransitionFired { .. }))
                .unwrap();
            // The last Entered event must be BEFORE the transition.
            assert!(
                last_entered.unwrap() < split,
                "up-transition: no entries after the transition fires"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_up_chained_then_lateral() {
    // D → C, then C is a leaf with no children, so this becomes the new
    // innermost. From C we don't have any other handlers, so test ends here.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Up::new(Ctx);
            let _ = m.dispatch(Ev::UpToC).await;
            let _ = m.dispatch(Ev::Halt).await;
            let j = m.take_journal();
            // After UpToC, we should have Exited(D) but C remains. Halt then exits
            // C → B → A → Root.
            let exits: Vec<u16> = j
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(
                exits,
                vec![SD, SC, SB, SA, SR],
                "up to C, then terminate exits remaining path"
            );
        })
        .await;
}
