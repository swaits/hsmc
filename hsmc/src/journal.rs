//! Deterministic execution journal for `hsmc` charts.
//!
//! Behind `feature = "journal"`. The macro emits one [`TraceEvent`] for every
//! observable atom in chart execution: entries, exits, action invocations,
//! during start/cancel, timer arm/cancel/fire, queued emits, event delivery,
//! transitions, and termination.
//!
//! For a fixed `(chart, initial context, external event sequence,
//! elapsed-time inputs)`, the produced journal is byte-identical across runs
//! — that's the contract the verification harness checks against.
//!
//! All identifiers in the journal are stable u16 indices generated at macro
//! expansion time. Each chart also exposes:
//!
//! - `<Chart>::CHART_HASH: u64` — identifies the chart definition; mismatch
//!   on replay = different chart.
//! - `<Chart>::state_name(id)`, `<Chart>::action_name(id)`,
//!   `<Chart>::event_name(id)`, `<Chart>::timer_label(id)` — human-readable
//!   names for debugging.
//!
//! The journal does not record `during:` race outcomes from `run()` — race
//! winners depend on wall-clock timing and are not part of the deterministic
//! contract (see semantics doc). What you see in the journal is the *result*
//! of any race: the event the winning during emitted.

extern crate alloc;
pub(crate) use alloc::vec::Vec;

/// Owned journal returned by `take_journal()`. A `Vec<TraceEvent>` whose
/// concrete crate (`alloc::vec::Vec`) is hidden so callers don't need an
/// `extern crate alloc` of their own.
pub type Journal = alloc::vec::Vec<TraceEvent>;

/// Distinguishes which kind of slot an action was invoked in. Pure metadata
/// — the action body is the same regardless. Lets the journal be readable
/// without cross-referencing surrounding events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Entry action of a state.
    Entry,
    /// Exit action of a state.
    Exit,
    /// Handler action (action attached to a transition or standalone
    /// `action(...) => fn`).
    Handler,
}

/// Why a transition fired. Carried on `TransitionFired` so observers can
/// see *what triggered* the state change, not just that one happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionReason {
    /// External or queued event with the given index dispatched into a
    /// handler that fired this transition.
    Event { event: u16 },
    /// Timer on the given (state, timer) slot expired and its handler
    /// fired this transition.
    Timer { state: u16, timer: u16 },
    /// Driven from inside the runtime without an originating event or
    /// timer. Emitted for `default(...)` firings: after a state with a
    /// declared default is entered, the default fires as a transition
    /// with this reason. The default target may be any state in the
    /// chart — sibling, ancestor, anywhere.
    Internal,
}

/// One observable atom of chart execution. The macro emits these at every
/// hook point so the full sequence can be journaled and compared.
///
/// Variants carry stable u16 indices, never strings. Serialization (binary
/// or JSON) of these values is reproducible across rebuilds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TraceEvent {
    /// First event in any journal. Identifies the chart definition.
    Started { chart_hash: u64 },
    /// A state's entry began. The marker is emitted before any of the
    /// state's entry actions, timers, or durings have run. Followed by
    /// `ActionInvoked{Entry}`, `TimerArmed`, `DuringStarted`, then the
    /// matching `Entered` end marker.
    EnterBegan { state: u16 },
    /// A state's entry completed. Emitted after every entry action,
    /// timer arm, and during start has run for that state. Pairs with
    /// the prior `EnterBegan { state }`.
    Entered { state: u16 },
    /// A state's exit began. The marker is emitted before any of the
    /// state's during cancellations, timer cancellations, or exit
    /// actions have run. Followed by `DuringCancelled`, `TimerCancelled`,
    /// `ActionInvoked{Exit}`, then the matching `Exited` end marker.
    ExitBegan { state: u16 },
    /// A state's exit completed. Emitted after every during cancel,
    /// timer cancel, and exit action has run for that state. Pairs with
    /// the prior `ExitBegan { state }`.
    Exited { state: u16 },
    /// An action function was called.
    ActionInvoked {
        state: u16,
        action: u16,
        kind: ActionKind,
    },
    /// A `during:` activity became logically active for its owning state.
    /// Index is the state-local declaration order (0-based).
    DuringStarted { state: u16, during: u16 },
    /// A `during:` activity was cancelled because its owning state is
    /// being exited.
    DuringCancelled { state: u16, during: u16 },
    /// A transition is about to fire. `from` is the innermost active
    /// state at the moment the transition was triggered; `to` is the
    /// target; `reason` records what caused the transition (event,
    /// timer, or internal). Followed by the actual `ExitBegan`/`Exited`/
    /// `EnterBegan`/`Entered` events that implement the transition,
    /// then the matching `TransitionComplete` end marker.
    TransitionFired {
        from: Option<u16>,
        to: u16,
        reason: TransitionReason,
    },
    /// A transition completed. Emitted after the transition's exit and
    /// entry sequences are fully done. Pairs with the prior
    /// `TransitionFired { from, to, .. }`.
    TransitionComplete { from: Option<u16>, to: u16 },
    /// An event was popped from the internal queue and is about to be
    /// dispatched. Emitted before the handler search; followed by either
    /// `EventDelivered` (handler found and ran) or `EventDropped`
    /// (no handler on the active path), or `TerminateRequested` if the
    /// event matches the chart's terminate declaration.
    EventReceived { event: u16 },
    /// An event reached a state's handler. `handler_state` is the state
    /// whose handler ran (which may be an ancestor of the innermost
    /// active state if the event bubbled).
    EventDelivered { handler_state: u16, event: u16 },
    /// An event was dispatched but no state on the active path had a
    /// handler. Silently dropped per spec.
    EventDropped { event: u16 },
    /// `emit()` from inside an action successfully pushed an event onto
    /// the internal queue.
    EmitQueued { event: u16 },
    /// `emit()` from inside an action failed because the queue was full.
    /// The action chain continues; the runtime surfaces `QueueFull` at
    /// end of tick.
    EmitFailed { event: u16 },
    /// An event matched the chart's `terminate(...)` declaration and is
    /// about to drive shutdown. Followed by the bottom-up exit chain
    /// (Exited events) and a final `Terminated`.
    TerminateRequested { event: u16 },
    /// A timer was armed (set to start counting down) for a state.
    /// `ns` is the duration in nanoseconds.
    TimerArmed { state: u16, timer: u16, ns: u64 },
    /// A timer was cancelled because its owning state is being exited.
    TimerCancelled { state: u16, timer: u16 },
    /// A timer's countdown reached zero and its handler is about to run.
    TimerFired { state: u16, timer: u16 },
    /// The chart processed its `terminate` event and the run loop is
    /// done. Always the final event in the journal.
    Terminated,
}

// JournalSink lives in `__private` (in `lib.rs`) so the codegen can
// unconditionally refer to `::hsmc::__private::JournalSink` whether or
// not the `journal` feature is on.
