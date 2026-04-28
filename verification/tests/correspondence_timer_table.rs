//! Correspondence test: verified [`TimerTable`] mirror vs runtime.
//!
//! The verified mirror currently exposes only `new`, `len`, `is_empty`.
//! The runtime ops (`start`, `cancel_state`, `cancel_one`, `decrement`,
//! `pop_expired`, `min_remaining`) aren't yet ported into the verified
//! mirror — when they are, this test will grow to drive both with the
//! same op sequence and assert step-by-step parity.
//!
//! For now the runtime's behavior on those ops is pinned by the
//! determinism tests in `hsmc/tests/det_timers*.rs` and the unit tests
//! in `hsmc/src/lib.rs::private_internals`.

use hsmc::__private::TimerTable as RealTable;
use hsmc_verification::timer_table::TimerTable as MirrorTable;

#[test]
fn corr_new_is_empty() {
    let real: RealTable<8> = RealTable::new();
    let mirror: MirrorTable = MirrorTable::new();
    assert_eq!(real.entries.len(), mirror.len());
    assert_eq!(real.entries.is_empty(), mirror.is_empty());
}

#[test]
fn corr_default_is_empty() {
    let real: RealTable<8> = RealTable::default();
    let mirror: MirrorTable = MirrorTable::default();
    assert_eq!(real.entries.len(), mirror.len());
    assert!(mirror.is_empty());
    assert!(real.entries.is_empty());
}
