//! End-to-end Embassy example: ISR push, task push, timer transition,
//! and clean termination — all driven by a single `statechart!` machine
//! bound to a user-declared `static Channel`.
//!
//! This example is a `no_std` library rather than a runnable binary: it
//! contains no executor, no time-driver, and no interrupt vector so it can
//! be checked on any bare-metal target without pulling in board-specific
//! dependencies. The acceptance test is:
//!
//! ```text
//! cargo check --features embassy --example embassy_full --target thumbv7em-none-eabihf
//! ```
//!
//! A real firmware pulls in `embassy-executor`, a time driver, and its own
//! interrupt bindings; the call sites below mirror exactly what the firmware
//! writes.

#![no_std]
#![allow(dead_code)]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use hsmc::{statechart, Duration};

#[derive(Debug, Clone)]
pub enum RadioEv {
    StartRx,
    IrqPacketReady,
    StopRx,
    Shutdown,
}

pub struct RadioData {
    pub packets_seen: u32,
}

statechart! {
    Radio {
        context: RadioData;
        events: RadioEv;
        terminate(Shutdown);
        default(Idle);
        state Idle {
            on(StartRx) => Listening;
        }
        state Listening {
            entry: on_enter_listening;
            exit: on_exit_listening;
            on(IrqPacketReady) => count_packet;
            on(StopRx) => Idle;
            on(after Duration::from_secs(30)) => Idle;
        }
    }
}

impl RadioActions for RadioActionContext<'_> {
    async fn on_enter_listening(&mut self) {
        // In a real firmware this might `radio.start_rx().await` or
        // `display.flush().await` — actions are `async fn` under the
        // embassy feature so peripheral calls can run inline.
    }
    async fn on_exit_listening(&mut self) {}
    async fn count_packet(&mut self) {
        self.packets_seen = self.packets_seen.saturating_add(1);
    }
}

// --- Capacity shared between the machine's internal emit-queue and the
// external event channel. The two must match — `Machine::new` enforces this
// at the type level via the channel's `N` const generic.
pub const QN: usize = 8;

pub static RADIO_CHAN: Channel<CriticalSectionRawMutex, RadioEv, QN> = Channel::new();

/// Entry point for the radio task — this is what `#[embassy_executor::task]`
/// would wrap in a real firmware.
pub async fn radio_task(ctx: RadioData) -> Result<(), hsmc::HsmcError> {
    let mut machine = Radio::new(ctx, &RADIO_CHAN);
    machine.run().await?;
    // `into_context()` still works after Embassy run exits cleanly.
    let _ctx = machine.into_context();
    Ok(())
}

/// How a *command* task would enqueue events with backpressure.
pub async fn command_task() {
    let sender: RadioSender = sender_from_channel();
    let _ = sender.send(RadioEv::StartRx).await;
    let _ = sender.send(RadioEv::StopRx).await;
}

/// How a hardware ISR would push an event. `try_send` is non-blocking and
/// `RadioSender` is `Copy + Send + Sync`, so it can be captured in a
/// `static` (here we just fetch it each call for brevity).
pub fn on_radio_irq() {
    let sender: RadioSender = sender_from_channel();
    // Best-effort push; drop on overflow rather than panic in an ISR.
    let _ = sender.try_send(RadioEv::IrqPacketReady);
}

/// A `RadioSender` can also be built directly from the static channel when
/// you haven't retained a reference to the `Radio` machine — e.g., an ISR
/// shim initialised before the task spawns.
fn sender_from_channel() -> RadioSender {
    RadioSender::from_channel(&RADIO_CHAN)
}
