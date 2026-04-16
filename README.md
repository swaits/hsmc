# hsmc — Hierarchical State Machines for Rust

`hsmc` is a declarative, proc-macro-based statechart crate for Rust. It targets
embedded Rust (embassy) as the primary runtime and supports `tokio` via
`LocalSet`. Machines are compiled from a `statechart!` block into
monomorphized code with no dynamic dispatch, no interior mutability, and no
heap allocations by default.

```rust
statechart! {
    Radio {
        context: Ctx;
        events:  Ev;
        default(Idle);

        state Idle {
            on(StartRx) => Receiving;
        }
        state Receiving {
            during: next_packet(lora, rx_buf);
            on(PacketRx { rssi: i16, snr: i16 }) => record_packet;
            on(StopRx) => Idle;
        }
    }
}
```

## What it does

- **Harel-style hierarchical states.** Nested states with `entry:`/`exit:`
  actions, LCA-aware transitions, default-child descent, `terminate`.
- **Event-driven transitions and actions.** Typed payload bindings:
  `on(PacketRx { rssi: i16 }) => handler;`.
- **Timer triggers.** `on(after Duration::from_secs(5)) => Next;` and
  `on(every Duration::from_millis(100)) => tick;`.
- **`during:` activities.** Async functions that run while a state is
  active and produce events. Multiple durings can borrow disjoint fields
  of the context concurrently via Rust's native split borrow — no
  `RefCell`, no `Mutex`, no `UnsafeCell` in user code.
