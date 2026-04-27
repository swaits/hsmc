#![allow(dead_code)]
//! Deterministic-flow tests for INITIAL ENTRY and DEFAULT DESCENT.
//!
//! Pins down the semantics from `docs/002. hsmc-semantics-formal.md`,
//! section "Where the machine 'is'":
//!
//!   At any moment, the machine is in a path of states from root down to one
//!   innermost state. ... The root is always active until the machine
//!   terminates.
//!
//! And from "Default child":
//!
//!   Whenever a state with children is entered, the default child is entered
//!   immediately after that state's entry actions finish — it's effectively
//!   an automatic transition into the child.
//!
//! Each test compares the FULL journal to a hand-built EXPECTED sequence.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Halt,
}

// 4-level hierarchy: Root → A → B → C → D
statechart! {
    Init4 {
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
                    }
                }
            }
        }
    }
}

impl Init4Actions for Init4ActionContext<'_> {
    async fn r_in(&mut self)  {}
    async fn r_out(&mut self) {}
    async fn a_in(&mut self)  {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self)  {}
    async fn b_out(&mut self) {}
    async fn c_in(&mut self)  {}
    async fn c_out(&mut self) {}
    async fn d_in(&mut self)  {}
    async fn d_out(&mut self) {}
}

// State ids: 0=Root, 1=A, 2=B, 3=C, 4=D.
const SR: u16 = 0;
const SA: u16 = 1;
const SB: u16 = 2;
const SC: u16 = 3;
const SD: u16 = 4;
// Action ids in declaration order: 0=r_in, 1=r_out, 2=a_in, 3=a_out,
// 4=b_in, 5=b_out, 6=c_in, 7=c_out, 8=d_in, 9=d_out.
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
// Halt = event 0 (interned via `terminate(Halt)`).
const E_HALT: u16 = 0;

fn entry(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked { state, action, kind: ActionKind::Entry }
}
fn exit_(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked { state, action, kind: ActionKind::Exit }
}

#[tokio::test(flavor = "current_thread")]
async fn det_initial_descent_records_full_path() {
    // Spec: "the root is always active" + default-descent unrolls Root → A → B → C → D.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        // Push Halt to drive the chart through initial entry then termination.
        let _ = m.dispatch(Ev::Halt).await;

        let actual = m.take_journal();
        let expected = vec![
            // Initial enter (driven by dispatch's prime-state branch).
            TraceEvent::Started { chart_hash: Init4::<8>::CHART_HASH },
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
            // Halt.
            TraceEvent::TerminateRequested { event: E_HALT },
            // Bottom-up exit chain.
            exit_(SD, A_D_OUT),
            TraceEvent::Exited { state: SD },
            exit_(SC, A_C_OUT),
            TraceEvent::Exited { state: SC },
            exit_(SB, A_B_OUT),
            TraceEvent::Exited { state: SB },
            exit_(SA, A_A_OUT),
            TraceEvent::Exited { state: SA },
            exit_(SR, A_R_OUT),
            TraceEvent::Exited { state: SR },
            TraceEvent::Terminated,
        ];

        let mismatch = actual.iter().zip(expected.iter())
            .position(|(a, e)| a != e)
            .map(|i| i.to_string())
            .unwrap_or_else(|| "<length>".to_string());
        assert_eq!(actual, expected, "first mismatch at index {mismatch}");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_started_is_first_event() {
    // Spec: every journal begins with Started carrying the chart hash.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let j = m.journal();
        assert!(matches!(j[0], TraceEvent::Started { .. }));
        if let TraceEvent::Started { chart_hash } = j[0] {
            assert_eq!(chart_hash, Init4::<8>::CHART_HASH);
        }
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_chart_hash_nonzero() {
    // The FNV-1a fingerprint must actually have run.
    assert_ne!(Init4::<8>::CHART_HASH, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn det_root_state_id_is_zero() {
    // Spec: "the root state (the machine itself) is just a state". It's
    // allocated first and so gets id 0; tests below depend on this.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        // First Entered event must be state 0.
        let first_entered = m.journal().iter().find(|e| matches!(e, TraceEvent::Entered { .. }));
        assert!(matches!(first_entered, Some(TraceEvent::Entered { state: 0 })));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_entry_actions_fire_outer_to_inner() {
    // Spec: "Entry runs outer-to-inner."
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let entered: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state, kind: ActionKind::Entry, .. } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(entered, vec![SR, SA, SB, SC, SD],
            "entry actions must fire outer-to-inner across hierarchy");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_exit_actions_fire_inner_to_outer() {
    // Spec: "Exit runs inner-to-outer."
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let exited: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state, kind: ActionKind::Exit, .. } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(exited, vec![SD, SC, SB, SA, SR],
            "exit actions must fire inner-to-outer across hierarchy");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_state_ids_are_declaration_order() {
    // Spec: state ids assigned in parse order (0=root, then nested).
    // Verify by checking the order Entered events appear.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let entered_ids: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::Entered { state } => Some(*state),
            _ => None,
        }).collect();
        assert_eq!(entered_ids, vec![SR, SA, SB, SC, SD]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_initial_descent_byte_deterministic_across_runs() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Init4::new(Ctx);
            let _ = m.dispatch(Ev::Halt).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..20 {
        assert_eq!(first, run().await, "divergence on run {i}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_terminated_is_last_event() {
    // Spec: "Always the final event in the journal."
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Init4::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        assert!(matches!(m.journal().last(), Some(TraceEvent::Terminated)));
    }).await;
}
