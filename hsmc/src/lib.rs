//! # hsmc
//!
//! Hierarchical state machines (statecharts) with a declarative proc-macro front-end.
//!
//! See the crate's spec (`docs/000. original-hsmc-spec.md`) for v0.1 semantics
//! and the top-level README for the v0.2 additions (`during:`, optional
//! `events:`, `dispatch()`, `STATE_CHART`).
//!
//! ## Macro invocation syntax
//!
//! The `statechart!` macro uses Rust's standard proc-macro call syntax —
//! the machine name goes *inside* the outer brace/parenthesis, because Rust
//! does not permit tokens between `!` and the macro delimiter:
//!
//! ```ignore
//! use hsmc::{statechart, Duration};
//!
//! # #[derive(Default)] struct MyContext;
//! # #[derive(Debug, Clone)] enum MyEvent { Go }
//! statechart! {
//!     MyMachine {
//!         context: MyContext;
//!         events:  MyEvent;
//!         default(Idle);
//!         state Idle { on(Go) => Busy; }
//!         state Busy { }
//!     }
//! }
//! ```
//!
//! ## `during:` activities (v0.2)
//!
//! Any state can declare one or more `during:` activities — async functions
//! that run while the state is on the active path and produce events:
//!
//! ```ignore
//! state Receiving {
//!     during: next_packet(lora, rx_buf);
//!     on(PacketRx { rssi: i16 }) => count_packet;
//!     on(StopRx) => Idle;
//! }
//!
//! async fn next_packet(lora: &mut LoRaDriver, buf: &mut [u8; 256]) -> Ev {
//!     lora.rx(buf).await.into()
//! }
//! ```
//!
//! The macro emits `next_packet(&mut ctx.lora, &mut ctx.rx_buf)` in the
//! generated run loop's `select` call, and Rust's native split-borrow
//! verifies at compile time that concurrent durings don't borrow
//! overlapping fields.
//!
//! ### Cancel-safety contract
//!
//! A `during:` future **will** be dropped at any `.await` point whenever a
//! handler fires, a timer expires, an external event arrives, or the state
//! transitions. Write durings as cancel-safe state machines: every `.await`
//! must be a clean resume point where dropping the future leaves borrowed
//! fields in a valid, re-enterable state. Prefer performing mutations
//! *after* awaited I/O completes rather than straddling await points.
//!
//! ## A note on `unexpected_cfgs` warnings
//!
//! The code emitted by `statechart!` contains `#[cfg(feature = "tokio")]` /
//! `#[cfg(feature = "embassy")]` gates so that feature-specific items (the
//! `Sender`, `run()`) compile only under the matching feature of the `hsmc`
//! crate. Rust 1.80+ `check-cfg` evaluates those attributes in the context
//! of the *calling* crate — so if your crate doesn't itself declare features
//! named `tokio` or `embassy`, rustc will emit `unexpected_cfgs` warnings at
//! every `statechart!` invocation. To silence them, add either:
//!
//! ```ignore
//! #![allow(unexpected_cfgs)]   // at the crate root, or
//! ```
//!
//! or, in your `Cargo.toml`:
//!
//! ```toml
//! [lints.rust]
//! unexpected_cfgs = { level = "allow", check-cfg = ['cfg(feature, values("tokio", "embassy"))'] }
//! ```
//!
//! This is a known limitation of proc-macros that emit cfg-gated code.
//!
//! ## Out of scope (spec §11)
//!
//! The following are explicitly NOT part of v0.1 and should not be added
//! without revisiting the spec: guard conditions, orthogonal / parallel
//! regions, history states (shallow or deep), deferred events, internal
//! transitions, state-local context / per-state data, visual rendering or
//! diagram export, runtime statechart modification, event priorities beyond
//! FIFO, and inter-machine communication primitives.

#![cfg_attr(not(any(test, feature = "tokio")), no_std)]

