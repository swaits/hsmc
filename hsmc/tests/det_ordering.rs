#![allow(dead_code)]
//! Deterministic-flow tests for ENTRY/EXIT ORDERING (declaration order, depth order).
//!
//! Spec section "Entry and exit ordering":
//!   Entry runs outer-to-inner. Within a single state, entry actions fire in
//!   declaration order. Exit runs inner-to-outer. Within a single state, exit
//!   actions fire in declaration order.
//!
//! Spec: "If a single state has multiple handlers (actions and/or a transition)
//! for the same trigger, the actions fire in declaration order, then the
//! transition fires last."

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Multi,
    Halt,
}

statechart! {
    Ord {
        context: Ctx;
        events:  Ev;
        default(A);
        terminate(Halt);

        entry: r1, r2, r3;
        exit:  r_x1, r_x2;

        state A {
            entry: a1, a2;
            exit:  a_x1, a_x2, a_x3;
            on(Go) => B;
            on(Multi) => act1;
            on(Multi) => act2;
            on(Multi) => act3;
            on(Multi) => B;
        }
        state B {
            entry: b1;
            exit:  b_x1;
        }
    }
}

impl OrdActions for OrdActionContext<'_> {
    async fn r1(&mut self)   {}
    async fn r2(&mut self)   {}
    async fn r3(&mut self)   {}
    async fn r_x1(&mut self) {}
    async fn r_x2(&mut self) {}
    async fn a1(&mut self)   {}
    async fn a2(&mut self)   {}
    async fn a_x1(&mut self) {}
    async fn a_x2(&mut self) {}
    async fn a_x3(&mut self) {}
    async fn b1(&mut self)   {}
    async fn b_x1(&mut self) {}
    async fn act1(&mut self) {}
    async fn act2(&mut self) {}
    async fn act3(&mut self) {}
}

const SR: u16 = 0;
const SA: u16 = 1;
const SB: u16 = 2;

// Action interning order from declaration:
//   Root: r1=0, r2=1, r3=2 (entries), r_x1=3, r_x2=4 (exits)
//   A: a1=5, a2=6, a_x1=7, a_x2=8, a_x3=9, then handlers act1=10, act2=11, act3=12
//   B: b1=13, b_x1=14
const A_R1: u16 = 0;
const A_R2: u16 = 1;
const A_R3: u16 = 2;
const A_RX1: u16 = 3;
const A_RX2: u16 = 4;
const A_A1: u16 = 5;
const A_A2: u16 = 6;
const A_AX1: u16 = 7;
const A_AX2: u16 = 8;
const A_AX3: u16 = 9;
const A_ACT1: u16 = 10;
const A_ACT2: u16 = 11;
const A_ACT3: u16 = 12;
const A_B1: u16 = 13;
const A_BX1: u16 = 14;

#[tokio::test(flavor = "current_thread")]
async fn det_ord_root_entries_in_declaration_order() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        // Filter to root entry actions in journal order.
        let r_entries: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state: SR, action, kind: ActionKind::Entry } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(r_entries, vec![A_R1, A_R2, A_R3]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_root_exits_in_declaration_order() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let r_exits: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state: SR, action, kind: ActionKind::Exit } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(r_exits, vec![A_RX1, A_RX2]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_state_a_entries_in_declaration_order() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let a_entries: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state: SA, action, kind: ActionKind::Entry } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(a_entries, vec![A_A1, A_A2]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_state_a_exits_in_declaration_order() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let a_exits: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { state: SA, action, kind: ActionKind::Exit } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(a_exits, vec![A_AX1, A_AX2, A_AX3]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_entry_outer_then_inner_across_hierarchy() {
    // Initial entry: Root → A. Root's entries fire first, then A's.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let entries_in_order: Vec<u16> = m.journal().iter().take_while(|e| !matches!(
            e, TraceEvent::TerminateRequested { .. }
        )).filter_map(|e| match e {
            TraceEvent::ActionInvoked { action, kind: ActionKind::Entry, .. } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(entries_in_order, vec![A_R1, A_R2, A_R3, A_A1, A_A2]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_exit_inner_then_outer_across_hierarchy() {
    // On terminate: A's exits then Root's exits.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Halt).await;
        let exits_in_order: Vec<u16> = m.journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { action, kind: ActionKind::Exit, .. } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(exits_in_order, vec![A_AX1, A_AX2, A_AX3, A_RX1, A_RX2]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_actions_then_transition_on_same_trigger() {
    // Multi: act1, act2, act3 fire in declaration order, then transition to B.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Multi).await;
        let j = m.take_journal();

        // Find the action invocations and the TransitionFired in order.
        let after_delivered: Vec<&TraceEvent> = j.iter()
            .skip_while(|e| !matches!(e, TraceEvent::EventDelivered { event: 1, .. }))
            // Multi is the second event: Go=0, Multi=1.
            .skip(1)
            .take(20)
            .collect();
        // First three should be Handler actions in order act1, act2, act3.
        let handler_actions: Vec<u16> = after_delivered.iter().take_while(|e| matches!(
            e, TraceEvent::ActionInvoked { kind: ActionKind::Handler, .. }
        )).filter_map(|e| match e {
            TraceEvent::ActionInvoked { action, .. } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(handler_actions, vec![A_ACT1, A_ACT2, A_ACT3],
            "actions in declaration order");
        // After the actions, a TransitionFired must follow.
        let trans_idx = after_delivered.iter().position(|e| matches!(
            e, TraceEvent::TransitionFired { .. }
        )).unwrap();
        assert_eq!(trans_idx, 3, "transition fires after the 3 actions");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_ord_full_transition_canonical_sequence() {
    // Spec: cancel durings → run exits inner→outer → run entries outer→inner → start durings.
    // No durings here, so just exits inner→outer + entries outer→inner.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Ord::new(Ctx);
        let _ = m.dispatch(Ev::Go).await;
        let j = m.take_journal();
        // Find the TransitionFired and capture everything until A's exit and B's entry are done.
        let trans_idx = j.iter().position(|e| matches!(e, TraceEvent::TransitionFired { .. })).unwrap();
        let kinds: Vec<ActionKind> = j[trans_idx..].iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { kind, .. } => Some(*kind),
            _ => None,
        }).collect();
        // First A's exit kinds (inner-to-outer order — A is innermost), then B's entry kinds.
        let exit_count = kinds.iter().filter(|k| **k == ActionKind::Exit).count();
        let entry_count = kinds.iter().filter(|k| **k == ActionKind::Entry).count();
        assert_eq!(exit_count, 3, "A has 3 exit actions");
        assert_eq!(entry_count, 1, "B has 1 entry action");
        // Exits before entries.
        let first_entry = kinds.iter().position(|k| *k == ActionKind::Entry).unwrap();
        let last_exit = kinds.iter().rposition(|k| *k == ActionKind::Exit).unwrap();
        assert!(last_exit < first_entry, "all exits must precede all entries in a transition");
    }).await;
}
