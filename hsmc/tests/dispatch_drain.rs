//! `m.dispatch(ev).await` pushes an event AND drains the internal queue to
//! quiescence before returning. After it returns, `current_state()` reflects
//! the post-dispatch state — the replacement for the old `send + drain`
//! helper that firmware users hand-rolled.

#![cfg(feature = "tokio")]

use hsmc::statechart;

#[derive(Default)]
pub struct Ctx {
    pub transitioned: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Halt,
}

statechart! {
DispatchM {
    context: Ctx;
    events: Ev;
    default(Start);
    terminate(Halt);

    state Start {
        on(Go) => Next;
    }
    state Next {
        entry: mark_entered;
    }
}
}

impl DispatchMActions for DispatchMActionContext<'_> {
    async fn mark_entered(&mut self) {
        self.transitioned = self.transitioned.wrapping_add(1);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dispatch_drains_before_returning() {
    let mut m = DispatchM::new(Ctx::default());
    // One call does: prime state, enqueue Go, process it, reach Next,
    // run mark_entered, all before returning.
    m.dispatch(Ev::Go).await.unwrap();
    assert_eq!(m.current_state(), DispatchMState::Next);
    assert_eq!(m.context().transitioned, 1);
}
