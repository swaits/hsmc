//! Correspondence test: verified [`TimerTable`] mirror vs the real
//! runtime [`hsmc::__private::TimerTable`].
//!
//! Same idea as `correspondence_event_queue.rs` — drive both with the
//! same op sequence and assert observable parity.

use hsmc::__private::TimerTable as RealTable;
use hsmc_verification::timer_table::TimerTable as MirrorTable;

#[derive(Debug, Clone, Copy)]
enum Op {
    Start { state: u16, trigger: u16, ns: u128 },
    CancelState(u16),
    CancelOne { state: u16, trigger: u16 },
    Decrement(u128),
    PopExpired,
    MinRemaining,
    Len,
}

fn assert_corresponds<const CAP: usize>(depth: &[u8], ops: &[Op]) {
    let mut real: RealTable<CAP> = RealTable::new();
    let mut mirror: MirrorTable<CAP> = MirrorTable::new();

    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Start { state, trigger, ns } => {
                real.start(
                    *state,
                    *trigger,
                    core::time::Duration::from_nanos(*ns as u64),
                );
                mirror.start(*state, *trigger, *ns);
            }
            Op::CancelState(s) => {
                real.cancel_state(*s);
                mirror.cancel_state(*s);
            }
            Op::CancelOne { state, trigger } => {
                real.cancel_one(*state, *trigger);
                mirror.cancel_one(*state, *trigger);
            }
            Op::Decrement(elapsed) => {
                real.decrement(core::time::Duration::from_nanos(*elapsed as u64));
                mirror.decrement(*elapsed);
            }
            Op::PopExpired => {
                let r = real.pop_expired(depth);
                let m = mirror.pop_expired(depth);
                assert_eq!(r, m, "step {i} PopExpired: real {r:?} vs mirror {m:?}");
            }
            Op::MinRemaining => {
                let r = real.min_remaining().map(|d| d.as_nanos());
                let m = mirror.min_remaining();
                assert_eq!(r, m, "step {i} MinRemaining: real {r:?} vs mirror {m:?}");
            }
            Op::Len => {
                assert_eq!(
                    real.entries.len(),
                    mirror.len(),
                    "step {i} Len mismatch"
                );
            }
        }
    }
}

#[test]
fn corr_timer_start_replaces_in_place() {
    // Re-starting (1, 10) must not duplicate; the table has one entry for it.
    assert_corresponds::<8>(
        &[0, 1, 1, 1, 1, 1, 1, 1],
        &[
            Op::Start { state: 1, trigger: 10, ns: 100 },
            Op::Len,
            Op::Start { state: 1, trigger: 10, ns: 500 },
            Op::Len, // still 1 entry
            Op::MinRemaining,
        ],
    );
}

#[test]
fn corr_timer_cancel_state_removes_only_matching() {
    assert_corresponds::<8>(
        &[0, 1, 1, 2],
        &[
            Op::Start { state: 1, trigger: 10, ns: 100 },
            Op::Start { state: 1, trigger: 20, ns: 200 },
            Op::Start { state: 2, trigger: 10, ns: 300 },
            Op::CancelState(1),
            Op::Len, // only state-2 survives
            Op::PopExpired, // none expired
        ],
    );
}

#[test]
fn corr_timer_cancel_one_targets_exact_pair() {
    assert_corresponds::<8>(
        &[0, 1, 1, 2],
        &[
            Op::Start { state: 1, trigger: 10, ns: 100 },
            Op::Start { state: 1, trigger: 20, ns: 200 },
            Op::Start { state: 2, trigger: 10, ns: 300 },
            Op::CancelOne { state: 1, trigger: 10 },
            Op::Len,
        ],
    );
}

#[test]
fn corr_timer_decrement_to_zero_then_pop() {
    assert_corresponds::<8>(
        &[0, 1, 1, 1],
        &[
            Op::Start { state: 1, trigger: 10, ns: 500 },
            Op::Decrement(200),
            Op::MinRemaining, // 300
            Op::Decrement(400), // saturates to 0
            Op::MinRemaining, // 0
            Op::PopExpired,   // returns (1, 10)
            Op::Len,          // 0
            Op::PopExpired,   // None
        ],
    );
}

#[test]
fn corr_timer_pop_picks_deepest() {
    // Two expired timers at different depths. depth[1]=1, depth[2]=3.
    // pop_expired must pick state 2 first (deeper).
    assert_corresponds::<8>(
        &[0, 1, 3, 0],
        &[
            Op::Start { state: 1, trigger: 100, ns: 0 },
            Op::Start { state: 2, trigger: 200, ns: 0 },
            Op::PopExpired, // (2, 200)
            Op::PopExpired, // (1, 100)
            Op::PopExpired, // None
        ],
    );
}

#[test]
fn corr_timer_pop_breaks_tie_by_declaration_order() {
    // depth[1]=2, depth[2]=2: equal depth, first-declared (1, 100) wins.
    assert_corresponds::<8>(
        &[0, 2, 2, 0],
        &[
            Op::Start { state: 1, trigger: 100, ns: 0 },
            Op::Start { state: 2, trigger: 200, ns: 0 },
            Op::PopExpired, // (1, 100)
            Op::PopExpired, // (2, 200)
        ],
    );
}

#[test]
fn corr_timer_min_remaining_is_smallest() {
    assert_corresponds::<8>(
        &[0, 1, 1, 1],
        &[
            Op::Start { state: 1, trigger: 1, ns: 9_999 },
            Op::Start { state: 2, trigger: 2, ns: 100 },
            Op::Start { state: 3, trigger: 3, ns: 5_000 },
            Op::MinRemaining, // 100
        ],
    );
}

#[test]
fn corr_timer_complex_sequence() {
    assert_corresponds::<8>(
        &[0, 1, 1, 2, 2, 3],
        &[
            Op::Start { state: 1, trigger: 1, ns: 1_000 },
            Op::Start { state: 2, trigger: 1, ns: 2_000 },
            Op::Start { state: 3, trigger: 1, ns: 3_000 },
            Op::Decrement(1_500),
            Op::MinRemaining, // 0 (state 1 hit zero)
            Op::PopExpired,   // (1, 1) — only one expired
            Op::Decrement(600),
            Op::PopExpired, // (2, 1) — now expired
            Op::Len,        // 1
            Op::CancelState(3),
            Op::Len, // 0
        ],
    );
}