#[cfg(all(feature = "tokio", feature = "embassy"))]
compile_error!("hsmc features `tokio` and `embassy` are mutually exclusive");

pub use core::time::Duration;
pub use hsmc_macros::statechart;

mod error;
pub use error::HsmcError;

// ── Journal (deterministic execution trace) ─────────────────────────
//
// When `feature = "journal"` is enabled, the macro emits one
// [`TraceEvent`] per observable atom: entries, exits, action calls,
// during start/cancel, timer arm/cancel/fire, queued emits, event
// dispatch, transitions, termination. The events are appended to the
// machine's internal `JournalSink` so callers can compare against an
// expected sequence (see `tests/replay.rs`).
//
// When the feature is OFF, the codegen still emits `__chart_journal!`
// invocations at every hook site, but the macro expands to nothing
// (zero overhead) and `JournalSink` is a ZST so the field on the
// machine takes 0 bytes.

#[cfg(feature = "journal")]
mod journal;

#[cfg(feature = "journal")]
pub use journal::{ActionKind, Journal, TraceEvent};

/// Journal call dispatcher invoked by `statechart!`-generated code.
/// Not part of the public API.
///
/// Call form: `__chart_journal!(<sink-expr>, <TraceEvent expr>)`.
/// The first argument is a place expression for the sink (typically
/// `self.__journal`); the second is the full `TraceEvent` value to
/// push.
///
/// When the `journal` feature is off, the macro discards both arguments
/// and expands to nothing — the codegen can refer to `TraceEvent::*`
/// without the type having to exist.
#[cfg(feature = "journal")]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_journal {
    ($sink:expr, $event:expr) => {
        $sink.push($event);
    };
}

/// No-op fallback for when `feature = "journal"` is disabled.
#[cfg(not(feature = "journal"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_journal {
    ($($_:tt)*) => {};
}

// ── Auto-tracing dispatcher ──────────────────────────────────────────
//
// When a chart opts in via `trace;`, the proc-macro emits calls to
// `::hsmc::__chart_trace!(<kind> <chart-name>, <state-or-action>);` at
// every state entry, exit, action invocation, and transition. This
// crate-level macro picks the backend based on which `trace-*` cargo
// feature is enabled — exactly one of `trace-defmt` / `trace-log` /
// `trace-tracing`. With none enabled it expands to nothing.
//
// Multiple `trace-*` features enabled simultaneously produce a
// duplicate-definition compile error from rustc. Intentional.

/// Internal trace dispatcher invoked by `statechart!`-generated code.
/// Not part of the public API.
#[cfg(feature = "trace-defmt")]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_trace {
    (enter $chart:literal, $state:expr) => {
        ::defmt::info!("[{}] enter {}", $chart, $state);
    };
    (exit $chart:literal, $state:expr) => {
        ::defmt::info!("[{}] exit  {}", $chart, $state);
    };
    (action $chart:literal, $action:expr) => {
        ::defmt::trace!("[{}] action {}", $chart, $action);
    };
    (transition $chart:literal, $from:expr, $to:expr) => {
        ::defmt::info!("[{}] {} -> {}", $chart, $from, $to);
    };
    ($($_:tt)*) => {};
}

/// Internal trace dispatcher invoked by `statechart!`-generated code.
/// Not part of the public API.
#[cfg(feature = "trace-log")]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_trace {
    (enter $chart:literal, $state:expr) => {
        ::log::info!("[{}] enter {}", $chart, $state);
    };
    (exit $chart:literal, $state:expr) => {
        ::log::info!("[{}] exit  {}", $chart, $state);
    };
    (action $chart:literal, $action:expr) => {
        ::log::trace!("[{}] action {}", $chart, $action);
    };
    (transition $chart:literal, $from:expr, $to:expr) => {
        ::log::info!("[{}] {} -> {}", $chart, $from, $to);
    };
    ($($_:tt)*) => {};
}

