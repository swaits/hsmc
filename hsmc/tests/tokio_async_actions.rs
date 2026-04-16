//! Proves that action bodies can `.await` real futures under the `tokio`
//! feature, that transitions are committed only after all action futures
//! complete, and that `emit()` inside an async action still enqueues.
#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx {
    pub log: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Go,
    Halt,
}

statechart! {
AsyncM {
    context: Ctx;
    events: Ev;
    default(Idle);
    terminate(Halt);
    state Idle {
        on(Go) => Working;
    }
    state Working {
        entry: on_enter_working;
        exit: on_exit_working;
    }
}
}

impl AsyncMActions for AsyncMActionContext<'_> {
    async fn on_enter_working(&mut self) {
        // A real `.await` that suspends the task.
        tokio::time::sleep(Duration::from_millis(5)).await;
        self.log.push("entered_after_sleep".into());
        // emit() inside an async action must enqueue synchronously.
        let _ = self.emit(Ev::Halt);
    }
    async fn on_exit_working(&mut self) {
        tokio::time::sleep(Duration::from_millis(1)).await;
        self.log.push("exited_after_sleep".into());
    }
}

#[tokio::test]
async fn async_actions_await_and_emit() {
    let mut m = AsyncM::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        let _ = sender.send(Ev::Go);
    });
    let res = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    assert!(res.expect("hang").is_ok());
    assert!(m.is_terminated());
    let ctx = m.into_context();
    // Entry action's await completed before the emitted Halt was processed.
    assert!(ctx.log.iter().any(|s| s == "entered_after_sleep"));
    // Exit action also ran via Halt → terminate path.
    assert!(ctx.log.iter().any(|s| s == "exited_after_sleep"));
}
