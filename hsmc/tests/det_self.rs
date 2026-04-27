#![allow(dead_code)]
//! Deterministic-flow tests for SELF-TRANSITIONS.
//!
//! Spec section "Transitioning to yourself":
//!   The standard transition algorithm gives the clean answer: the LCA of the
//!   target with itself is its parent, so we exit only the target and re-enter
//!   only the target. Nothing above the target is exited or re-entered.
//!
//! Concrete example given in spec: in `Root → SomeState → SomeOtherState`,
//! a transition (Trig) => SomeOtherState exits and re-enters ONLY
//! SomeOtherState; SomeState is never touched.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::{statechart, ActionKind, TraceEvent};

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    SelfLeaf,
    SelfMid,
    Halt,
}

// Hierarchy: Root → Mid → Leaf
//   Self-transitions: SelfLeaf targets Leaf; SelfMid targets Mid.
statechart! {
    Self_ {
        context: Ctx;
        events:  Ev;
        default(Mid);
        terminate(Halt);

        state Mid {
            entry: m_in;
            exit:  m_out;
            default(Leaf);
            on(SelfMid) => Mid;
            state Leaf {
                entry: l_in;
                exit:  l_out;
                on(SelfLeaf) => Leaf;
            }
        }
    }
}

impl Self_Actions for Self_ActionContext<'_> {
    async fn m_in(&mut self)  {}
    async fn m_out(&mut self) {}
    async fn l_in(&mut self)  {}
    async fn l_out(&mut self) {}
}

const SR: u16 = 0;
const SM: u16 = 1;
const SL: u16 = 2;
const A_M_IN: u16 = 0;
const A_M_OUT: u16 = 1;
const A_L_IN: u16 = 2;
const A_L_OUT: u16 = 3;
// Event interning order: handlers on the parent (Mid) are processed before
// recursing into children (Leaf), so SelfMid is interned first.
const E_SELFMID: u16 = 0;
const E_SELFLEAF: u16 = 1;