/// Internal trace dispatcher invoked by `statechart!`-generated code.
/// Not part of the public API.
#[cfg(feature = "trace-tracing")]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_trace {
    (enter $chart:literal, $state:expr) => {
        ::tracing::info!(chart = $chart, state = $state, "enter");
    };
    (exit $chart:literal, $state:expr) => {
        ::tracing::info!(chart = $chart, state = $state, "exit");
    };
    (action $chart:literal, $action:expr) => {
        ::tracing::trace!(chart = $chart, action = $action, "action");
    };
    (transition $chart:literal, $from:expr, $to:expr) => {
        ::tracing::info!(chart = $chart, from = $from, to = $to, "transition");
    };
    ($($_:tt)*) => {};
}

/// No-op fallback when no `trace-*` backend feature is enabled. The
/// chart's `trace;` keyword still parses; calls just expand to nothing.
#[cfg(not(any(
    feature = "trace-defmt",
    feature = "trace-log",
    feature = "trace-tracing",
)))]
#[doc(hidden)]
#[macro_export]
macro_rules! __chart_trace {
    ($($_:tt)*) => {};
}

#[doc(hidden)]
pub mod __private {
    //! Implementation details used by code generated by the `statechart!` macro.
    //! Not part of the public API.
    pub use heapless;

    pub use crate::error::HsmcError;

    // ── JournalSink ──────────────────────────────────────────────────
    //
    // The codegen unconditionally declares a `__journal: JournalSink`
    // field on the generated machine and on the action context. When the
    // `journal` feature is on, JournalSink wraps a `Vec<TraceEvent>` and
    // the `__chart_journal!` macro pushes events into it. When off, it's
    // a ZST and the macro expands to nothing — zero overhead, no `alloc`
    // requirement.

    /// Journal sink storing the in-order trace of observable execution
    /// atoms when `feature = "journal"` is enabled. Zero-sized when
    /// disabled.
    #[cfg(feature = "journal")]
    #[derive(Default, Debug, Clone)]
    pub struct JournalSink {
        events: crate::journal::Vec<crate::TraceEvent>,
    }

    #[cfg(feature = "journal")]
    impl JournalSink {
        pub const fn new() -> Self {
            Self {
                events: crate::journal::Vec::new(),
            }
        }
        #[inline]
        pub fn push(&mut self, event: crate::TraceEvent) {
            self.events.push(event);
        }
        pub fn events(&self) -> &[crate::TraceEvent] {
            &self.events
        }
        pub fn take(&mut self) -> crate::journal::Vec<crate::TraceEvent> {
            core::mem::take(&mut self.events)
        }
        pub fn clear(&mut self) {
            self.events.clear();
        }
    }

    #[cfg(not(feature = "journal"))]
    #[derive(Default, Debug, Clone, Copy)]
    pub struct JournalSink;

    #[cfg(not(feature = "journal"))]
    impl JournalSink {
        pub const fn new() -> Self {
            Self
        }
    }

    /// A simple bounded event queue built on `heapless::Deque`.
    /// Capacity is a const generic.
    pub struct EventQueue<E, const N: usize> {
        inner: heapless::Deque<E, N>,
    }

    impl<E, const N: usize> EventQueue<E, N> {
        pub const fn new() -> Self {
            Self {
                inner: heapless::Deque::new(),
            }
        }
        pub fn push(&mut self, ev: E) -> Result<(), HsmcError> {
            self.inner.push_back(ev).map_err(|_| HsmcError::QueueFull)
        }
        pub fn pop(&mut self) -> Option<E> {
            self.inner.pop_front()
        }
        pub fn is_empty(&self) -> bool {
            self.inner.is_empty()
        }
        pub fn clear(&mut self) {
            while self.inner.pop_front().is_some() {}
        }
    }

    impl<E, const N: usize> Default for EventQueue<E, N> {
        fn default() -> Self {
            Self::new()
        }
    }

