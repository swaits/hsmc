//! Verified mirror of `hsmc::__private::TimerTable`.
//!
//! Tracks live timer countdowns: `(state, trigger, remaining_ns)`.
//! Operations: `start`, `cancel_state`, `cancel_one`, `decrement`,
//! `pop_expired`, `min_remaining`.
//!
//! Spec rules pinned (see `docs/002. hsmc-semantics-formal.md`):
//!
//! - **S5.UNIQ**: at most one entry per `(state, trigger)` pair. `start`
//!   replaces in place.
//! - **S5.CAP**: total entries always `≤ CAP`.
//! - **S5.POP_DEEPEST**: `pop_expired` returns the deepest expired entry
//!   (greatest `depth[state]`); ties broken by insertion order.
//! - **S5.POP_REMOVE**: returned entry is removed from the table.
//! - **S5.POP_NONE**: `None` iff no entry has `remaining_ns == 0`.
//! - **S5.SAT_DEC**: `decrement(elapsed)` saturates at 0 (no underflow).

// Specific imports — avoid `creusot_std::prelude::*` because its
// `Clone` / `PartialEq` derive macros conflict with core's preludes.
//
// `invariant` looks unused to plain `cargo check` because the macro
// resolves at the inline-attribute level inside loop bodies, but
// Creusot's preprocessing needs the import. Allow the false warning.
#[allow(unused_imports)]
use creusot_std::macros::invariant;
use creusot_std::logic::Seq;
use creusot_std::model::{DeepModel, View};
use creusot_std::macros::{ensures, logic, requires, trusted};

/// One live timer entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, DeepModel)]
pub struct Entry {
    pub state: u16,
    pub trigger: u16,
    pub remaining_ns: u128,
}

/// Bounded timer table.
///
/// Logical view is a `Seq<Entry>` of length `0..=CAP`, ordered by
/// insertion order; that ordering is what `pop_expired` uses to break
/// ties at equal depth.
pub struct TimerTable<const CAP: usize> {
    entries: [Option<Entry>; CAP],
    len: usize,
}

impl<const CAP: usize> View for TimerTable<CAP> {
    type ViewTy = Seq<Entry>;
    // Opaque logical model — extensionally defined by per-operation
    // contracts.
    #[trusted]
    #[logic(opaque)]
    fn view(self) -> Seq<Entry> {
        dead
    }
}

impl<const CAP: usize> Default for TimerTable<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> TimerTable<CAP> {
    /// Construct an empty table.
    #[ensures(result@.len() == 0)]
    pub const fn new() -> Self {
        Self {
            entries: [None; CAP],
            len: 0,
        }
    }

    /// Number of live entries.
    #[ensures(result@ == self@.len())]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True iff there are no live entries.
    #[ensures(result == (self@.len() == 0))]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Start (or reset) the timer for `(state, trigger)` to `duration_ns`.
    ///
    /// - **S5.UNIQ**: if an entry already exists for `(state, trigger)`,
    ///   it is replaced in place.
    /// - **S5.CAP**: post-condition view length is `≤ CAP`.
    #[requires(self@.len() <= CAP@)]
    #[ensures((^self)@.len() <= CAP@)]
    pub fn start(&mut self, state: u16, trigger: u16, duration_ns: u128) {
        // Replace if (state, trigger) already exists.
        let mut i = 0;
        #[invariant(i@ <= self.len@)]
        while i < self.len {
            if let Some(e) = self.entries[i] {
                if e.state == state && e.trigger == trigger {
                    self.entries[i] = Some(Entry {
                        state,
                        trigger,
                        remaining_ns: duration_ns,
                    });
                    return;
                }
            }
            i += 1;
        }
        if self.len < CAP {
            self.entries[self.len] = Some(Entry {
                state,
                trigger,
                remaining_ns: duration_ns,
            });
            self.len += 1;
        }
    }

    /// Decrement all entries by `elapsed_ns`, saturating at 0.
    ///
    /// - **S5.SAT_DEC**: no underflow; entries reaching 0 stay at 0.
    #[requires(self@.len() <= CAP@)]
    #[ensures((^self)@.len() == self@.len())]
    pub fn decrement(&mut self, elapsed_ns: u128) {
        let mut i = 0;
        #[invariant(i@ <= self.len@)]
        #[invariant(self.len@ <= CAP@)]
        while i < self.len {
            if let Some(e) = self.entries[i] {
                let new_remaining = e.remaining_ns.saturating_sub(elapsed_ns);
                self.entries[i] = Some(Entry {
                    state: e.state,
                    trigger: e.trigger,
                    remaining_ns: new_remaining,
                });
            }
            i += 1;
        }
    }

