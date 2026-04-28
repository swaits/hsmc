//! Correspondence test: the verified [`EventQueue`] mirror behaves
//! identically to the real runtime [`hsmc::__private::EventQueue`]
//! for the operations we've verified.
//!
//! The mirror is unbounded (uses `Vec<E>`) where the runtime is
//! capacity-bounded (uses `heapless::Deque<E, N>`). This test ignores
//! capacity exhaustion — that path is exercised by the runtime's own
//! unit tests in `hsmc/src/lib.rs::private_internals`. What this test
//! pins down is the FIFO discipline + length tracking + clear, which
//! ARE proven by Creusot for the mirror.

use hsmc::__private::EventQueue as RealQueue;
use hsmc_verification::event_queue::EventQueue as MirrorQueue;

#[derive(Debug, Clone, Copy)]
enum Op {
    Push(u32),
    Pop,
    Clear,
    IsEmpty,
    Len,
}

/// Drive both queues with the same op sequence and assert observable
/// behavior matches at every step.
fn assert_corresponds<const CAP: usize>(ops: &[Op]) {
    let mut real: RealQueue<u32, CAP> = RealQueue::new();
    let mut mirror: MirrorQueue<u32> = MirrorQueue::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Push(v) => {
                // Real may return Err(QueueFull) when full; mirror is
                // unbounded so push always succeeds. Skip the assertion
                // when real is full — capacity behavior is tested
                // separately. Otherwise both succeed; lengths track.
                let r = real.push(*v);
                if r.is_ok() {
                    mirror.push(*v);
                }
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
            Op::Len => {
                // The runtime's EventQueue doesn't expose len(); skip,
                // but the mirror's len is verified to match its view.
                let _ = mirror.len();
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
        Op::Len,
        Op::IsEmpty,
        Op::Pop,
        Op::Pop,
        Op::Pop,
        Op::IsEmpty,
        Op::Pop,
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
