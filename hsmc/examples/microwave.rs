//! # Microwave Oven Controller — a tour of every `hsmc` feature.
//!
//! Run with:
//!
//! ```text
//! cargo run --example microwave --features tokio
//! ```
//!
//! This single example exercises the entire public surface of the crate:
//!
//! | Feature                                          | Where                                   |
//! |--------------------------------------------------|-----------------------------------------|
//! | `statechart!` proc macro                         | the big block below                     |
//! | Nested states (3 levels deep)                    | `Running { Cooking { Heating / Paused } }` |
//! | `default(...)` descent at every level            | root, `Running`, `Cooking`              |
//! | Multiple entry actions (comma form)              | `Idle` entry                            |
//! | Multiple entry actions (repeated-keyword form)   | root entry                              |
//! | Multiple exit actions                            | `Paused` exit                           |
//! | Event-driven transitions                         | `StartPressed`, `StopPressed`, ...      |
//! | Timer-driven transitions (`Duration`)            | `Cooking` countdown, `Done` auto-reset  |
//! | Event-driven actions (no state change)           | `DoorOpened` beep in `Idle`             |
//! | Timer-driven actions (no state change)           | `Running` heartbeat tick                |
//! | Actions *and* transition on same trigger         | `StartPressed` in `Idle`                |
//! | Multiple actions on the same trigger             | `StartPressed` has two action handlers  |
//! | Event bubbling up the hierarchy                  | `StopPressed` from `Heating` → `Running`|
//! | Cross-hierarchy transition (leaf → sibling-of-ancestor) | `StopPressed` target         |
//! | Self-transition (exit + re-entry, timer restart) | `Done` on `StartPressed`? no — see `Idle` `Reset` |
//! | Transition to state with children → default descent | `StartPressed` in `Idle` → `Running` |
//! | Ancestor-timer survives child transitions        | `Running` cook timer across `Heating`⇄`Paused` |
//! | `terminate(...)` at root                         | `PowerFailure` event                    |
//! | Exit cascade on terminate                        | logged in exit actions                  |
//! | Shared action name across states                 | `beep` used in `Idle` and `Done`        |
//! | `emit()` from inside an action                   | `Done` auto-reset emits `StartPressed`  |
//! | `with_queue_capacity::<N>`                       | machine created with capacity 16        |
//! | `sender()` — cloneable, sendable across tasks    | button-press + fault tasks              |
//! | `run()` on the Tokio runtime                     | `main` awaits it                        |
//! | `into_context()` after termination               | final report at the end                 |
//! | `current_state()` / `is_terminated()`            | printed during shutdown                 |
//! | Action context: `Deref`/`DerefMut` to user ctx   | every action touches `self.<field>`     |
//!
//! The oven runs a short cook job, is interrupted by a door-open (pause),
//! resumed by a door-close, completes, auto-resets via `emit`, then a simulated
//! power failure terminates the machine.

use hsmc::{statechart, Duration};
use std::sync::{Arc, Mutex};

// ---------- User context ----------
//
// The machine never sees or pollutes this struct — it is the user's own data.
// Inside action methods we reach these fields via `Deref`/`DerefMut` on the
// generated `ActionContext`.

#[derive(Default)]
pub struct OvenContext {
    /// Human-readable transcript of everything that happened. Kept for the
    /// final report printed after the machine terminates.
    pub log: Vec<String>,
    /// Seconds remaining on the current cook job (mutated by actions).
    pub remaining_secs: u32,
    /// Simulated hardware state, shared with the outside world so the driving
    /// code can inspect it without owning the machine.
    pub hw: Arc<Mutex<Hardware>>,
}

#[derive(Default, Debug)]
pub struct Hardware {
    pub door_open: bool,
    pub magnetron_on: bool,
    pub heater_on: bool,
    pub display: String,
}

// ---------- Events ----------
//
// Plain user-defined enum. `Clone` is needed by the internal queue; `Debug`
// is for the transcript.

#[derive(Debug, Clone)]
pub enum OvenEvent {
    StartPressed,
    StopPressed,
    DoorOpened,
    DoorClosed,
    /// Emitted by the machine itself from a timer action in `Done`.
    Reset,
    /// Root-level `terminate` trigger.
    PowerFailure,
}

// ---------- Statechart ----------
//
// Declarative structure only. No behavior. Reading this block tells you
// everything about the control flow. The macro validates it at compile time:
// missing defaults, duplicate transitions, `terminate` outside root, and
// unknown state references all become compile errors.

