//! `during:` activity: a single async function scoped to a state, repeatedly
//! producing events while the state is active. This test verifies that the
//! during fires, the generated event dispatches through the handler chain,
//! and a transition out of the state cancels the during.

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx {
    pub tick_field: u32,
    pub seen_tick: u32,
    pub log: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Tick,
    Halt,
}

statechart! {
DuringM {
    context: Ctx;
    events: Ev;
    default(Running);
    terminate(Halt);

    state Running {
        during: ticker(tick_field);
        on(Tick) => saw_tick;
    }
}
}

async fn ticker(tick_field: &mut u32) -> Ev {
    tokio::time::sleep(Duration::from_millis(10)).await;
    *tick_field = tick_field.wrapping_add(1);
    Ev::Tick
}

impl DuringMActions for DuringMActionContext<'_> {
    async fn saw_tick(&mut self) {
        self.seen_tick = self.seen_tick.wrapping_add(1);
        let n = self.seen_tick;
        self.log.push(format!("tick#{}", n));
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn during_fires_repeatedly_until_halt() {
    let mut m = DuringM::new(Ctx::default());
    let sender = m.sender();
    // Let a few ticks happen, then halt.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(45)).await;
        let _ = sender.send(Ev::Halt);
    });
    let res = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    let res = res.expect("run() hung past timeout");
    assert!(res.is_ok(), "run() returned error: {:?}", res);
    assert!(m.is_terminated());
    let ctx = m.into_context();
    // We slept for 45ms and the during fires every 10ms — expect ≥3 ticks
    // before Halt is dispatched. (The first dispatch happens at t≈10ms.)
    assert!(
        ctx.seen_tick >= 3,
        "expected ≥3 ticks, got {} (log={:?})",
        ctx.seen_tick,
        ctx.log
    );
    // Field mutation is observed.
    assert_eq!(ctx.tick_field, ctx.seen_tick);
}
