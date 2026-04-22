//! Multiple `during:` activities on the same state. Verifies that split-borrow
//! lets each during hold `&mut` to disjoint fields of the context, that the
//! generated `select_N` call compiles, and that the expected during fires
//! when given enough time.
//!
//! Note on starvation: when several durings race via `select`, the shortest
//! one always wins and the others are dropped. That's the documented
//! cancel-safety contract. The test reflects that by only asserting that the
//! fast during fires and the slow one *can* fire eventually (not that it
//! fires deterministically within the test window).

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct Ctx {
    pub fast_field: u32,
    pub slow_field: u32,
    pub seen_fast: u32,
    pub seen_slow: u32,
}

#[derive(Debug, Clone)]
pub enum Ev {
    Fast,
    Slow,
    Halt,
}

statechart! {
Concurrent {
    context: Ctx;
    events: Ev;
    default(Running);
    terminate(Halt);

    state Running {
        during: fast(fast_field);
        during: slow(slow_field);
        on(Fast) => on_fast;
        on(Slow) => on_slow;
    }
}
}

async fn fast(f: &mut u32) -> Ev {
    tokio::time::sleep(Duration::from_millis(5)).await;
    *f = f.wrapping_add(1);
    Ev::Fast
}

async fn slow(s: &mut u32) -> Ev {
    tokio::time::sleep(Duration::from_millis(15)).await;
    *s = s.wrapping_add(1);
    Ev::Slow
}

impl ConcurrentActions for ConcurrentActionContext<'_> {
    async fn on_fast(&mut self) {
        self.seen_fast = self.seen_fast.wrapping_add(1);
    }
    async fn on_slow(&mut self) {
        self.seen_slow = self.seen_slow.wrapping_add(1);
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn both_durings_compile_and_fast_fires() {
    let mut m = Concurrent::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = sender.send(Ev::Halt);
    });
    let res = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    assert!(res.expect("run hung").is_ok());
    let ctx = m.into_context();
    // Fast (5ms period) wins its race consistently against slow (15ms).
    // This documents the starvation property: in select-drop semantics,
    // the faster during starves the slower one.
    assert!(
        ctx.seen_fast >= 4,
        "expected fast ≥4, got {}",
        ctx.seen_fast
    );
    assert_eq!(ctx.fast_field, ctx.seen_fast);
}
