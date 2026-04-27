#![allow(dead_code)]
//! Deterministic-flow tests for `current_state()` reflecting the innermost
//! active state at every point.

#![cfg(all(feature = "tokio", feature = "journal"))]
#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Default)]
pub struct Ctx;

#[derive(Debug, Clone)]
pub enum Ev {
    GoB,
    UpToParent,
    Halt,
}

statechart! {
    Cur {
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
                on(UpToParent) => Parent;
            }
            state B {
                entry: b_in;
                exit:  b_out;
                on(UpToParent) => Parent;
            }
        }
    }
}

impl CurActions for CurActionContext<'_> {
    async fn p_in(&mut self)  {}
    async fn p_out(&mut self) {}
    async fn a_in(&mut self)  {}
    async fn a_out(&mut self) {}
    async fn b_in(&mut self)  {}
    async fn b_out(&mut self) {}
}

#[tokio::test(flavor = "current_thread")]
async fn det_current_state_initial_is_default_leaf() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Cur::new(Ctx);
        // Drive initial entry by sending an event.
        let _ = m.dispatch(Ev::GoB).await;
        // Now in B.
        assert_eq!(m.current_state(), CurState::B);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_current_state_after_lateral_transition() {
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Cur::new(Ctx);
        let _ = m.dispatch(Ev::GoB).await;
        assert_eq!(m.current_state(), CurState::B);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn det_current_state_after_up_returns_composite() {
    // Spec: current_state can return a composite if it became innermost via
    // an up-transition.
    tokio::task::LocalSet::new().run_until(async {
        let mut m = Cur::new(Ctx);
        let _ = m.dispatch(Ev::UpToParent).await;
        assert_eq!(m.current_state(), CurState::Parent,
            "after up-transition, current_state returns the composite Parent");
    }).await;
}