    /// A fixed-capacity timer table: `(state_index, trigger_index, remaining_nanos)`.
    pub struct TimerTable<const N: usize> {
        pub entries: heapless::Vec<TimerEntry, N>,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct TimerEntry {
        pub state: u16,
        pub trigger: u16,
        pub remaining_ns: u128,
    }

    impl<const N: usize> Default for TimerTable<N> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<const N: usize> TimerTable<N> {
        pub const fn new() -> Self {
            Self {
                entries: heapless::Vec::new(),
            }
        }
        pub fn start(&mut self, state: u16, trigger: u16, duration: core::time::Duration) {
            let ns = duration.as_nanos();
            // Replace existing entry for same (state, trigger) — resets on re-entry.
            for e in self.entries.iter_mut() {
                if e.state == state && e.trigger == trigger {
                    e.remaining_ns = ns;
                    return;
                }
            }
            let _ = self.entries.push(TimerEntry {
                state,
                trigger,
                remaining_ns: ns,
            });
        }
        pub fn cancel_state(&mut self, state: u16) {
            self.entries.retain(|e| e.state != state);
        }
        pub fn cancel_one(&mut self, state: u16, trigger: u16) {
            self.entries
                .retain(|e| !(e.state == state && e.trigger == trigger));
        }
        pub fn decrement(&mut self, elapsed: core::time::Duration) {
            let ns = elapsed.as_nanos();
            for e in self.entries.iter_mut() {
                e.remaining_ns = e.remaining_ns.saturating_sub(ns);
            }
        }
        /// Returns (state, trigger) of first expired timer, picking the innermost
        /// (deepest) state, ties broken by declaration order.
        pub fn pop_expired(&mut self, depth: &[u8]) -> Option<(u16, u16)> {
            let mut best: Option<usize> = None;
            for (i, e) in self.entries.iter().enumerate() {
                if e.remaining_ns == 0 {
                    match best {
                        None => best = Some(i),
                        Some(bi) => {
                            let cur = depth[e.state as usize];
                            let prev = depth[self.entries[bi].state as usize];
                            if cur > prev {
                                best = Some(i);
                            }
                        }
                    }
                }
            }
            if let Some(i) = best {
                let e = self.entries.swap_remove(i);
                Some((e.state, e.trigger))
            } else {
                None
            }
        }
        pub fn min_remaining(&self) -> Option<core::time::Duration> {
            self.entries.iter().map(|e| e.remaining_ns).min().map(|ns| {
                let secs = (ns / 1_000_000_000) as u64;
                let rem = (ns % 1_000_000_000) as u32;
                core::time::Duration::new(secs, rem)
            })
        }
    }

    /// Convert an f64 seconds value to a Duration (runtime, non-const).
    pub fn duration_from_secs_f64(secs: f64) -> core::time::Duration {
        core::time::Duration::from_nanos((secs * 1_000_000_000.0) as u64)
    }

    /// Type-erased push interface over the event queue, so the generated
    /// `ActionContext` does not need to be parameterized by the queue capacity.
    pub trait QueuePush<E> {
        fn push(&mut self, ev: E) -> Result<(), HsmcError>;
    }

    impl<E, const N: usize> QueuePush<E> for heapless::Deque<E, N> {
        fn push(&mut self, ev: E) -> Result<(), HsmcError> {
            self.push_back(ev).map_err(|_| HsmcError::QueueFull)
        }
    }

    /// Proxy used by generated code to route `emit()` pushes through the
    /// internal deque while recording overflow on the machine. When the deque
    /// rejects a push, the `overflow` flag is set and further pushes within
    /// the same tick short-circuit to `Err(QueueFull)` without reaching the
    /// deque. `run()` inspects this flag each iteration and surfaces the
    /// failure rather than letting it be silently swallowed by action code.
    pub struct EmitProxy<'a, E, const N: usize> {
        pub queue: &'a mut heapless::Deque<E, N>,
        pub overflow: &'a mut bool,
    }

