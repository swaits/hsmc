//! Correspondence test: the verified [`EventQueue`] mirror behaves
//! identically to the real runtime [`hsmc::__private::EventQueue`] under
//! the same sequence of operations.
//!
//! Why this test exists: the verified mirror lives in this crate with
//! Pearlite contracts. The real runtime queue lives in hsmc and ships
//! to users. If the two ever drift apart, the proofs become wrong even
//! though Creusot still passes — we'd be proving the wrong thing.
//! This test is the seam that keeps them locked together.

use hsmc::__private::EventQueue as RealQueue;
use hsmc_verification::event_queue::{EventQueue as MirrorQueue, QueueFull};

/// A small DSL of operations exercised against both queues.
#[derive(Debug, Clone, Copy)]
enum Op {
    Push(u32),
    Pop,
    Clear,
    IsEmpty,
}

/// Drive both queues with the same op sequence and assert observable
/// behavior (return values + length) match at every step.
fn assert_corresponds<const CAP: usize>(ops: &[Op]) {
    let mut real: RealQueue<u32, CAP> = RealQueue::new();
    let mut mirror: MirrorQueue<u32, CAP> = MirrorQueue::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Push(v) => {
                let r = real.push(*v);
                let m = mirror.push(*v);
                let r_full = matches!(r, Err(hsmc::HsmcError::QueueFull));
                let m_full = matches!(m, Err(QueueFull));
                assert_eq!(
                    r_full, m_full,
                    "step {i} Push({v}): real-full {r_full}, mirror-full {m_full}"
                );
            }
            Op::Pop => {
                let r = real.pop();
                let m = mirror.pop();
                assert_eq!(r, m, "step {i} Pop: real {r:?} vs mirror {m:?}");
            }
            Op::Clear => {
                real.clear();
                mirror.clear();
            }
            Op::IsEmpty => {
                assert_eq!(
                    real.is_empty(),
                    mirror.is_empty(),
                    "step {i} IsEmpty mismatch"
                );
            }
        }
    }
}

#[test]
fn corr_basic_push_pop() {
    assert_corresponds::<8>(&[
        Op::IsEmpty,
        Op::Push(1),
        Op::Push(2),
        Op::Push(3),
        Op::IsEmpty,
        Op::Pop,
        Op::Pop,
        Op::Pop,
        Op::IsEmpty,
        Op::Pop,
    ]);
}

#[test]
fn corr_overflow() {
    // Capacity 4: fifth push must fail in both implementations.
    assert_corresponds::<4>(&[
        Op::Push(10),
        Op::Push(20),
        Op::Push(30),
        Op::Push(40),
        Op::Push(50), // overflow
        Op::Pop,
        Op::Push(60), // succeeds again after pop
        Op::Push(70), // overflow
    ]);
}

#[test]
fn corr_clear_empties() {
    assert_corresponds::<8>(&[
        Op::Push(1),
        Op::Push(2),
        Op::Push(3),
        Op::Clear,
        Op::IsEmpty,
        Op::Pop,
        Op::Push(99),
        Op::Pop,
    ]);
}

#[test]
fn corr_alternating() {
    // Wrap-around stress test: push and pop interleave so the ring
    // buffer's head index wraps multiple times.
    let mut ops = Vec::new();
    for i in 0..50u32 {
        ops.push(Op::Push(i));
        ops.push(Op::Push(i + 100));
        ops.push(Op::Pop);
    }
    assert_corresponds::<8>(&ops);
}

#[test]
fn corr_fill_drain_repeat() {
    // Fill to capacity, drain fully, repeat 5 times.
    let mut ops = Vec::new();
    for round in 0..5 {
        for i in 0..8u32 {
            ops.push(Op::Push(round * 100 + i));
        }
        for _ in 0..8 {
            ops.push(Op::Pop);
        }
    }
    ops.push(Op::IsEmpty);
    assert_corresponds::<8>(&ops);
}
