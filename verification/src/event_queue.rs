//! Verified mirror of `hsmc::__private::EventQueue`.
//!
//! FIFO queue. The runtime uses a fixed-capacity ring buffer
//! (`heapless::Deque<E, N>`); this mirror uses a `Vec<E>` for which
//! creusot-std already ships full specs — push/pop/remove/clear all
//! have proven contracts upstream. Our wrapper just exposes the FIFO
//! discipline on top.
//!
//! Spec rules pinned (see `docs/002. hsmc-semantics-formal.md`,
//! section "emit() — sending events from inside an action"):
//!
//! - **S6.FIFO**: push appends to the back; pop returns view[0].
//! - **S6.EMPTY**: pop returns `None` iff empty.
//! - **S6.CLEAR**: clear empties the view.
//! - **S6.LEN**: len matches view length.
//!
//! Boundedness (the runtime's `Err(QueueFull)` path) is intentionally
//! NOT modeled here — it's a property of the ring buffer at the
//! runtime level and is verified by the unit tests in
//! `hsmc/src/lib.rs::private_internals` and the correspondence tests
//! in `tests/correspondence_event_queue.rs`. Creusot proves the FIFO
//! discipline; the unit tests pin the cap behavior.

use creusot_std::logic::Seq;
use creusot_std::macros::ensures;
// `pearlite!` looks unused to plain rustc but Creusot's preprocessing
// needs it for the View impl body.
#[allow(unused_imports)]
use creusot_std::macros::pearlite;

/// FIFO event queue. Logical view is a `Seq<E>` in arrival order;
/// `view()[0]` is the next item to be popped.
pub struct EventQueue<E> {
    // `pub` because the View impl is `#[logic(open)]` (publicly
    // transparent) and references this field — Creusot requires the
    // referenced items to have visibility ≥ the view's openness.
    pub items: ::std::vec::Vec<E>,
}

impl<E> EventQueue<E> {
    /// Construct an empty queue.
    #[ensures(result@ == Seq::empty())]
    pub fn new() -> Self {
        Self {
            items: ::std::vec::Vec::new(),
        }
    }

    /// Push to the back.
    ///
    /// **S6.FIFO**: the item lands at the back of the view.
    #[ensures((^self)@ == self@.push_back(ev))]
    pub fn push(&mut self, ev: E) {
        self.items.push(ev);
    }

    /// Pop from the front. Returns `None` iff empty.
    ///
    /// - **S6.EMPTY**: `None` iff `self@.len() == 0`.
    /// - **S6.FIFO**: returns `Some(view[0])` and drops view[0].
    #[ensures(self@.len() == 0 ==> result == None && (^self)@ == self@)]
    #[ensures(self@.len() > 0 ==> result == Some(self@[0]) && (^self)@ == self@.subsequence(1, self@.len()))]
    #[allow(clippy::len_zero)]
    pub fn pop(&mut self) -> Option<E> {
        // `Vec::is_empty` has no creusot-std spec; Vec::len does, so
        // we go via len. Clippy's len_zero lint disagrees with this
        // workaround, hence the allow.
        if self.items.len() == 0 {
            None
        } else {
            Some(self.items.remove(0))
        }
    }

    /// True iff the queue contains no elements.
    #[ensures(result == (self@.len() == 0))]
    #[allow(clippy::len_zero)]
    pub fn is_empty(&self) -> bool {
        self.items.len() == 0
    }

    /// Drop all elements.
    ///
    /// **S6.CLEAR**: post-condition is the empty view.
    #[ensures((^self)@ == Seq::empty())]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Number of live elements.
    #[ensures(result@ == self@.len())]
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl<E> Default for EventQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}

// View is the view of the inner Vec. `#[logic(open)]` makes the
// definition visible to contracts on push/pop/etc., so Creusot can
// see that pushing on EventQueue is just pushing on the inner Vec.
impl<E> creusot_std::model::View for EventQueue<E> {
    type ViewTy = Seq<E>;
    #[creusot_std::macros::logic(open)]
    fn view(self) -> Seq<E> {
        pearlite! { self.items@ }
    }
}
