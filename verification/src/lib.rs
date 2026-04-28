//! Deductive proofs for the `hsmc` runtime data structures.
//!
//! This crate hosts Creusot/Pearlite contracts on simplified mirrors of
//! `hsmc::__private::EventQueue` and `hsmc::__private::TimerTable`. The
//! mirrors are byte-for-byte equivalent to the runtime types' observable
//! behavior; the difference is that the mirrors carry full
//! `#[requires] / #[ensures]` annotations, so a Creusot run discharges
//! VCs (verification conditions) for them.
//!
//! ## Why mirror, not annotate hsmc directly?
//!
//! Two reasons:
//!
//! 1. **Cleanliness.** `hsmc` itself stays free of `creusot-contracts`
//!    — production users don't have to think about a verifier dep.
//! 2. **Tractability.** The runtime types are layered on `heapless`
//!    types whose Pearlite models would have to be written first.
//!    A simplified mirror with `&mut [_; CAP]` storage avoids the
//!    indirection and makes the proofs readable.
//!
//! The correspondence is enforced by `tests/correspondence.rs`, which
//! drives both the mirror AND the real `hsmc` runtime type with the same
//! operation sequences and asserts equal observable results. If the
//! mirror drifts away from the runtime, the correspondence test fails
//! before the proofs become wrong.
//!
//! ## Status
//!
//! See `INVARIANTS.md` for the spec-rule → proof mapping. Initial focus
//! (in priority order):
//!
//! 1. `EventQueue` — FIFO; bounded; pop-after-push returns the pushed
//!    value. (S6 in the semantics doc.)
//! 2. `TimerTable::start` — at most one entry per `(state, trigger)`
//!    pair; capacity respected. (S5.1)
//! 3. `TimerTable::pop_expired` — deepest expired entry first; ties
//!    broken by insertion index; entry removed; `None` iff none expired.
//!    (S5.2)
//! 4. (later) `transition()` path validity — codegen-emitted; needs
//!    macro changes to thread Creusot attributes through.
//! 5. (later) `step()` end-to-end determinism.

// Verification is a dev-time concern — no need for no_std here. We
// use `std::vec::Vec` (via creusot-std's specs) directly.

pub mod event_queue;
pub mod timer_table;
