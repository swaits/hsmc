//! Integration test for async `run()` under the `tokio` feature (T6.3).
#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx { pub log: Vec<String> }

#[derive(Debug, Clone)]
pub enum Ev { Halt }

statechart! {
TokM {
    context: Ctx;
    events: Ev;
    default(A);
    terminate(Halt);
    state A { entry: a_entry; exit: a_exit; }
}
}

impl TokMActions for TokMActionContext<'_> {
    async fn a_entry(&mut self) {
        tokio::task::yield_now().await;
        self.log.push("a_entry".into());
    }
    async fn a_exit(&mut self) {
        tokio::task::yield_now().await;
        self.log.push("a_exit".into());
    }
}

#[tokio::test]
async fn t6_3_run_returns_after_terminate() {
    let mut m = TokM::new(Ctx::default());
    let sender = m.sender();
    // Trigger terminate from another task.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = sender.send(Ev::Halt);
    });
    let res = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    let res = res.expect("run() hung past timeout");
    assert!(res.is_ok());
    assert!(m.is_terminated());
    let ctx = m.into_context();
    assert!(ctx.log.contains(&"a_entry".to_string()));
    assert!(ctx.log.contains(&"a_exit".to_string()));
}