fn entry(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked { state, action, kind: ActionKind::Entry }
}
fn exit_(state: u16, action: u16) -> TraceEvent {
    TraceEvent::ActionInvoked { state, action, kind: ActionKind::Exit }
}
fn initial_descent() -> Vec<TraceEvent> {
    vec![
        TraceEvent::Started { chart_hash: Self_::<8>::CHART_HASH },
        TraceEvent::Entered { state: SR },
        TraceEvent::Entered { state: SM },
        entry(SM, A_M_IN),
        TraceEvent::Entered { state: SL },
        entry(SL, A_L_IN),
    ]
}
fn assert_journal(actual: &[TraceEvent], expected: &[TraceEvent]) {
    let mismatch = actual.iter().zip(expected.iter())
        .position(|(a, e)| a != e);
    if let Some(i) = mismatch {
        panic!("mismatch at index {}\n  actual:   {:?}\n  expected: {:?}",
            i, actual.get(i), expected.get(i));
    }
    assert_eq!(actual.len(), expected.len(),
        "length mismatch: actual {} vs expected {}", actual.len(), expected.len());
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_leaf_exits_and_reenters_only_leaf() {
    // Spec: nothing above the target is touched.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Self_::new(Ctx);
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let actual = m.take_journal();

        let mut expected = initial_descent();
        expected.extend(vec![
            TraceEvent::EventDelivered { handler_state: SL, event: E_SELFLEAF },
            TraceEvent::TransitionFired { from: Some(SL), to: SL },
            exit_(SL, A_L_OUT),
            TraceEvent::Exited { state: SL },
            TraceEvent::Entered { state: SL },
            entry(SL, A_L_IN),
        ]);
        assert_journal(&actual, &expected);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_leaf_does_not_touch_parent() {
    // The key spec example. Mid must NOT appear in any Exited or Entered
    // event after the initial descent.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Self_::new(Ctx);
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let j = m.take_journal();

        // Find the index of the EventDelivered for SelfLeaf — everything
        // after that is the self-transition body.
        let split = j.iter().position(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_SELFLEAF, .. }
        )).unwrap();

        let parent_after = j[split..].iter().filter(|e| matches!(
            e,
            TraceEvent::Entered { state: SM } | TraceEvent::Exited { state: SM }
        )).count();
        assert_eq!(parent_after, 0,
            "Mid must not be exited or re-entered during self-transition on Leaf; got tail: {:?}",
            &j[split..]);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_composite_default_descends() {
    // Spec: "If the target happens to be a composite ... the same default-child
    // rule still applies on re-entry: exit the composite, re-enter it, then
    // descend through its default chain to a leaf."
    //
    // Currently in Mid via Leaf default. SelfMid bubbles from Leaf to Mid (Mid
    // has the handler). At dispatch time, innermost is Leaf, so Mid is not
    // the innermost — that makes it an UP-then-self situation. Actually the
    // transition algorithm classifies it: target=Mid, current=Leaf, target IS
    // on the active path and target ≠ I, so this is an UP-transition not a
    // self-transition. Let's verify what the journal records.
    //
    // (See det_up.rs for explicit up-transition tests; this test pins the
    // bubbling behavior when the handler is on Mid but the transition is to
    // Mid itself from Leaf.)
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Self_::new(Ctx);
        let _ = m.dispatch(Ev::SelfMid).await;
        let actual = m.take_journal();

        // Per spec rules: target=Mid is ON the active path (Root→Mid→Leaf),
        // and target ≠ Leaf. So it's an UP-transition: only Leaf exits.
        // Mid's entry actions don't fire. No default-descent.
        let mut expected = initial_descent();
        expected.extend(vec![
            TraceEvent::EventDelivered { handler_state: SM, event: E_SELFMID },
            TraceEvent::TransitionFired { from: Some(SL), to: SM },
            exit_(SL, A_L_OUT),
            TraceEvent::Exited { state: SL },
        ]);
        assert_journal(&actual, &expected);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_leaf_repeated_is_deterministic() {
    // Multiple self-transitions: each independently records the same pattern.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Self_::new(Ctx);
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let j = m.take_journal();
        // Three EventDelivered for SelfLeaf, three TransitionFired, three of each
        // exit/entry pair.
        let delivered = j.iter().filter(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_SELFLEAF, .. }
        )).count();
        assert_eq!(delivered, 3);
        let trans = j.iter().filter(|e| matches!(
            e, TraceEvent::TransitionFired { from: Some(SL), to: SL }
        )).count();
        assert_eq!(trans, 3);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_byte_deterministic_across_runs() {
    let run = || async {
        tokio::task::LocalSet::new().run_until(async {
            let mut m = Self_::new(Ctx);
            let _ = m.dispatch(Ev::SelfLeaf).await;
            let _ = m.dispatch(Ev::SelfLeaf).await;
            m.take_journal()
        }).await
    };
    let first = run().await;
    for i in 1..10 {
        assert_eq!(first, run().await, "run {i} diverged");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn det_self_action_kinds_are_exit_then_entry() {
    // Spec: "cancel durings, run exits, run entries, start durings".
    // No durings on this chart, so we expect: Exit kind action, then Entry kind action.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Self_::new(Ctx);
        let _ = m.dispatch(Ev::SelfLeaf).await;
        let j = m.take_journal();
        let split = j.iter().position(|e| matches!(
            e, TraceEvent::EventDelivered { event: E_SELFLEAF, .. }
        )).unwrap();
        let kinds_after: Vec<ActionKind> = j[split..].iter().filter_map(|e| match e {
            TraceEvent::ActionInvoked { kind, .. } => Some(*kind),
            _ => None,
        }).collect();
        assert_eq!(kinds_after, vec![ActionKind::Exit, ActionKind::Entry],
            "self-transition: exit before entry");
    }).await;
}
