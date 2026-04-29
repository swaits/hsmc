#![allow(dead_code)]
//! Deterministic-flow tests for LATERAL transitions (target not on active path).
//!
//! Spec section "Transitioning to a sibling (or anywhere not on your active path)":
//!   The standard case. LCA is somewhere strictly above both current and
//!   target. Exit up to LCA, enter down to target, default-descend if the
//!   target has children.
//!
//! Also pins down that the LCA itself is **not** exited or re-entered.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent, TransitionReason};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    GoB,
    GoC,
    GoD,
    GoE,
    Halt,
}

// Hierarchy:
//   Root
//   ├── Parent
//   │   ├── A (default)
//   │   └── B
//   ├── C
//   └── D
//       └── E
statechart! {
    Lat {
        context: Ctx;
        events:  Ev;
        default(Parent);
        terminate(Halt);

        state Parent {
            entry: p_in;
            exit:  p_out;
            default(A);
            state A {
                entry: a_in;
                exit:  a_out;
                on(GoB) => B;
                on(GoC) => C;
                on(GoD) => D;
                on(GoE) => E;
            }
            state B {
                entry: b_in;
                exit:  b_out;
                on(GoC) => C;
            }
        }
        state C {
            entry: c_in;
            exit:  c_out;
        }
        state D {
            entry: d_in;
            exit:  d_out;
            default(E);
            state E {
                entry: e_in;
                exit:  e_out;
            }
        }
    }
}

impl LatActions for LatActionContext<'_> {
    async fn p_in(&mut self) {}
    async fn p_out(&mut self) {}
    async fn a_in(&mut self) {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self) {}
    async fn b_out(&mut self) {}
    async fn c_in(&mut self) {}
    async fn c_out(&mut self) {}
    async fn d_in(&mut self) {}
    async fn d_out(&mut self) {}
    async fn e_in(&mut self) {}
    async fn e_out(&mut self) {}
}

// State ids: 0=Root, 1=Parent, 2=A, 3=B, 4=C, 5=D, 6=E.
const SR: u16 = 0;
const SP: u16 = 1;
const SA: u16 = 2;
const SB: u16 = 3;
const SC: u16 = 4;
const SD: u16 = 5;
const SE: u16 = 6;
// Action ids in declaration order: 0=p_in, 1=p_out, 2=a_in, 3=a_out,
// 4=b_in, 5=b_out, 6=c_in, 7=c_out, 8=d_in, 9=d_out, 10=e_in, 11=e_out.
const A_P_IN: u16 = 0;
const A_P_OUT: u16 = 1;
const A_A_IN: u16 = 2;
const A_A_OUT: u16 = 3;
const A_B_IN: u16 = 4;
const A_B_OUT: u16 = 5;
const A_C_IN: u16 = 6;
const A_C_OUT: u16 = 7;
const A_D_IN: u16 = 8;
const A_D_OUT: u16 = 9;
const A_E_IN: u16 = 10;
const A_E_OUT: u16 = 11;
// Event ids in first-seen order: 0=GoB, 1=GoC, 2=GoD, 3=GoE, 4=Halt.
const E_GOB: u16 = 0;
const E_GOC: u16 = 1;
const E_GOD: u16 = 2;
const E_GOE: u16 = 3;

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
#[allow(dead_code)]
fn handler(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked {
        state,
        action,
        kind: ActionKind::Handler,
    }
}

// Initial descent: Root → (Root's default fires) → Parent → (Parent's
// default fires) → A. Each default-fire is a real transition.
fn initial_descent() -> Vec<TraceEvent> {
    vec![
        TraceEvent::Started {
            chart_hash: Lat::<8>::CHART_HASH,
        },
        TraceEvent::EnterBegan { state: SR },
        TraceEvent::Entered { state: SR },
        TraceEvent::TransitionFired {
            from: Some(SR),
            to: SP,
            reason: TransitionReason::Internal,
        },
        TraceEvent::EnterBegan { state: SP },
        entry(SP, A_P_IN),
        TraceEvent::Entered { state: SP },
        TraceEvent::TransitionComplete {
            from: Some(SR),
            to: SP,
        },
        TraceEvent::TransitionFired {
            from: Some(SP),
            to: SA,
            reason: TransitionReason::Internal,
        },
        TraceEvent::EnterBegan { state: SA },
        entry(SA, A_A_IN),
        TraceEvent::Entered { state: SA },
        TraceEvent::TransitionComplete {
            from: Some(SP),
            to: SA,
        },
    ]
}

