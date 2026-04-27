//! Verified mirror of `hsmc::__private::EventQueue`.
//!
//! Bounded FIFO queue. The runtime uses `heapless::Deque<E, N>` under the
//! hood (a fixed-capacity ring buffer); this mirror uses an array with
//! head index + length, which Creusot can reason about directly via the
//! `Seq<E>` view returned by [`View::view`].
//!
//! Spec rules pinned (see `docs/002. hsmc-semantics-formal.md`):
//!
//! - `push()` returns `Err(QueueFull)` exactly when at capacity.
//! - `push()` on a non-full queue grows the view by appending at the back.
//! - `pop()` returns `None` exactly when empty; otherwise returns the
//!   front element and truncates the view from the front.
//! - `is_empty()` ⇔ `view().len() == 0`.
//! - `clear()` makes the view empty.
//!
//! Correspondence with the real runtime type is enforced by
//! `tests/correspondence.rs`.

// Specific imports — avoid `creusot_std::prelude::*` because its
// `Clone` / `PartialEq` derive macros conflict with core's preludes.
//
// `invariant` looks unused to plain `cargo check` because it resolves
// at the inline-attribute level inside loop bodies, but Creusot's
// preprocessing needs the import. Allow the false warning.
#[allow(unused_imports)]
use creusot_std::macros::invariant;
use creusot_std::logic::Seq;
use creusot_std::model::{DeepModel, View};
use creusot_std::macros::{ensures, logic, requires, trusted};

/// Error returned by `push()` when the queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, DeepModel)]
pub struct QueueFull;

/// Bounded FIFO event queue.
///
/// Logical view is a `Seq<E>` of length `0..=CAP`, with `view()[0]` the
/// next element to be popped.
pub struct EventQueue<E, const CAP: usize> {
    items: [Option<E>; CAP],
    head: usize,
    len: usize,
}

impl<E, const CAP: usize> View for EventQueue<E, CAP> {
    type ViewTy = Seq<E>;
    // Logical model treated as opaque — defined extensionally by the
    // contracts on push/pop. `#[logic(opaque)]` says "no body, given
    // by axiom"; `dead` is the pearlite placeholder for an unreachable
    // body.
    #[trusted]
    #[logic(opaque)]
    fn view(self) -> Seq<E> {
        dead
    }
}

impl<E: Copy, const CAP: usize> Default for EventQueue<E, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Copy, const CAP: usize> EventQueue<E, CAP> {
    /// Construct an empty queue.
    #[ensures(result@.len() == 0)]
    pub const fn new() -> Self {
        Self {
            items: [None; CAP],
            head: 0,
            len: 0,
        }
    }

    /// Push to the back. Fails iff the queue is full.
    ///
    /// - **S6.BOUND**: returns `Err(QueueFull)` exactly when at capacity.
    /// - **S6.FIFO**: on success, the item lands at the back of the view.
    #[requires(self@.len() <= CAP@)]
    #[ensures(self@.len() == CAP@ ==> result == Err(QueueFull))]
    #[ensures(self@.len() < CAP@ ==> result == Ok(()))]
    #[ensures(result == Ok(()) ==> (^self)@ == self@.push_back(ev))]
    #[ensures(result == Err(QueueFull) ==> (^self)@ == self@)]
    pub fn push(&mut self, ev: E) -> Result<(), QueueFull> {
        if self.len == CAP {
            return Err(QueueFull);
        }
        let tail = (self.head + self.len) % CAP;
        self.items[tail] = Some(ev);
        self.len += 1;
        Ok(())
    }

    /// Pop from the front. Returns `None` iff the queue is empty.
    ///
    /// - **S6.EMPTY**: `None` iff `is_empty()`.
    /// - **S6.FIFO**: returns the oldest pushed element.
    #[requires(self@.len() <= CAP@)]
    #[ensures(self@.len() == 0 ==> result == None)]
    #[ensures(self@.len() == 0 ==> (^self)@ == self@)]
    #[ensures(self@.len() > 0 ==> result == Some(self@[0]))]
    #[ensures(self@.len() > 0 ==> (^self)@ == self@.subsequence(1, self@.len()))]
    pub fn pop(&mut self) -> Option<E> {
        if self.len == 0 {
            return None;
        }
        let item = self.items[self.head];
        self.items[self.head] = None;
        self.head = (self.head + 1) % CAP;
        self.len -= 1;
        item
    }

    /// True iff the queue contains no elements.
    #[ensures(result == (self@.len() == 0))]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Drop all elements.
    #[ensures((^self)@.len() == 0)]
    pub fn clear(&mut self) {
        #[invariant(self.len@ <= CAP@)]
        while self.len > 0 {
            self.items[self.head] = None;
            self.head = (self.head + 1) % CAP;
            self.len -= 1;
        }
        self.head = 0;
    }

    /// Number of live elements.
    #[ensures(result@ == self@.len())]
    pub fn len(&self) -> usize {
        self.len
    }
}
