//! Timer-only statechart: no `events:` declaration. The machine is driven
//! purely by timer triggers. No `Sender` is generated; `run()` races
//! timers and any `during:` activities.

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};

#[derive(Default)]
pub struct BlinkCtx {
    pub on_count: u32,
    pub off_count: u32,
}

statechart! {
Blinker {
    context: BlinkCtx;
    default(On);

    state On {
        entry: bump_on;
        on(after Duration::from_millis(10)) => Off;
    }
    state Off {
        entry: bump_off;
        on(after Duration::from_millis(10)) => On;
    }
}
}

impl BlinkerActions for BlinkerActionContext<'_> {
    async fn bump_on(&mut self) {
        self.on_count = self.on_count.wrapping_add(1);
    }
    async fn bump_off(&mut self) {
        self.off_count = self.off_count.wrapping_add(1);
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn timer_only_blinker_alternates() {
    let mut m = Blinker::new(BlinkCtx::default());
    // Cap run time — the blinker has no terminate event, so we stop it
    // externally via timeout.
    let _ = tokio::time::timeout(Duration::from_millis(100), m.run()).await;
    let ctx = m.into_context();
    // Entered On once on default descent, then Off, then On, ... Over 100ms
    // with a 10ms period, we should see roughly 5 entries of each.
    assert!(
        ctx.on_count >= 3,
        "expected ≥3 on_count, got {}",
        ctx.on_count
    );
    assert!(
        ctx.off_count >= 3,
        "expected ≥3 off_count, got {}",
        ctx.off_count
    );
}