statechart! {
Oven {
    context: OvenContext;
    events: OvenEvent;

    // Root-level entry/exit actions — repeated-keyword form. Invoked in
    // declaration order on initial entry and in reverse-declaration inner→
    // outer order at termination.
    entry: boot_self_test;
    entry: log_boot_done;
    exit: save_last_state;
    exit: cut_all_power;

    // Required: which child to enter on initial step.
    default(Idle);

    // Only valid at root. When this event fires in any active state, the full
    // active configuration is exited (innermost → outermost, calling exit
    // actions) and the machine terminates.
    terminate(PowerFailure);

    state Idle {
        // Comma form for multi-entry.
        entry: show_ready, clear_remaining;
        exit: clear_display;

        // Two actions AND a transition on the same trigger. Actions fire in
        // declaration order, then the transition executes.
        on(StartPressed) => record_start_request;
        on(StartPressed) => prime_cook_timer;
        on(StartPressed) => Running;

        // Event-driven action (no state change).
        on(DoorOpened) => beep;
    }

    // Composite state with its own timer (the total cook duration). The
    // timer belongs to `Running` and survives transitions between its
    // descendants (`Heating` ⇄ `Paused`).
    state Running {
        entry: start_magnetron;
        exit: stop_magnetron;
        default(Cooking);

        // Cook duration. This whole-oven timer keeps running while the door
        // is open (the user can pause; the job clock does not).
        // NOTE: in a real oven you'd pause this; here it stays to demonstrate
        // ancestor-timer survival across child transitions (spec §2.13).
        on(after Duration::from_millis(2500)) => Done;

        // Heartbeat every 200ms — a timer-driven action, no state change.
        on(after Duration::from_millis(200)) => heartbeat;

        // Cancelling the cook job bubbles from any descendant to here.
        on(StopPressed) => Idle;

        state Cooking {
            entry: show_cooking;
            default(Heating);

            state Heating {
                entry: heater_on;
                exit: heater_off;
                // Door opens → pause. Sibling transition, one level up.
                on(DoorOpened) => Paused;
            }

            state Paused {
                // Multi-line exit form.
                entry: show_paused, start_pause_blink;
                exit: stop_pause_blink;
                exit: log_resume;
                on(DoorClosed) => Heating;
            }
        }
    }

    state Done {
        // `beep` is shared with `Idle` — a single trait method serves both.
        entry: beep, show_done;
        exit: clear_display;

        // Timer-driven action that emits an event back into the queue. Spec
        // §2.12: emitted events are processed AFTER the current handler
        // finishes. Here the emitted `Reset` drives a transition below.
        on(after Duration::from_millis(500)) => schedule_auto_reset;

        // Self-transition: Done → Done. Exit + re-entry runs, timer restarts.
        on(StartPressed) => Done;

        // Responding to the emitted event.
        on(Reset) => Idle;
    }
}
}

// ---------- Action implementations ----------
//
// The macro generates `OvenActions` (one method per unique action name used
// anywhere above) and `OvenActionContext<'_>` (a wrapper that derefs to
// `OvenContext` and exposes `emit`). The user simply implements the trait.

