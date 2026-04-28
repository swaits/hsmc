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
- **Unified observability.** One observation per atom of execution
  fans out at compile time to a journal (`Vec<TraceEvent>`) and/or
  textual trace backends (`defmt`, `log`, `tracing`) — they share a
  single vocabulary, so they cannot diverge. With no sink feature on,
  every site expands to `()` (zero overhead). See
  [Observability](#observability--one-journal-multiple-outputs).
- **Deterministic + mechanically verified.** The `journal` feature
  records every action, transition, timer, and event in order — same
  inputs, byte-identical journal. The runtime queue and timer table
  carry Pearlite specs that Creusot + Why3 + alt-ergo + z3 discharge
  mechanically (`just verify`). 159 tests pin every spec rule
  (journal sequences + trace-format regression + integration).
- **Backward-compatible chart syntax.** Every v0.1 / v0.2 / v0.3
  statechart compiles under v0.4 unchanged. The 0.4 breaks are in
  observability surface area only — see
  [Migration](#migration-from-v03).

## How a chart behaves — the building blocks

Eight rules, each building on the previous. Internalize these and every
edge case below falls out — you don't have to memorize anything else.

The full canonical reference lives at
[`docs/002. hsmc-semantics-formal.md`](docs/002.%20hsmc-semantics-formal.md);
this section is the on-ramp.

### 1. The active path — you are in every state from root to leaf

At any moment, the machine is in a **path** of states from the root down
to one innermost state. If you're in `C` inside `B` inside `A`, you're
**simultaneously** in `Root`, `A`, `B`, and `C`. The root is always
active until termination.

This is the most important rule. Every rule below is downstream of it.
If a behavior somewhere else seems weird, come back here — it's
probably the resolution.

### 2. Default-descent — composites auto-enter their default child

Every state with children declares one `default(...)`. When a composite
state is entered, its entry actions run first, **then** the default
child is entered, recursively, until a leaf.

So `Root` with `default(A)` and `A` with `default(B)` means: the
machine starts by entering `Root` → `A` → `B`, all on first step.

### 3. Event bubbling — leaf first, first handler wins

An event arrives. Search starts at the **innermost** active state. If
that state has a handler, it runs and the event is consumed. If not,
walk up to the parent and check there. Repeat. If you reach the root
with no handler, the event is silently discarded.

Three corollaries:

- Leaf shadows ancestor for the same trigger.
- Multiple handlers in one state on the same trigger fire in
  declaration order; the transition (if any) fires last.
- **Timers don't bubble.** A timer belongs to the state that declared
  it; it fires only in that state's scope.

### 4. Transitions — exit to LCA, enter to target

Any transition's algorithm is uniform:

1. Find the LCA (lowest common ancestor) of current and target.
2. Walk **up** from current, exiting each state, until you hit the LCA.
   The LCA itself is **not** exited.
3. Walk **down** from the LCA to target, entering each state. The LCA
   is **not** re-entered.
4. If target has children, default-descend (rule 2).

The LCA always exists because root is always an upper bound. State
names are globally unique across the chart, so a transition can target
**any** state — siblings, cousins, ancestors, the root itself
(`on(Trig) => MyChart;`).

### 5. The up-transition rule — never re-enter what you never left

If your transition target is **already on the active path** (i.e., it's
an ancestor of the innermost state), then steps 3 and 4 of the
transition algorithm do nothing. You exit the states strictly between
current and target; the target itself is **not** re-entered.

So from leaf `C` inside `B` inside `A`, an `on(Up) => A;` exits `C`
then `B`. `A`'s entry actions don't fire. `A`'s `default` is not
followed. `A`'s timers don't restart. `A`'s durings keep running.

You cannot enter a state you never left.

### 6. Entry / exit ordering

Direction is set by the path:

- **Entries** fire **outer-to-inner**: when entering down through a
  path, the outermost state's entries fire first, then the next, then
  the leaf's. Within a single state, entry actions fire in declaration
  order. After a state's entries finish, its `during:` activities
  start. Then default-descent (rule 2) applies recursively.
- **Exits** fire **inner-to-outer**: leaf first, then parent, then
  grandparent. Within a single state, exit actions fire in declaration
  order. Before a state's exits begin, its `during:` activities are
  cancelled.

So a full transition reads: cancel durings on the way out → run exits
on the way out → run entries on the way in → start durings on the way
in.

### 7. Timers — armed by state lifecycle

A timer (`on(after D) => …` or `on(every D) => …`) is **armed** when
its declaring state is entered, **cancelled** when that state is
exited. Descending into a child is _not_ an exit, so the parent's
timers keep running while you're in a child. Re-entering a state
restarts its timers from zero.

### 8. emit / during / termination — all built on the rules above

- **`emit(ev)`** from inside an action queues an event. The runtime
  finishes the current event's handling — including every entry/exit
  in the resulting transition — **before** dequeuing the new event.
  No re-entrant dispatch.
- **`during:`** is an async future tied to its state's lifecycle. It
  starts after the state's entry actions finish and is cancelled (the
  future is dropped) before the state's exit actions begin. Multiple
  durings on the active path race; each borrows disjoint `&mut` fields
  of the context (Rust's split-borrow checker enforces this at compile
  time).
- **Termination** is just rules 6 + 4 with target = "outside the
  chart": exit the entire active path inner-to-outer, then stop.
  Pending events drop. In-flight durings drop at their next `.await`.

That's the whole behavior model. The
[full semantics doc](docs/002.%20hsmc-semantics-formal.md)
spells out edge cases (queue overflow surfacing, same-tick timer ties,
self-transitions on composites) but the rules above cover the
overwhelming majority of charts.

### What `hsmc` deliberately does NOT have

The bet is on **simple, consistent semantics**. These features are
omitted on purpose; for each, what to do instead:

| Missing feature                 | Use instead                                                                                           |
| ------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Guard conditions on transitions | Split source into two states, or have an action `emit()` a follow-up event for the chart to branch on |
| Orthogonal / parallel regions   | Two charts as two tasks connected by channels, or multiple `during:` activities in one state          |
| History states                  | Store last child explicitly in your context; transition to it on re-entry                             |
| Deferred events                 | Handle where they arrive (drop/log if not relevant), or have producer check state                     |
| Internal transitions            | `action(Trig) => fn;` (no transition) — exactly that                                                  |
| State-local context             | All state in the root context; durings borrow disjoint fields                                         |
| Event priorities beyond FIFO    | Higher-priority events on a separate channel into a separate, faster machine                          |
| Runtime statechart modification | Design every variation up front as states                                                             |

## Quickstart

Add to `Cargo.toml`:

```toml
[dependencies]
hsmc = { version = "0.4", features = ["embassy"] }  # or ["tokio"]
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

| Item                       | What it is                                                            |
| -------------------------- | --------------------------------------------------------------------- |
| `FooState`                 | Enum of every user-declared state.                                    |
| `FooActions`               | Trait with one async fn per unique action name. User implements.      |
| `FooActionContext<'a>`     | Wrapper passed to actions. `Deref`s to the root context. `emit()`.    |
| `FooCtx<'a>`               | Type alias for `FooActionContext<'a>`.                                |
| `Foo<const QN: usize = 8>` | The machine itself.                                                   |
| `FooSender`                | Cloneable handle for cross-task / ISR event injection.                |
| `Foo::<8>::STATE_CHART`    | `&'static str` ASCII tree of the hierarchy (useful for `defmt`/docs). |

### Machine methods

| Method                                 | Notes                                                  |
| -------------------------------------- | ------------------------------------------------------ |
| `new(ctx)` / `new(ctx, channel)`       | Construct. Under embassy, pass an `&'static Channel`.  |
| `with_queue_capacity::<N>(ctx,...)`    | Custom event-queue capacity.                           |
| `async fn run()`                       | Drive the machine to completion (async features).      |
| `async fn dispatch(ev)`                | Push event + drain to quiescence. Same task injection. |
| `fn step(elapsed) -> Option<Duration>` | One unit of work (test/polling driver).                |
| `fn send(ev)`                          | Raw push, non-draining. For tests.                     |
| `sender() -> Sender`                   | Clone handle for other tasks / ISRs.                   |
| `current_state()`                      | The active leaf state.                                 |
| `is_terminated()`                      | True after `terminate` event.                          |
| `has_pending_events()`                 | Queue non-empty.                                       |
| `context()` / `context_mut()`          | Borrow the root context.                               |
| `into_context(self)`                   | Consume, return ctx.                                   |
| `STATE_CHART`                          | Const ASCII diagram.                                   |

## Feature flags

| Feature   | Effect                                                                     |
| --------- | -------------------------------------------------------------------------- |
| (default) | `no_std`, no async runtime. Drive via `step(Duration)` + `send(ev)`.       |
| `tokio`   | Async actions + `run()`. Targets single-thread via `LocalSet`.             |
| `embassy` | Async actions + `run()`. Targets embassy's cooperative executor. `no_std`. |

`embassy` and `tokio` are mutually exclusive — enabling both is a compile
error.

## Migration from v0.3

The 0.4 breaks are in the observability surface area only; chart
syntax and runtime API are unchanged.

### 1. Trace prefix changed

Trace lines (when `trace-defmt` / `trace-log` / `trace-tracing` is
enabled) now use `[statechart:Name]` instead of `[Name]`. Update any
log filters or grep patterns. The verb after the prefix is now a
single hyphenated token followed by `key=value` pairs (logfmt).

### 2. `TraceEvent::TransitionFired` carries `reason`

Add `reason` to any pattern matches:

```rust
// before:
TraceEvent::TransitionFired { from, to } => …
// after:
TraceEvent::TransitionFired { from, to, reason } => …
// or, if you don't need it:
TraceEvent::TransitionFired { from, to, .. } => …
```

The new field carries `TransitionReason::Event { event }`,
`Timer { state, timer }`, or `Internal` so consumers can see what
triggered the transition.

### 3. New `TraceEvent` variants

If you `match` exhaustively on `TraceEvent`, add arms for the new
variants. Otherwise nothing changes:

- `EnterBegan { state }` — phase begin marker pairs with `Entered`.
- `ExitBegan { state }` — phase begin marker pairs with `Exited`.
- `TransitionComplete { from, to }` — pairs with `TransitionFired`.
- `EventReceived { event }` — fires when an event is popped from the
  queue, before handler search.

These are pure observation — no runtime semantic change.

### 4. `trace;` keyword is a no-op

Charts that declared `trace;` continue to compile. The keyword no
longer gates emission — observation is now controlled purely by
which `trace-*` cargo feature you enable. You can leave `trace;` in
or remove it; both work.

### 5. Trace backends are no longer mutually exclusive

Previously, enabling more than one of `trace-defmt` / `trace-log` /
`trace-tracing` was a compile error. Now they fan out independently:
turn on whichever you want.

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

| Example         | Purpose                                                               |
| --------------- | --------------------------------------------------------------------- |
| `microwave`     | Full-feature pure-event machine: entry/exit, timers, emit, terminate. |
| `microwave_tui` | TUI variant of microwave.                                             |
| `during_radio`  | `during:` activities simulating a radio task with RX + TX loops.      |
| `embassy_full`  | Embassy feature demonstration.                                        |

Run `cargo run --example during_radio --features tokio` to see the
`during:` pattern end-to-end.

## Verification

`hsmc` ships with a Creusot deductive-verification crate
(`verification/`) that proves the runtime's `EventQueue` and
`TimerTable` correct against Pearlite specs. Why3 + alt-ergo + z3
discharge every VC mechanically. Run:

```bash
just verify
```

The recipe is self-installing (mise pulls in the pinned nightly,
opam, Why3, the SMT solvers, and `cargo-creusot`) so a fresh clone
gets to a green proof in one command. See
[`verification/INVARIANTS.md`](verification/INVARIANTS.md) for the
spec-rule → proof mapping.

Beyond Creusot, the suite includes:

- **159 integration tests** — full chart behavior, byte-equal expected-
  vs-actual journals for ~113 deterministic-flow tests, plus 11 trace-
  format regression tests pinning every observation verb's exact
  textual rendering.
- **0 missed mutants** under `cargo mutants --features
  tokio,journal,trace-log`.
- **Miri**: 68 tests, no UB.

## Observability — one journal, multiple outputs

Every chart emits a single observation per atom of execution. That
observation fans out to whichever sinks you compile in:

| Feature         | Sink                                                     |
| --------------- | -------------------------------------------------------- |
| `journal`       | In-memory `Vec<TraceEvent>` (replay, byte-deterministic) |
| `trace-defmt`   | `defmt::*` log lines                                     |
| `trace-log`     | `log::*` log lines                                       |
| `trace-tracing` | `tracing::*` events with structured fields               |

With **no** sink feature on, observation is fully elided at compile
time — zero overhead. Multiple sinks can be enabled simultaneously;
the journal IS the observation stream and trace backends are textual
renderings of it, so they cannot diverge.

The textual format is logfmt-style — greppable and regex-parseable:

```
[statechart:Microwave] started chart_hash=0xa1b2c3d4
[statechart:Microwave] entering state=Idle
[statechart:Microwave] action state=Idle action=clear_display kind=entry
[statechart:Microwave] timer-armed state=Idle timer=t0 duration_ns=5000000000
[statechart:Microwave] during-started state=Idle during=poll_sensor
[statechart:Microwave] entered state=Idle
[statechart:Microwave] event-received name=StartButton
[statechart:Microwave] event-delivered name=StartButton handler=Idle
[statechart:Microwave] transition-begin from=Idle to=Heating reason=event:StartButton
[statechart:Microwave] exiting state=Idle
[statechart:Microwave] timer-cancelled state=Idle timer=t0
[statechart:Microwave] during-cancelled state=Idle during=poll_sensor
[statechart:Microwave] action state=Idle action=stop_motor kind=exit
[statechart:Microwave] exited state=Idle
[statechart:Microwave] entering state=Heating
[statechart:Microwave] entered state=Heating
[statechart:Microwave] transition-complete from=Idle to=Heating
```

### The verb vocabulary

Twenty verbs cover every `TraceEvent` variant. Begin/end markers
bracket entry, exit, and transition phases so the actions, timers,
and durings running inside each phase are visible.

| Verb | Fires when |
|---|---|
| `started` | First step of a chart's life — carries `chart_hash` |
| `entering` / `entered` | Begin / end of a state's entry phase |
| `exiting` / `exited` | Begin / end of a state's exit phase |
| `action` | An action fn was called — `kind=entry\|exit\|handler` |
| `during-started` / `during-cancelled` | A `during:` activity started / was dropped |
| `transition-begin` / `transition-complete` | Begin / end of a transition's exit-then-enter sequence |
| `event-received` | An event was popped from the queue |
| `event-delivered` | A handler ran for the event |
| `event-dropped` | The event reached root with no handler |
| `emit-queued` / `emit-failed` | `emit()` from inside an action — queued or rejected |
| `timer-armed` / `timer-cancelled` / `timer-fired` | Timer lifecycle |
| `terminate-requested` | `terminate(...)` event matched, exit chain about to run |
| `terminated` | Chart finished (always last) |

The `reason=` field on `transition-begin` records what triggered the
transition: an event (`event:VariantName`), a timer
(`timer:State/timer-id`), or `internal`. Real-life captures can be
diffed against expected behavior the same way the journal can.

## Status

- v0.1 spec: fully implemented.
- v0.2: `during:`, optional `events:`, `dispatch()`, `STATE_CHART`,
  drain-on-send. Up-transitions no longer exit/re-enter the target.
- v0.3: `journal` feature + deterministic trace + replay; root
  state targetable by chart name; Creusot verification of the
  runtime data structures; 113-test deterministic-flow suite.
- v0.4: unified observation pipeline (journal + trace share one
  vocabulary, fan out to multiple sinks at compile time); logfmt-
  style trace format with `[statechart:Name]` prefix; transition
  reasons; entry/exit/transition begin/end markers; multiple trace
  backends now compose; chart-level `trace;` keyword deprecated.
  Breaking on the `TraceEvent` enum shape; chart syntax unchanged.
- Out of scope (deliberate): guard conditions, orthogonal/parallel
  regions, history states, deferred events, internal transitions,
  state-local context, event priorities beyond FIFO, runtime
  statechart modification, multi-threaded tokio. See
  [`docs/002. hsmc-semantics-formal.md`](docs/002.%20hsmc-semantics-formal.md)
  § "What `hsmc` deliberately does NOT have" for the rationale and
  what to use instead.

## License

Dual-licensed MIT OR Apache-2.0.