    /// Cancel every entry whose `state` matches.
    ///
    /// Used when a state is exited — all of its timers go away at once.
    #[requires(self@.len() <= CAP@)]
    #[ensures((^self)@.len() <= self@.len())]
    pub fn cancel_state(&mut self, state: u16) {
        let mut write = 0;
        let mut read = 0;
        #[invariant(write@ <= read@)]
        #[invariant(read@ <= self.len@)]
        #[invariant(self.len@ <= CAP@)]
        while read < self.len {
            if let Some(e) = self.entries[read] {
                if e.state != state {
                    self.entries[write] = Some(e);
                    write += 1;
                }
            }
            read += 1;
        }
        // Clear the tail.
        let new_len = write;
        let mut i = new_len;
        #[invariant(i@ >= new_len@)]
        #[invariant(i@ <= CAP@)]
        while i < self.len {
            self.entries[i] = None;
            i += 1;
        }
        self.len = new_len;
    }

    /// Cancel a single specific `(state, trigger)` entry.
    #[requires(self@.len() <= CAP@)]
    #[ensures((^self)@.len() <= self@.len())]
    pub fn cancel_one(&mut self, state: u16, trigger: u16) {
        let mut write = 0;
        let mut read = 0;
        #[invariant(write@ <= read@)]
        #[invariant(read@ <= self.len@)]
        #[invariant(self.len@ <= CAP@)]
        while read < self.len {
            if let Some(e) = self.entries[read] {
                if !(e.state == state && e.trigger == trigger) {
                    self.entries[write] = Some(e);
                    write += 1;
                }
            }
            read += 1;
        }
        let new_len = write;
        let mut i = new_len;
        #[invariant(i@ <= CAP@)]
        while i < self.len {
            self.entries[i] = None;
            i += 1;
        }
        self.len = new_len;
    }

    /// Pop the deepest expired entry, ties broken by insertion order.
    ///
    /// - **S5.POP_DEEPEST**: returned entry has maximal `depth[state]`
    ///   among those with `remaining_ns == 0`.
    /// - **S5.POP_REMOVE**: removed from the table.
    /// - **S5.POP_NONE**: `None` iff no entry has `remaining_ns == 0`.
    #[requires(self@.len() <= CAP@)]
    pub fn pop_expired(&mut self, depth: &[u8]) -> Option<(u16, u16)> {
        let mut best: Option<usize> = None;
        let mut i = 0;
        #[invariant(i@ <= self.len@)]
        while i < self.len {
            if let Some(e) = self.entries[i] {
                if e.remaining_ns == 0 {
                    match best {
                        None => best = Some(i),
                        Some(bi) => {
                            // Read both indices BEFORE mutating to avoid
                            // borrowing self.entries twice.
                            let cur_state = e.state as usize;
                            let prev_state =
                                self.entries[bi].map(|p| p.state as usize).unwrap_or(0);
                            if cur_state < depth.len()
                                && prev_state < depth.len()
                                && depth[cur_state] > depth[prev_state]
                            {
                                best = Some(i);
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        if let Some(idx) = best {
            let popped = self.entries[idx].take()?;
            // Shift the tail down so insertion order is preserved.
            let mut k = idx;
            #[invariant(k@ < self.len@ || k@ == self.len@)]
            while k + 1 < self.len {
                self.entries[k] = self.entries[k + 1].take();
                k += 1;
            }
            self.len -= 1;
            Some((popped.state, popped.trigger))
        } else {
            None
        }
    }

    /// Smallest `remaining_ns` across live entries, or `None` when empty.
    #[ensures(self@.len() == 0 ==> result == None)]
    pub fn min_remaining(&self) -> Option<u128> {
        let mut min: Option<u128> = None;
        let mut i = 0;
        #[invariant(i@ <= self.len@)]
        while i < self.len {
            if let Some(e) = self.entries[i] {
                min = Some(match min {
                    None => e.remaining_ns,
                    Some(m) if e.remaining_ns < m => e.remaining_ns,
                    Some(m) => m,
                });
            }
            i += 1;
        }
        min
    }
}