impl OvenActions for OvenActionContext<'_> {
    // Root
    async fn boot_self_test(&mut self) {
        self.log.push("BOOT: self-test ok".into());
    }
    async fn log_boot_done(&mut self) {
        self.log.push("BOOT: ready".into());
    }
    async fn save_last_state(&mut self) {
        self.log.push("SHUTDOWN: persisted last state".into());
    }
    async fn cut_all_power(&mut self) {
        {
            let mut hw = self.hw.lock().unwrap();
            hw.magnetron_on = false;
            hw.heater_on = false;
            hw.display = "-- OFF --".into();
        }
        self.log.push("SHUTDOWN: power cut".into());
    }

    // Idle
    async fn show_ready(&mut self) {
        self.hw.lock().unwrap().display = "READY".into();
        self.log.push("IDLE: ready".into());
    }
    async fn clear_remaining(&mut self) {
        self.remaining_secs = 0;
    }
    async fn clear_display(&mut self) {
        self.hw.lock().unwrap().display.clear();
    }
    async fn record_start_request(&mut self) {
        self.log.push("BTN: start pressed".into());
    }
    async fn prime_cook_timer(&mut self) {
        // Uses DerefMut to mutate the user context directly.
        self.remaining_secs = 3;
        let n = self.remaining_secs;
        self.log.push(format!("BTN: primed {}s cook", n));
    }
    async fn beep(&mut self) {
        // Shared between Idle (DoorOpened) and Done (entry).
        self.log.push("*BEEP*".into());
    }

    // Running
    async fn start_magnetron(&mut self) {
        self.hw.lock().unwrap().magnetron_on = true;
        self.log.push("MAG: on".into());
    }
    async fn stop_magnetron(&mut self) {
        self.hw.lock().unwrap().magnetron_on = false;
        self.log.push("MAG: off".into());
    }
    async fn heartbeat(&mut self) {
        self.log.push("tick".into());
    }

    // Cooking / Heating
    async fn show_cooking(&mut self) {
        self.hw.lock().unwrap().display = "COOKING".into();
    }
    async fn heater_on(&mut self) {
        self.hw.lock().unwrap().heater_on = true;
        self.log.push("HEAT: on".into());
    }
    async fn heater_off(&mut self) {
        self.hw.lock().unwrap().heater_on = false;
        self.log.push("HEAT: off".into());
    }

    // Paused
    async fn show_paused(&mut self) {
        self.hw.lock().unwrap().display = "PAUSED".into();
    }
    async fn start_pause_blink(&mut self) {
        self.log.push("PAUSE: blink on".into());
    }
    async fn stop_pause_blink(&mut self) {
        self.log.push("PAUSE: blink off".into());
    }
    async fn log_resume(&mut self) {
        self.log.push("PAUSE: resuming".into());
    }

    // Done
    async fn show_done(&mut self) {
        self.hw.lock().unwrap().display = "DONE".into();
    }
    async fn schedule_auto_reset(&mut self) {
        // emit() is provided by the generated ActionContext. It returns a
        // Result so callers can handle queue overflow (here we panic because
        // the queue is plenty big and overflow would be a bug).
        self.log.push("DONE: auto-reset queued".into());
        self.emit(OvenEvent::Reset).expect("queue has capacity");
    }
}

// ---------- Driver ----------

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let hw = Arc::new(Mutex::new(Hardware::default()));
    let ctx = OvenContext { hw: Arc::clone(&hw), ..Default::default() };

    // `with_queue_capacity::<N>` overrides the default queue size (8). We
    // pick 16 to comfortably hold the burst of events below.
    let mut oven = Oven::with_queue_capacity::<16>(ctx);

    // `sender()` returns a cheap, cloneable, Send handle — usable from other
    // tasks, threads, or ISRs (under the tokio feature).
    let buttons = oven.sender();
    let faults = oven.sender();

    // Simulate a user pushing buttons and opening/closing the door.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        buttons.send(OvenEvent::StartPressed).unwrap();

        // Open door partway through — should transition Heating → Paused.
        tokio::time::sleep(Duration::from_millis(600)).await;
        buttons.send(OvenEvent::DoorOpened).unwrap();

        // Close it 400ms later — Paused → Heating.
        tokio::time::sleep(Duration::from_millis(400)).await;
        buttons.send(OvenEvent::DoorClosed).unwrap();

        // Cook timer (2500ms from Running entry) fires around here → Done.
        // Done's timer-action emits Reset → machine returns to Idle.
        // Wait for auto-reset, then press Start again for a second cycle.
        tokio::time::sleep(Duration::from_millis(2000)).await;
        buttons.send(OvenEvent::StartPressed).unwrap();

        // This time cancel via Stop before the cook completes.
        tokio::time::sleep(Duration::from_millis(400)).await;
        buttons.send(OvenEvent::StopPressed).unwrap();
    });

    // Simulated fault monitor — kills the machine after the demo has played
    // out. Demonstrates `terminate(PowerFailure)` and the exit cascade.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5000)).await;
        let _ = faults.send(OvenEvent::PowerFailure);
    });

    println!("=== Microwave coming online ===\n");
    oven.run().await.expect("run returned Err");

    // After termination we can inspect final state and reclaim the context.
    println!("\n=== Machine terminated ===");
    println!("is_terminated() = {}", oven.is_terminated());
    println!("hardware = {:?}", hw.lock().unwrap());

    let ctx = oven.into_context();
    println!("\n--- Transcript ({} entries) ---", ctx.log.len());
    for (i, line) in ctx.log.iter().enumerate() {
        println!("{:>3}. {}", i + 1, line);
    }
}
