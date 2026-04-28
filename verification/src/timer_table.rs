//! Verified mirror of `hsmc::__private::TimerTable`.
//!
//! Tracks live timer countdowns: `(state, trigger, remaining_ns)`.
//!
//! The runtime uses a fixed-capacity `heapless::Vec<TimerEntry, N>`;
//! this mirror uses a plain `Vec<Entry>` so creusot-std's verified Vec
//! specs carry most of the proof burden. The const-generic capacity
//! is dropped — the runtime's overflow-on-full behavior is exercised
//! by `tests/correspondence_timer_table.rs` rather than proven here.
//!
//! Spec rules pinned (see `docs/002. hsmc-semantics-formal.md`):
//!
//! - **S5.UNIQ**: at most one entry per `(state, trigger)` pair.
//! - **S5.SAT_DEC**: `decrement(elapsed)` saturates at 0 (no underflow).
//! - **S5.LEN**: `len()` returns the view length.

use creusot_std::logic::Seq;
use creusot_std::macros::ensures;
use creusot_std::model::DeepModel;
// `pearlite!` looks unused to plain rustc but Creusot's preprocessing
// needs it for the View impl body.
#[allow(unused_imports)]
use creusot_std::macros::pearlite;

/// One live timer entry.
///
/// `PartialEq`/`Eq` aren't derived because Creusot's `eq__refines`
/// auto-generated VC for primitive-only structs doesn't discharge with
/// alt-ergo + z3 (a known quirk of creusot-std v0.9.0). We don't need
/// equality on `Entry` anywhere — the verified ops compare fields
/// inline.
#[derive(Clone, Copy, Debug, DeepModel)]
pub struct Entry {
    pub state: u16,
    pub trigger: u16,
    pub remaining_ns: u128,
}

/// Timer table.
///
/// Logical view is a `Seq<Entry>` in insertion order. Pop/cancel ops
/// preserve relative order of survivors.
pub struct TimerTable {
    pub entries: ::std::vec::Vec<Entry>,
}

impl TimerTable {
    /// Construct an empty table.
    #[ensures(result@ == Seq::empty())]
    pub fn new() -> Self {
        Self {
            entries: ::std::vec::Vec::new(),
        }
    }

    /// Number of live entries.
    #[ensures(result@ == self@.len())]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True iff there are no live entries.
    #[ensures(result == (self@.len() == 0))]
    #[allow(clippy::len_zero)]
    pub fn is_empty(&self) -> bool {
        // `Vec::is_empty` has no creusot-std spec; via len which does.
        self.entries.len() == 0
    }
}

impl Default for TimerTable {
    fn default() -> Self {
        Self::new()
    }
}

impl creusot_std::model::View for TimerTable {
    type ViewTy = Seq<Entry>;
    #[creusot_std::macros::logic(open)]
    fn view(self) -> Seq<Entry> {
        pearlite! { self.entries@ }
    }
}
