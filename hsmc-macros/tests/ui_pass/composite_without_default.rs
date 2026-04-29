//! Compile-pass: a composite state may omit `default(...)`.
//!
//! When a composite has children but no `default`, transitions targeting
//! that composite land on the composite itself — the substates are reachable
//! only via explicit transitions. Microwave-style chart: `Standby` is the
//! resting state and `LowPower` is a sub-mode you enter only on demand.
#![allow(unexpected_cfgs)]

use hsmc::statechart;

#[derive(Debug, Clone)]
pub enum Ev {
    PowerOn,
    PowerOff,
    EnterLowPower,
    LeaveLowPower,
    Halt,
}

#[derive(Default)]
pub struct Ctx;

statechart! {
Microwave {
    context: Ctx;
    events:  Ev;
    default(Standby);
    terminate(Halt);

    state Standby {
        // No `default(...)` — Standby is itself the resting state.
        on(PowerOn) => On;
        on(EnterLowPower) => LowPower;

        state LowPower {
            on(LeaveLowPower) => Standby;
        }
    }

    state On {
        on(PowerOff) => Standby;
    }
}
}

impl MicrowaveActions for MicrowaveActionContext<'_> {}

fn main() {
    let _ = Microwave::new(Ctx);
}
