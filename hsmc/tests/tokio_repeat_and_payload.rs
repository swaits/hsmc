//! Integration test covering the two grammar extensions added together:
//!
//! 1. Repeating timers via `on(every <dur>) => handler;` and the
//!    optional `after <dur>` explicit-one-shot form.
//! 2. Event payload bindings via `on(Variant(a: T, b: U)) => handler;`
//!    and `on(Variant { a: T, b: U }) => handler;`, which destructure
//!    into typed action-handler parameters.

#![cfg(feature = "tokio")]

use hsmc::{statechart, Duration};
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub struct Ctx {
    /// Count of ticks fired from the repeating timer.
    pub ticks: Arc<Mutex<u32>>,
    /// Last RSSI/SNR pair seen via the tuple-variant payload binding.
    pub last_rx: Arc<Mutex<Option<(i16, i16)>>>,
    /// Last (freq, bw) seen via the struct-variant payload binding.
    pub last_cfg: Arc<Mutex<Option<(u32, u32)>>>,
}

#[derive(Debug, Clone)]
pub enum Ev {
    PacketRx(i16, i16),
    ConfigChanged { freq: u32, bw: u32 },
    Halt,
}

statechart! {
    Tk {
        context: Ctx;
        events: Ev;
        default(Running);
        terminate(Halt);
        state Running {
            // Repeating timer (advances the tick counter).
            on(every Duration::from_millis(10)) => on_tick;
            // Explicit one-shot form — `after` is a synonym for bare duration.
            // Included to verify parsing; target state is the same so nothing
            // observable happens.
            on(after Duration::from_secs(60)) => Running;
            // Payload-bearing event actions.
            on(PacketRx(rssi: i16, snr: i16)) => record_rx;
            on(ConfigChanged { freq: u32, bw: u32 }) => record_cfg;
        }
    }
}

impl TkActions for TkActionContext<'_> {
    async fn on_tick(&mut self) {
        *self.ticks.lock().unwrap() += 1;
    }
    async fn record_rx(&mut self, rssi: i16, snr: i16) {
        *self.last_rx.lock().unwrap() = Some((rssi, snr));
    }
    async fn record_cfg(&mut self, freq: u32, bw: u32) {
        *self.last_cfg.lock().unwrap() = Some((freq, bw));
    }
}

#[tokio::test]
async fn repeating_timer_fires_many_times() {
    let mut m = Tk::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(120)).await;
        let _ = sender.send(Ev::Halt);
    });
    let _ = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    let ctx = m.into_context();
    let ticks = *ctx.ticks.lock().unwrap();
    // 10ms timer running for ~120ms should fire at least 5 times. We give
    // loose bounds to tolerate scheduler jitter under `cargo test`.
    assert!(
        (5..=30).contains(&ticks),
        "expected 5..=30 ticks, got {}",
        ticks
    );
}

#[tokio::test]
async fn tuple_variant_payload_binding_dispatches() {
    let mut m = Tk::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn(async move {
        let _ = sender.send(Ev::PacketRx(-42, 7));
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = sender.send(Ev::Halt);
    });
    let _ = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    let ctx = m.into_context();
    assert_eq!(*ctx.last_rx.lock().unwrap(), Some((-42, 7)));
}

#[tokio::test]
async fn struct_variant_payload_binding_dispatches() {
    let mut m = Tk::new(Ctx::default());
    let sender = m.sender();
    tokio::spawn(async move {
        let _ = sender.send(Ev::ConfigChanged {
            freq: 915_000_000,
            bw: 125_000,
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _ = sender.send(Ev::Halt);
    });
    let _ = tokio::time::timeout(Duration::from_secs(2), m.run()).await;
    let ctx = m.into_context();
    assert_eq!(*ctx.last_cfg.lock().unwrap(), Some((915_000_000, 125_000)));
}