// Compare actual vs expected and report first mismatch.
fn assert_journal(actual: &[TraceEvent], expected: &[TraceEvent]) {
    let mismatch = actual.iter().zip(expected.iter()).position(|(a, e)| a != e);
    if let Some(i) = mismatch {
        panic!(
            "journal mismatch at index {}\n  actual:   {:?}\n  expected: {:?}\n  full actual: {:#?}",
            i, actual[i], expected[i], actual
        );
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "journal length mismatch: actual {} vs expected {}\n  full actual: {:#?}",
        actual.len(),
        expected.len(),
        actual
    );
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_a_to_b_lca_is_parent() {
    // Spec: A → B (siblings under Parent). LCA = Parent. Parent is not exited
    // or re-entered.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoB).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOB },
                TraceEvent::EventDelivered {
                    handler_state: SA,
                    event: E_GOB,
                },
                TraceEvent::TransitionFired {
                    from: Some(SA),
                    to: SB,
                    reason: TransitionReason::Event { event: E_GOB },
                },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::EnterBegan { state: SB },
                entry(SB, A_B_IN),
                TraceEvent::Entered { state: SB },
                TraceEvent::TransitionComplete {
                    from: Some(SA),
                    to: SB,
                },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_a_to_c_exits_parent() {
    // A → C: A is inside Parent, C is sibling of Parent. LCA = Root.
    // Exit A, exit Parent. Enter C.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoC).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOC },
                TraceEvent::EventDelivered {
                    handler_state: SA,
                    event: E_GOC,
                },
                TraceEvent::TransitionFired {
                    from: Some(SA),
                    to: SC,
                    reason: TransitionReason::Event { event: E_GOC },
                },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::ExitBegan { state: SP },
                exit_(SP, A_P_OUT),
                TraceEvent::Exited { state: SP },
                TraceEvent::EnterBegan { state: SC },
                entry(SC, A_C_IN),
                TraceEvent::Entered { state: SC },
                TraceEvent::TransitionComplete {
                    from: Some(SA),
                    to: SC,
                },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_target_with_children_default_descends() {
    // A → D: D has child E (default). After entering D, default-descend to E.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoD).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOD },
                TraceEvent::EventDelivered {
                    handler_state: SA,
                    event: E_GOD,
                },
                TraceEvent::TransitionFired {
                    from: Some(SA),
                    to: SD,
                    reason: TransitionReason::Event { event: E_GOD },
                },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::ExitBegan { state: SP },
                exit_(SP, A_P_OUT),
                TraceEvent::Exited { state: SP },
                TraceEvent::EnterBegan { state: SD },
                entry(SD, A_D_IN),
                TraceEvent::Entered { state: SD },
                // D's `default(E)` fires as an internal transition.
                TraceEvent::TransitionFired {
                    from: Some(SD),
                    to: SE,
                    reason: TransitionReason::Internal,
                },
                TraceEvent::EnterBegan { state: SE },
                entry(SE, A_E_IN),
                TraceEvent::Entered { state: SE },
                TraceEvent::TransitionComplete {
                    from: Some(SD),
                    to: SE,
                },
                TraceEvent::TransitionComplete {
                    from: Some(SA),
                    to: SD,
                },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_to_deeply_nested_target() {
    // A → E (E is the leaf of D's default chain). The target itself is the leaf
    // so default-descent doesn't add another level — but D is entered along the way.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoE).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOE },
                TraceEvent::EventDelivered {
                    handler_state: SA,
                    event: E_GOE,
                },
                TraceEvent::TransitionFired {
                    from: Some(SA),
                    to: SE,
                    reason: TransitionReason::Event { event: E_GOE },
                },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::ExitBegan { state: SP },
                exit_(SP, A_P_OUT),
                TraceEvent::Exited { state: SP },
                TraceEvent::EnterBegan { state: SD },
                entry(SD, A_D_IN),
                TraceEvent::Entered { state: SD },
                TraceEvent::EnterBegan { state: SE },
                entry(SE, A_E_IN),
                TraceEvent::Entered { state: SE },
                TraceEvent::TransitionComplete {
                    from: Some(SA),
                    to: SE,
                },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_chained_transitions() {
    // A → B → C: two consecutive transitions. Each independently records its
    // exits and entries.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoB).await;
            let _ = m.dispatch(Ev::GoC).await;
            let actual = m.take_journal();

            let mut expected = initial_descent();
            // A → B
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOB },
                TraceEvent::EventDelivered {
                    handler_state: SA,
                    event: E_GOB,
                },
                TraceEvent::TransitionFired {
                    from: Some(SA),
                    to: SB,
                    reason: TransitionReason::Event { event: E_GOB },
                },
                TraceEvent::ExitBegan { state: SA },
                exit_(SA, A_A_OUT),
                TraceEvent::Exited { state: SA },
                TraceEvent::EnterBegan { state: SB },
                entry(SB, A_B_IN),
                TraceEvent::Entered { state: SB },
                TraceEvent::TransitionComplete {
                    from: Some(SA),
                    to: SB,
                },
            ]);
            // B → C (LCA = Root, so Parent exits too).
            expected.extend(vec![
                TraceEvent::EventReceived { event: E_GOC },
                TraceEvent::EventDelivered {
                    handler_state: SB,
                    event: E_GOC,
                },
                TraceEvent::TransitionFired {
                    from: Some(SB),
                    to: SC,
                    reason: TransitionReason::Event { event: E_GOC },
                },
                TraceEvent::ExitBegan { state: SB },
                exit_(SB, A_B_OUT),
                TraceEvent::Exited { state: SB },
                TraceEvent::ExitBegan { state: SP },
                exit_(SP, A_P_OUT),
                TraceEvent::Exited { state: SP },
                TraceEvent::EnterBegan { state: SC },
                entry(SC, A_C_IN),
                TraceEvent::Entered { state: SC },
                TraceEvent::TransitionComplete {
                    from: Some(SB),
                    to: SC,
                },
            ]);
            assert_journal(&actual, &expected);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_lca_stays_active() {
    // A → B: Parent (the LCA) must NOT appear in any Exited or Entered event.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoB).await;
            let j = m.take_journal();
            // After initial Entered(Parent), Parent must not appear again.
            let initial_p_idx = j
                .iter()
                .position(|e| matches!(e, TraceEvent::Entered { state: SP }))
                .unwrap();
            let parent_events: Vec<&TraceEvent> = j
                .iter()
                .skip(initial_p_idx + 1)
                .filter(|e| {
                    matches!(
                        e,
                        TraceEvent::Entered { state: SP } | TraceEvent::Exited { state: SP }
                    )
                })
                .collect();
            assert!(
                parent_events.is_empty(),
                "LCA Parent must not be re-entered or exited; got: {:?}",
                parent_events
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_exit_inner_then_outer() {
    // A → C: exit A first (deeper), then Parent (outer).
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoC).await;
            let exits: Vec<u16> = m
                .journal()
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Exited { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // First exit must be A (deeper), then Parent (outer).
            assert_eq!(&exits[..2], &[SA, SP], "exits must be inner-to-outer");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_entry_outer_then_inner() {
    // A → D where D has child E: enter D first, then E.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoD).await;
            let entries: Vec<u16> = m
                .journal()
                .iter()
                .skip_while(|e| {
                    !matches!(
                        e,
                        TraceEvent::TransitionFired {
                            reason: TransitionReason::Event { .. },
                            ..
                        }
                    )
                })
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            assert_eq!(
                &entries,
                &[SD, SE],
                "entries on the way down must be outer-to-inner"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_no_default_when_target_is_leaf() {
    // A → C where C is a leaf: no default-descent, journal stops at C.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoC).await;
            let last_entered: Vec<u16> = m
                .journal()
                .iter()
                .filter_map(|e| match e {
                    TraceEvent::Entered { state } => Some(*state),
                    _ => None,
                })
                .collect();
            // Initial: Root, Parent, A. After GoC: C. No further entries.
            assert_eq!(last_entered, vec![SR, SP, SA, SC]);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_lateral_event_delivered_records_handler_state() {
    // The handler for GoB lives on A. EventDelivered.handler_state = A.
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut m = Lat::new(Ctx);
            let _ = m.dispatch(Ev::GoB).await;
            let delivered = m
                .journal()
                .iter()
                .find(|e| matches!(e, TraceEvent::EventDelivered { event: E_GOB, .. }));
            assert!(
                matches!(
                    delivered,
                    Some(TraceEvent::EventDelivered {
                        handler_state: SA,
                        event: E_GOB
                    })
                ),
                "got: {:?}",
                delivered
            );
        })
        .await;
}