- **`m.run().await` is the whole task body.** Races active durings, the
  external event channel, and timer deadlines. The MCU sleeps between
  events (embassy's cooperative executor parks all futures).
- **Backward-compatible.** Every v0.1 statechart compiles under v0.2
  unchanged.

## Quickstart

Add to `Cargo.toml`:

```toml
[dependencies]
hsmc = { version = "0.2", features = ["embassy"] }  # or ["tokio"]
```

Minimal timer-only machine:

```rust
use hsmc::{statechart, Duration};

pub struct Ctx { pub ticks: u32 }

statechart! {
    Blinker {
        context: Ctx;
        default(On);

        state On {
            entry:  light_on;
            on(after Duration::from_millis(500)) => Off;
        }
        state Off {
            on(after Duration::from_millis(500)) => On;
        }
    }
}

impl BlinkerActions for BlinkerActionContext<'_> {
    async fn light_on(&mut self) { self.ticks += 1; }
}
```

Drive it under tokio:

```rust
let mut m = Blinker::new(Ctx { ticks: 0 });
tokio::task::LocalSet::new().run_until(m.run()).await.unwrap();
```

Or under embassy:

```rust
static CHANNEL: Channel<CriticalSectionRawMutex, __NoEvents, 8> = Channel::new();
// (timer-only machines don't need a channel; omit `events:` and use `Blinker::new(ctx)`.)

#[embassy_executor::task]
async fn blink_task() {
    let mut m = Blinker::new(Ctx { ticks: 0 });
    let _ = m.run().await;
}
```

## `during:` — async activities scoped to a state

A `during:` runs while its state is on the active path. It's a plain async
function that takes `&mut` references to specific fields of the root context
and returns one event:

```rust
state Receiving {
    during: next_packet(lora, rx_buf);
    on(PacketRx { rssi: i16 }) => count_packet;
    on(StopRx) => Idle;
}

async fn next_packet(
    lora:   &mut LoRaDriver,
    rx_buf: &mut [u8; 256],
) -> RadioInput {
    lora.rx(rx_buf).await.into()
}
```

The macro emits `next_packet(&mut ctx.lora, &mut ctx.rx_buf)` and Rust's
native split-borrow verifies the fields don't overlap with any other
concurrent during on the active path. Overlapping fields produce a
compile-time error.

### Multiple concurrent durings

A state (or any ancestor on its path) may declare more than one `during:`.
The run loop races all of them against the external channel and the next
timer deadline via `tokio::select!` / `embassy_futures::select`. Each
during borrows disjoint fields, so the split borrow is free.

```rust
state Configured {
    during: heartbeat(hb_counter);     // active in every Configured substate
    state Receiving {
        during: next_packet(lora, rx_buf);
    }
}
```

### Cancel-safety contract

> A `during:` future **will** be dropped at any `.await` point when a
> handler fires, a timer expires, an external event arrives, or the state
> transitions. Write durings as cancel-safe state machines: every
> `.await` must be a clean resume point where dropping the future leaves
> the borrowed fields in a valid, re-enterable state.

**Anti-pattern:**

```rust
async fn bad(acc: &mut u32) -> Ev {
    *acc += 1;                                   // commit happens here
    tokio::time::sleep(Duration::from_ms(100)).await;  // ← may be dropped
    *acc += 1;                                   // may never run
    Ev::Tick
}
```

**Preferred:**

```rust
async fn good(acc: &mut u32) -> Ev {
    tokio::time::sleep(Duration::from_ms(100)).await;
    *acc = acc.wrapping_add(2);                   // all mutations after the .await
    Ev::Tick
}
```

### Starvation in select-drop semantics

When several durings race, the shortest-to-complete wins every iteration
and the others are dropped without making progress. This is expected:
firmware I/O activities are naturally cancel-safe (a dropped `lora.rx()`
just stops receiving) and the starved ones would typically be
heartbeat-like tasks that deliver their events eventually.

If two durings compete for the same "always ready" resource, combine them
into a single during that uses `select` internally.

## Generated API per machine

For `statechart! Foo { ... }` the macro emits:

| Item                            | What it is                                                           |
|---------------------------------|----------------------------------------------------------------------|
| `FooState`                      | Enum of every user-declared state.                                   |
| `FooActions`                    | Trait with one async fn per unique action name. User implements.     |
| `FooActionContext<'a>`          | Wrapper passed to actions. `Deref`s to the root context. `emit()`.  |
| `FooCtx<'a>`                    | Type alias for `FooActionContext<'a>`.                               |
| `Foo<const QN: usize = 8>`      | The machine itself.                                                  |
| `FooSender`                     | Cloneable handle for cross-task / ISR event injection.               |
| `Foo::<8>::STATE_CHART`         | `&'static str` ASCII tree of the hierarchy (useful for `defmt`/docs).|

### Machine methods

| Method                              | Notes                                                            |
|-------------------------------------|------------------------------------------------------------------|
| `new(ctx)` / `new(ctx, channel)`    | Construct. Under embassy, pass an `&'static Channel`.            |
| `with_queue_capacity::<N>(ctx,...)` | Custom event-queue capacity.                                     |
| `async fn run()`                    | Drive the machine to completion (async features).                |
| `async fn dispatch(ev)`             | Push event + drain to quiescence. Same task injection.           |
| `fn step(elapsed) -> Option<Duration>` | One unit of work (test/polling driver).                       |
| `fn send(ev)`                       | Raw push, non-draining. For tests.                               |
| `sender() -> Sender`                | Clone handle for other tasks / ISRs.                             |
| `current_state()`                   | The active leaf state.                                           |
| `is_terminated()`                   | True after `terminate` event.                                    |
| `has_pending_events()`              | Queue non-empty.                                                 |
| `context()` / `context_mut()`       | Borrow the root context.                                         |
| `into_context(self)`                | Consume, return ctx.                                             |
| `STATE_CHART`                       | Const ASCII diagram.                                             |

## Feature flags

| Feature   | Effect                                                                  |
|-----------|-------------------------------------------------------------------------|
| (default) | `no_std`, no async runtime. Drive via `step(Duration)` + `send(ev)`.    |
| `tokio`   | Async actions + `run()`. Targets single-thread via `LocalSet`.          |
| `embassy` | Async actions + `run()`. Targets embassy's cooperative executor. `no_std`.|

`embassy` and `tokio` are mutually exclusive — enabling both is a compile
error.

## Migration from v0.1

v0.1 statecharts continue to compile. New features are purely additive:

### 1. `events:` is now optional

Previously required; now omit for pure timer-only machines:

```rust
statechart! {
    Blinker {
        context: Ctx;
        default(On);
        state On  { on(after Duration::from_millis(500)) => Off; }
        state Off { on(after Duration::from_millis(500)) => On; }
    }
}
```

### 2. `dispatch(ev).await` replaces the push+drain helper

Before (v0.1, in the dogfood firmware):

```rust
async fn push<M>(m: &mut M, ev: E) {
    m.send(ev).expect("queue full");
    while m.has_pending_events() {
        m.step(Duration::ZERO).await;
    }
}
push(&mut machine, ev).await;
```

After (v0.2):

```rust
machine.dispatch(ev).await.unwrap();
```

### 3. `during:` replaces the imperative I/O outer loop

Before — a radio task with per-state match and select:

```rust
loop {
    match machine.current_state() {
        RadioState::Receiving => {
            match select(rx_once(&mut lora, &cfg, &mut rx_buf),
                         commands.receive()).await {
                Either::First(r) => handle_rx(r, &mut machine, ...).await,
                Either::Second(c) => handle_cmd(c, &mut machine, ...).await,
            }
        }
        _ => { ... }
    }
}
```

After — the statechart owns the loop:

```rust
state Receiving {
    during: next_packet(lora, rx_buf);
    on(PacketRx { rssi: i16, snr: i16 }) => record_packet;
    on(StopRx) => Idle;
}

#[embassy_executor::task]
async fn radio_task(lora: LoRaDriver, ch: &'static RadioChan) {
    let mut m = Radio::new(RadioCtx { lora, ... }, ch);
    m.run().await.unwrap();
}
```

## Examples

| Example            | Purpose                                                             |
|--------------------|---------------------------------------------------------------------|
| `microwave`        | Full-feature pure-event machine: entry/exit, timers, emit, terminate.|
| `microwave_tui`    | TUI variant of microwave.                                            |
| `during_radio`     | `during:` activities simulating a radio task with RX + TX loops.     |
| `embassy_full`     | Embassy feature demonstration.                                       |

Run `cargo run --example during_radio --features tokio` to see the
`during:` pattern end-to-end.

## Status

- v0.1 spec: fully implemented.
- v0.2 additions: `during:`, optional `events:`, `dispatch()`, `STATE_CHART`,
  drain-on-send.
- Out of scope: state-scoped context types (hierarchy), guard conditions,
  orthogonal/parallel regions, history states, runtime statechart
  modification, multi-threaded tokio.

## License

Dual-licensed MIT OR Apache-2.0.