    impl<'a, E, const N: usize> QueuePush<E> for EmitProxy<'a, E, N> {
        fn push(&mut self, ev: E) -> Result<(), HsmcError> {
            if *self.overflow {
                return Err(HsmcError::QueueFull);
            }
            match self.queue.push_back(ev) {
                Ok(()) => Ok(()),
                Err(_) => {
                    *self.overflow = true;
                    Err(HsmcError::QueueFull)
                }
            }
        }
    }
}

#[cfg(test)]
mod private_internals {
    //! Direct unit tests on `__private` internals. The behavior suite covers
    //! these through generated code, but mutation testing needs tight, fast
    //! tests that pin the exact semantics each helper is supposed to uphold.
    use super::__private::*;
    use crate::HsmcError;
    use core::time::Duration;

    // ───── EventQueue ─────

    // Kills: push -> Ok(()) (must actually enqueue, not swallow)
    #[test]
    fn event_queue_push_and_pop() {
        let mut q: EventQueue<u32, 2> = EventQueue::new();
        assert!(q.push(10).is_ok());
        assert!(q.push(20).is_ok());
        assert_eq!(q.pop(), Some(10));
        assert_eq!(q.pop(), Some(20));
        assert_eq!(q.pop(), None);
    }

    // Kills: push -> Ok(()) for the overflow branch (QueueFull must be surfaced)
    #[test]
    fn event_queue_push_overflow_returns_queue_full() {
        let mut q: EventQueue<u32, 2> = EventQueue::new();
        assert!(q.push(1).is_ok());
        assert!(q.push(2).is_ok());
        assert_eq!(q.push(3), Err(HsmcError::QueueFull));
    }

    // Kills: pop -> None (must drain in FIFO order)
    #[test]
    fn event_queue_pop_is_fifo() {
        let mut q: EventQueue<u32, 4> = EventQueue::new();
        q.push(1).unwrap();
        q.push(2).unwrap();
        q.push(3).unwrap();
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
    }

    // Kills: is_empty -> true | is_empty -> false
    #[test]
    fn event_queue_is_empty_tracks_contents() {
        let mut q: EventQueue<u32, 2> = EventQueue::new();
        assert!(q.is_empty()); // kills is_empty -> false
        q.push(1).unwrap();
        assert!(!q.is_empty()); // kills is_empty -> true
        q.pop();
        assert!(q.is_empty());
    }

