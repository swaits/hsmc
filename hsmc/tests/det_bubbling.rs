#![allow(dead_code)]
//! Deterministic-flow tests for EVENT BUBBLING.
//!
//! Spec section "How events are handled":
//!   Event comes in. Start at the innermost active state. Does it have a
//!   handler for this event? If yes, that handler runs and the event is
//!   consumed — no further handling. If no, walk up to the parent. Repeat.
//!   If you walk all the way to the root and still nothing handles it, the
//!   event is silently discarded.
//!
//!   - A handler on a leaf state shadows a handler on its ancestor.
//!   - Multiple handlers in the same state for the same trigger:
//!     actions in declaration order, then the transition.
//!   - Timer triggers do NOT bubble.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    LeafEv,    // handled at leaf
    MidEv,     // handled at Mid only
    RootEv,    // handled at root only
    Unknown,   // no handler anywhere
    Shadowed,  // handler at both Leaf and Mid; leaf wins
    MultiAct,  // multiple actions in same state for same trigger
    Halt,
}

statechart! {
    Bub {
        context: Ctx;
        events:  Ev;
        default(Mid);
        terminate(Halt);

        on(RootEv) => root_handler;

        state Mid {
            entry: m_in;
            exit:  m_out;
            default(Leaf);
            on(MidEv) => mid_handler;
            on(Shadowed) => mid_shadowed;
            state Leaf {
                entry: l_in;
                exit:  l_out;
                on(LeafEv) => leaf_handler;
                on(Shadowed) => leaf_shadowed;
                on(MultiAct) => multi1;
                on(MultiAct) => multi2;
                on(MultiAct) => multi3;
            }
        }
    }
}

impl BubActions for BubActionContext<'_> {
    async fn m_in(&mut self)          {}
    async fn m_out(&mut self)         {}
    async fn l_in(&mut self)          {}
    async fn l_out(&mut self)         {}
    async fn root_handler(&mut self)  {}
    async fn mid_handler(&mut self)   {}
    async fn leaf_handler(&mut self)  {}
    async fn mid_shadowed(&mut self)  {}
    async fn leaf_shadowed(&mut self) {}
    async fn multi1(&mut self)        {}
    async fn multi2(&mut self)        {}
    async fn multi3(&mut self)        {}
}

const SR: u16 = 0;
const SM: u16 = 1;
const SL: u16 = 2;
// Action interning order:
//   Root body: handlers (root_handler → 0)
//   Mid body: m_in → 1, m_out → 2; handlers (mid_handler → 3, mid_shadowed → 4)
//   Leaf body: l_in → 5, l_out → 6; handlers (leaf_handler → 7, leaf_shadowed → 8, multi1..3 → 9..11)
const A_ROOT_HANDLER: u16 = 0;
const A_MID_HANDLER: u16 = 3;
const A_LEAF_HANDLER: u16 = 7;
const A_MID_SHADOWED: u16 = 4;
const A_LEAF_SHADOWED: u16 = 8;
const A_MULTI1: u16 = 9;
const A_MULTI2: u16 = 10;
const A_MULTI3: u16 = 11;
// Event interning order:
//   Root processes first: handler RootEv → 0
//   Then Mid: MidEv → 1, Shadowed → 2
//   Then Leaf: LeafEv → 3, Shadowed already interned, MultiAct → 4
const E_ROOTEV: u16 = 0;
const E_MIDEV: u16 = 1;
const E_SHADOWED: u16 = 2;
const E_LEAFEV: u16 = 3;
const E_MULTIACT: u16 = 4;

