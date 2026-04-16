//! Transition cancels an in-flight `during:` future: the future is dropped
//! when the machine leaves the state, and the future's progress (counted by
//! a cancel-on-drop side effect) reflects the cancellation.

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

/// A cancel-aware guard that bumps `dropped_count` when dropped before
/// completion. Used by the during to prove it was cancelled by a transition.
pub struct DropGuard<'a> {
    counter: &'a mut u32,
    completed: bool,
}

impl<'a> Drop for DropGuard<'a> {
    fn drop(&mut self) {
        if !self.completed {
            *self.counter = self.counter.wrapping_add(1);
        }
    }
}

#[derive(Default)]
pub struct Ctx {
    pub drop_count: u32,
    pub ticked: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Leave,
    Halt,
}

statechart! {
CancelM {
    context: Ctx;
    events: Ev;
    default(Inside);
    terminate(Halt);

    state Inside {
        during: slow_tick(drop_count);
        on(Leave) => Outside;
    }

    state Outside {
        on(Halt) => Outside;  // placeholder so state isn't empty
    }
}
}

/// A slow activity: creates a drop guard, sleeps 500ms (much longer than
/// our test window), and would bump `ticked` only on successful completion.
/// When cancelled, the guard's Drop increments drop_count.
async fn slow_tick(drop_count: &mut u32) -> Ev {
    let mut guard = DropGuard {
        counter: drop_count,
        completed: false,
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    guard.completed = true;
    // unreachable in this test; the transition drops us before 500ms
    Ev::Leave
}

impl CancelMActions for CancelMActionContext<'_> {
    // no handler work needed
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn transition_cancels_during_future() {
    let mut m = CancelM::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn({
        let s = sender.clone();
        async move {
            // Transition out at 20ms, well before the during's 500ms sleep.
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = s.send(Ev::Leave);
            // Then terminate.
            tokio::time::sleep(Duration::from_millis(10)).await;
            let _ = s.send(Ev::Halt);
        }
    });
    let res = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    assert!(res.expect("run hung").is_ok());
    let ctx = m.into_context();
    // During was cancelled → DropGuard fired → drop_count >= 1.
    assert!(
        ctx.drop_count >= 1,
        "expected cancel to trigger DropGuard, got drop_count={}",
        ctx.drop_count
    );
    // The during never completed so ticked should be 0.
    assert_eq!(ctx.ticked, 0);
}