    // Kills: clear with ()
    #[test]
    fn event_queue_clear_drains() {
        let mut q: EventQueue<u32, 4> = EventQueue::new();
        q.push(1).unwrap();
        q.push(2).unwrap();
        q.push(3).unwrap();
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.pop(), None);
    }

    // ───── QueuePush for heapless::Deque ─────

    // Kills: <impl QueuePush for heapless::Deque>::push -> Ok(())
    #[test]
    fn deque_queue_push_surfaces_overflow() {
        let mut d: heapless::Deque<u32, 2> = heapless::Deque::new();
        let q: &mut dyn QueuePush<u32> = &mut d;
        assert!(q.push(1).is_ok());
        assert!(q.push(2).is_ok());
        assert_eq!(q.push(3), Err(HsmcError::QueueFull));
    }

    // ───── TimerTable::start ─────

    // Kills L173 && -> ||, L173 first == -> !=, L173 second == -> !=
    //
    // Three timers chosen so each mutation lands on a different outcome:
    //   (1,10,100) (1,20,200) (2,10,300); then re-start (1,10,500) to reset.
    // Normal: three distinct entries; (1,10) shows 500ns.
    //   && -> ||: re-start (1,10,500) matches (1,20) on state-1, rewrites it.
    //   state == -> !=: the second call (1,20) overwrites the first (1,10).
    //   trigger == -> !=: the re-start (1,10,500) doesn't find (1,10); pushes a 4th.
    #[test]
    fn timer_start_matches_state_and_trigger_exactly() {
        let mut t: TimerTable<8> = TimerTable::new();
        t.start(1, 10, Duration::from_nanos(100));
        t.start(1, 20, Duration::from_nanos(200));
        t.start(2, 10, Duration::from_nanos(300));
        assert_eq!(t.entries.len(), 3);

        t.start(1, 10, Duration::from_nanos(500));
        assert_eq!(t.entries.len(), 3); // reset, not appended

        let find = |s, trg| {
            t.entries
                .iter()
                .find(|e| e.state == s && e.trigger == trg)
                .map(|e| e.remaining_ns)
        };
        assert_eq!(find(1, 10), Some(500));
        assert_eq!(find(1, 20), Some(200));
        assert_eq!(find(2, 10), Some(300));
    }

    // Kills: start with () (must actually push an entry)
    #[test]
    fn timer_start_pushes_new_entry() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(7, 42, Duration::from_nanos(999));
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].state, 7);
        assert_eq!(t.entries[0].trigger, 42);
        assert_eq!(t.entries[0].remaining_ns, 999);
    }

    // ───── TimerTable::cancel_state ─────

    // Kills: cancel_state with (), != -> ==
    //
    // Start three timers, cancel state=1; expect only state=2 survives.
    //   cancel_state with (): all three survive.
    //   != -> ==: retains only state=1 entries, drops state=2. Inverted.
    #[test]
    fn timer_cancel_state_removes_matching_only() {
        let mut t: TimerTable<8> = TimerTable::new();
        t.start(1, 10, Duration::from_nanos(100));
        t.start(1, 20, Duration::from_nanos(200));
        t.start(2, 10, Duration::from_nanos(300));
        t.cancel_state(1);
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].state, 2);
        assert_eq!(t.entries[0].trigger, 10);
    }

    // ───── TimerTable::cancel_one ─────

    // Kills: cancel_one with (), delete !, && -> ||, both == -> !=
    //
    // Start (1,10), (1,20), (2,10). Cancel (1,10). Expect {(1,20),(2,10)}.
    //   body (): nothing removed.
    //   delete !: keeps only (1,10).
    //   && -> ||: retains `!(state==s || trig==t)`, drops everything.
    //   state == -> !=: (1,10) survives, (1,20) dropped.
    //   trigger == -> !=: (1,10) survives, (1,20) dropped.
    #[test]
    fn timer_cancel_one_removes_exact_pair() {
        let mut t: TimerTable<8> = TimerTable::new();
        t.start(1, 10, Duration::from_nanos(100));
        t.start(1, 20, Duration::from_nanos(200));
        t.start(2, 10, Duration::from_nanos(300));
        t.cancel_one(1, 10);

        let has = |s, trg| t.entries.iter().any(|e| e.state == s && e.trigger == trg);
        assert_eq!(t.entries.len(), 2);
        assert!(!has(1, 10), "(1,10) must be cancelled");
        assert!(has(1, 20), "(1,20) must remain");
        assert!(has(2, 10), "(2,10) must remain");
    }

    // ───── TimerTable::decrement ─────

    // Kills: decrement with ()
    #[test]
    fn timer_decrement_reduces_remaining() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(1, 1, Duration::from_nanos(500));
        t.decrement(Duration::from_nanos(200));
        assert_eq!(t.entries[0].remaining_ns, 300);
    }

    // ───── TimerTable::pop_expired ─────

    // Kills body mutations: None, Some((0,0)/(0,1)/(1,0)/(1,1))
    // Uses state/trigger distinct from 0/1 so no fixed return matches.
    // Also kills the "entry is removed" invariant.
    #[test]
    fn timer_pop_expired_returns_exact_pair_and_removes() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(5, 7, Duration::from_nanos(0));
        let depth = [0u8, 0, 0, 0, 0, 1, 0, 1];
        assert_eq!(t.pop_expired(&depth), Some((5, 7)));
        assert_eq!(t.entries.len(), 0);
    }

    // Kills L202: `remaining_ns == 0` -> `!= 0`
    // A still-running timer must not be returned.
    #[test]
    fn timer_pop_expired_ignores_nonzero_remaining() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(5, 7, Duration::from_nanos(100));
        let depth = [0u8, 0, 0, 0, 0, 1, 0, 1];
        assert_eq!(t.pop_expired(&depth), None);
        assert_eq!(t.entries.len(), 1); // not removed
    }

    // Kills L208: depth `>` -> `==` or `<`
    // Two expired entries at different depths; deepest must win.
    #[test]
    fn timer_pop_expired_picks_deepest() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(1, 100, Duration::from_nanos(0)); // shallow
        t.start(2, 200, Duration::from_nanos(0)); // deeper
        let depth = [0u8, 1, 3, 0];
        assert_eq!(t.pop_expired(&depth), Some((2, 200)));
    }

    // Kills L208: `>` -> `>=`
    // Two expired entries at *equal* depth; first-declared must win.
    // With `>=`, the later entry replaces the earlier on tie.
    #[test]
    fn timer_pop_expired_breaks_ties_by_declaration_order() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(1, 100, Duration::from_nanos(0));
        t.start(2, 200, Duration::from_nanos(0));
        let depth = [0u8, 2, 2, 0]; // both at depth 2
        assert_eq!(t.pop_expired(&depth), Some((1, 100)));
    }

    // ───── TimerTable::min_remaining ─────

    // Kills L223 body None + body Some(default)
    // Kills L224 / -> * and / -> %
    // Kills L225 % -> + and % -> /
    //
    // 1_500_000_000 ns = 1.5s. Every mutation yields a clearly wrong duration:
    //   None: returns None, not Some(1.5s).
    //   Some(default): returns Duration::ZERO.
    //   / -> *: secs = ns * 1e9 overflows (or caught by `as u64`); not 1.
    //   / -> %: secs = ns % 1e9 = 5e8; not 1.
    //   % -> +: rem = ns + 1e9 overflows u32; not 5e8.
    //   % -> /: rem = ns / 1e9 = 1; not 5e8.
    #[test]
    fn timer_min_remaining_exact_duration() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(1, 1, Duration::from_nanos(1_500_000_000));
        assert_eq!(t.min_remaining(), Some(Duration::new(1, 500_000_000)));
    }

    // Kills: min_remaining -> None (always). Explicit contrast to the empty case.
    #[test]
    fn timer_min_remaining_none_when_empty() {
        let t: TimerTable<4> = TimerTable::new();
        assert_eq!(t.min_remaining(), None);
    }

    // Picks the smallest across multiple entries.
    #[test]
    fn timer_min_remaining_picks_smallest() {
        let mut t: TimerTable<4> = TimerTable::new();
        t.start(1, 1, Duration::from_nanos(9_999));
        t.start(2, 2, Duration::from_nanos(100));
        t.start(3, 3, Duration::from_nanos(5_000));
        assert_eq!(t.min_remaining(), Some(Duration::from_nanos(100)));
    }

    // ───── duration_from_secs_f64 ─────

    // Kills body -> Default, * -> +, * -> /
    //
    // 1.5s -> 1_500_000_000ns exactly.
    //   body -> Default: returns ZERO.
    //   * -> +: secs + 1e9 = 1_000_000_001.5 -> 1_000_000_001 ns.
    //   * -> /: secs / 1e9 = 1.5e-9 -> 0 ns.
    #[test]
    fn duration_from_secs_f64_multiplies_correctly() {
        assert_eq!(
            duration_from_secs_f64(1.5),
            Duration::from_nanos(1_500_000_000)
        );
        assert_eq!(
            duration_from_secs_f64(0.25),
            Duration::from_nanos(250_000_000)
        );
    }
}