fn handler_act(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked { state, action, kind: ActionKind::Handler }
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_handler_at_leaf() {
    // Leaf has handler for LeafEv. Bubbling stops at Leaf.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::LeafEv).await;
        let j = m.take_journal();
        // Find the EventDelivered for LeafEv.
        let delivered = j.iter().find(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_LEAFEV, .. }
        ));
        assert!(matches!(
            delivered,
            Some(TraceEvent::EventDelivered { handler_state: SL, event: E_LEAFEV })
        ), "LeafEv must be delivered to Leaf, got: {:?}", delivered);
        // The handler action must fire.
        let saw_handler = j.iter().any(|e| matches!(
            e,
            TraceEvent::ActionInvoked { state: SL, action: A_LEAF_HANDLER, kind: ActionKind::Handler }
        ));
        assert!(saw_handler, "leaf_handler must run");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_event_bubbles_to_parent() {
    // Leaf has no handler for MidEv. Bubble to Mid.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::MidEv).await;
        let j = m.take_journal();
        let delivered = j.iter().find(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_MIDEV, .. }
        ));
        assert!(matches!(
            delivered,
            Some(TraceEvent::EventDelivered { handler_state: SM, event: E_MIDEV })
        ), "MidEv must be delivered to Mid, got: {:?}", delivered);
        assert_eq!(j.iter().filter(|e| matches!(
            e, TraceEvent::ActionInvoked { action: A_MID_HANDLER, .. }
        )).count(), 1);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_event_bubbles_to_root() {
    // Neither Leaf nor Mid handles RootEv. Bubble to Root.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::RootEv).await;
        let j = m.take_journal();
        let delivered = j.iter().find(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_ROOTEV, .. }
        ));
        assert!(matches!(
            delivered,
            Some(TraceEvent::EventDelivered { handler_state: SR, event: E_ROOTEV })
        ), "RootEv must be delivered to Root, got: {:?}", delivered);
        assert_eq!(j.iter().filter(|e| matches!(
            e, TraceEvent::ActionInvoked { action: A_ROOT_HANDLER, .. }
        )).count(), 1);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_unknown_event_dropped() {
    // No state handles Unknown. Spec: "the event is silently discarded."
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::Unknown).await;
        let j = m.take_journal();
        // Must contain an EventDropped for Unknown.
        let dropped = j.iter().any(|e| matches!(e, TraceEvent::EventDropped { .. }));
        assert!(dropped, "unhandled Unknown must be journaled as EventDropped");
        // No EventDelivered for it.
        let delivered_count = j.iter().filter(|e| matches!(
            e, TraceEvent::EventDelivered { .. }
        )).count();
        // No EventDelivered should fire at all (we only sent Unknown).
        assert_eq!(delivered_count, 0,
            "Unknown must not be delivered; got: {:?}", j);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_leaf_shadows_parent() {
    // Both Leaf and Mid have handlers for Shadowed. Leaf wins (search starts there).
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::Shadowed).await;
        let j = m.take_journal();
        let leaf_fired = j.iter().any(|e| matches!(
            e, TraceEvent::ActionInvoked { action: A_LEAF_SHADOWED, .. }
        ));
        let mid_fired = j.iter().any(|e| matches!(
            e, TraceEvent::ActionInvoked { action: A_MID_SHADOWED, .. }
        ));
        assert!(leaf_fired, "leaf_shadowed must fire (innermost wins)");
        assert!(!mid_fired, "mid_shadowed must NOT fire (shadowed by leaf)");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_multiple_handlers_declaration_order() {
    // Leaf has three actions for MultiAct. They must fire in declaration order.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::MultiAct).await;
        let order: Vec<u16> = m.take_journal().iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { action, kind: ActionKind::Handler, .. } => Some(*action),
            _ => None,
        }).collect();
        assert_eq!(order, vec![A_MULTI1, A_MULTI2, A_MULTI3],
            "multi-action handlers must fire in declaration order");
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_handler_state_recorded_correctly() {
    // EventDelivered.handler_state must reflect WHERE the handler matched, not
    // where the event was sent.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::MidEv).await;
        let _ = m.dispatch(Ev::RootEv).await;
        let j = m.take_journal();
        let mid_delivered = j.iter().find(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_MIDEV, .. }
        ));
        let root_delivered = j.iter().find(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_ROOTEV, .. }
        ));
        assert!(matches!(mid_delivered, Some(TraceEvent::EventDelivered { handler_state: SM, .. })));
        assert!(matches!(root_delivered, Some(TraceEvent::EventDelivered { handler_state: SR, .. })));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_multiple_unknown_events_each_dropped() {
    // Three unknowns — three EventDropped events.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Bub::new(Ctx);
        let _ = m.dispatch(Ev::Unknown).await;
        let _ = m.dispatch(Ev::Unknown).await;
        let _ = m.dispatch(Ev::Unknown).await;
        let dropped_count = m.journal().iter().filter(|e| matches!(
            e, TraceEvent::EventDropped { .. }
        )).count();
        assert_eq!(dropped_count, 3);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_bub_byte_deterministic_under_mixed_events() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Bub::new(Ctx);
            let _ = m.dispatch(Ev::LeafEv).await;
            let _ = m.dispatch(Ev::MidEv).await;
            let _ = m.dispatch(Ev::Unknown).await;
            let _ = m.dispatch(Ev::Shadowed).await;
            let _ = m.dispatch(Ev::MultiAct).await;
            let _ = m.dispatch(Ev::RootEv).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..15 {
        assert_eq!(first, run().await, "diverged on run {i}");
    }
}
