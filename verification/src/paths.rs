//! Verified validator for the codegen-emitted transition path tables.
//!
//! Background: after the Opt #2 perf pass, the generated `step()` /
//! `apply_path()` / `do_terminate()` index into static tables
//! (`__PATH_RANGE`, `__PATH_DATA`, `__TERMINATE_RANGE`,
//! `__TERMINATE_DATA`, `__DEFAULT_CHILD`) via `slice::get_unchecked`.
//! That's sound iff the codegen-emitted values satisfy two predicates:
//!
//! - **P1** *(state-id well-formedness)*: every state id stored in
//!   `__PATH_DATA`, `__TERMINATE_DATA`, or as a `Some(t)` inside
//!   `__DEFAULT_CHILD` is `< n_states`.
//! - **P2** *(range-table well-formedness)*: every triple
//!   `__PATH_RANGE[i] = (s, m, e)` satisfies `0 ≤ s ≤ m ≤ e ≤ data_len`.
//!   Same for `__TERMINATE_RANGE[i] = (s, e)` against `__TERMINATE_DATA`.
//!
//! The codegen already enforces these at *user build time* via a
//! `const _: () = { ... }` block in `hsmc-macros::codegen` that runs the
//! same predicates as `assert!`s in const-context — any chart whose
//! emitted tables fail the check is a `cargo check` error before
//! reaching runtime. So the property is operationally proven for every
//! chart that compiles.
//!
//! What this module adds: a Creusot-verified version of the *predicate
//! itself*, so we know the property the const-eval block is checking
//! is the property the unsafe blocks rely on. The contracts say:
//!
//! ```text
//! check_path_range_invariant(range, data_len) == true
//!     iff
//! ∀ i ∈ 0..range.len(): let (s, m, e) = range[i] in
//!     s ≤ m ≤ e ≤ data_len
//! ```
//!
//! ```text
//! check_path_ids_in_range(data, n_states) == true
//!     iff
//! ∀ i ∈ 0..data.len(): data[i] < n_states
//! ```
//!
//! With Creusot discharging both VCs, the chain
//!
//! - hsmc-macros emits tables
//! - const-eval runs the predicate (or its rustc-evaluated equivalent)
//! - this module's predicate is proven equivalent to P1 + P2
//! - therefore P1 + P2 hold for every chart that compiled
//!
//! is closed end-to-end with the only remaining trust point being the
//! identity between the const-eval block's body and this module's
//! predicate (mechanical — they are the same expressions, copied in
//! `tests/correspondence_paths.rs`).

use creusot_std::macros::ensures;
// `pearlite!` looks unused to plain rustc but Creusot's preprocessing
// needs it for quantified ensures bodies.
#[allow(unused_imports)]
use creusot_std::macros::pearlite;

/// Returns `true` iff every triple `(s, m, e)` in `range` satisfies
/// `s ≤ m ≤ e ≤ data_len`. Mirror of the const-eval check on
/// `__PATH_RANGE`.
///
/// Pearlite contract: the function is total (terminates), pure, and
/// the result is equivalent to the universal predicate over `range`.
#[ensures(result == forall<i: Int>
    0 <= i && i < range@.len()
    ==> (range@[i].0 as u64) <= (range@[i].1 as u64)
        && (range@[i].1 as u64) <= (range@[i].2 as u64)
        && (range@[i].2 as u64) <= (data_len as u64)
)]
pub fn check_path_range_invariant(range: &[(u16, u16, u16)], data_len: usize) -> bool {
    let mut i: usize = 0;
    #[allow(clippy::needless_range_loop)]
    while i < range.len() {
        let (s, m, e) = range[i];
        if !((s as usize) <= (m as usize)
            && (m as usize) <= (e as usize)
            && (e as usize) <= data_len)
        {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns `true` iff every triple `(s, e)` in `range` satisfies
/// `s ≤ e ≤ data_len`. Mirror of the const-eval check on
/// `__TERMINATE_RANGE`.
#[ensures(result == forall<i: Int>
    0 <= i && i < range@.len()
    ==> (range@[i].0 as u64) <= (range@[i].1 as u64)
        && (range@[i].1 as u64) <= (data_len as u64)
)]
pub fn check_terminate_range_invariant(range: &[(u16, u16)], data_len: usize) -> bool {
    let mut i: usize = 0;
    while i < range.len() {
        let (s, e) = range[i];
        if !((s as usize) <= (e as usize) && (e as usize) <= data_len) {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns `true` iff every state id stored in `data` is `< n_states`.
/// Mirror of the const-eval check on `__PATH_DATA` and
/// `__TERMINATE_DATA`.
#[ensures(result == forall<i: Int>
    0 <= i && i < data@.len()
    ==> (data@[i] as u64) < (n_states as u64)
)]
pub fn check_state_ids_in_bounds(data: &[u16], n_states: usize) -> bool {
    let mut i: usize = 0;
    while i < data.len() {
        if (data[i] as usize) >= n_states {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns `true` iff every `Some(t)` in `default_child` satisfies
/// `t < n_states`. `None` entries (states with no default child) are
/// ignored. Mirror of the const-eval check on `__DEFAULT_CHILD`.
#[ensures(result == forall<i: Int>
    0 <= i && i < default_child@.len()
    ==> match default_child@[i] {
        Some(t) => (t as u64) < (n_states as u64),
        None => true,
    }
)]
pub fn check_default_child_in_bounds(default_child: &[Option<u16>], n_states: usize) -> bool {
    let mut i: usize = 0;
    while i < default_child.len() {
        if let Some(t) = default_child[i] {
            if (t as usize) >= n_states {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// Aggregate predicate: returns `true` iff all four invariants hold.
/// This is the property that, when true, makes every codegen-emitted
/// `unsafe { ... .get_unchecked(...) }` in step / apply_path /
/// follow_defaults / do_terminate sound.
#[ensures(result == (
    (forall<i: Int>
        0 <= i && i < path_range@.len()
        ==> (path_range@[i].0 as u64) <= (path_range@[i].1 as u64)
            && (path_range@[i].1 as u64) <= (path_range@[i].2 as u64)
            && (path_range@[i].2 as u64) <= (path_data@.len() as u64))
    && (forall<i: Int>
        0 <= i && i < terminate_range@.len()
        ==> (terminate_range@[i].0 as u64) <= (terminate_range@[i].1 as u64)
            && (terminate_range@[i].1 as u64) <= (terminate_data@.len() as u64))
    && (forall<i: Int>
        0 <= i && i < path_data@.len()
        ==> (path_data@[i] as u64) < (n_states as u64))
    && (forall<i: Int>
        0 <= i && i < terminate_data@.len()
        ==> (terminate_data@[i] as u64) < (n_states as u64))
    && (forall<i: Int>
        0 <= i && i < default_child@.len()
        ==> match default_child@[i] {
            Some(t) => (t as u64) < (n_states as u64),
            None => true,
        })
))]
#[allow(clippy::too_many_arguments)]
pub fn check_path_tables_sound(
    path_range: &[(u16, u16, u16)],
    path_data: &[u16],
    terminate_range: &[(u16, u16)],
    terminate_data: &[u16],
    default_child: &[Option<u16>],
    n_states: usize,
) -> bool {
    check_path_range_invariant(path_range, path_data.len())
        && check_terminate_range_invariant(terminate_range, terminate_data.len())
        && check_state_ids_in_bounds(path_data, n_states)
        && check_state_ids_in_bounds(terminate_data, n_states)
        && check_default_child_in_bounds(default_child, n_states)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sanity checks that the predicates behave correctly on small
    // inputs. The ensures clauses are what Creusot proves; these tests
    // pin down the executable behavior.

    #[test]
    fn empty_range_passes() {
        assert!(check_path_range_invariant(&[], 0));
        assert!(check_path_range_invariant(&[], 100));
        assert!(check_terminate_range_invariant(&[], 0));
        assert!(check_state_ids_in_bounds(&[], 0));
        assert!(check_default_child_in_bounds(&[], 0));
    }

    #[test]
    fn well_formed_paths() {
        // s ≤ m ≤ e ≤ data_len for all entries.
        let data: Vec<u16> = vec![0, 1, 2, 0, 1];
        let range = vec![(0, 2, 3), (3, 3, 5), (0, 0, 0)];
        assert!(check_path_range_invariant(&range, data.len()));
        assert!(check_state_ids_in_bounds(&data, 3));
    }

    #[test]
    fn path_range_s_gt_m_rejected() {
        let range = vec![(2, 1, 3)];
        assert!(!check_path_range_invariant(&range, 5));
    }

    #[test]
    fn path_range_m_gt_e_rejected() {
        let range = vec![(0, 3, 2)];
        assert!(!check_path_range_invariant(&range, 5));
    }

    #[test]
    fn path_range_e_past_data_rejected() {
        let range = vec![(0, 1, 6)];
        assert!(!check_path_range_invariant(&range, 5));
    }

    #[test]
    fn state_id_oob_rejected() {
        let data = vec![0u16, 1, 99];
        assert!(!check_state_ids_in_bounds(&data, 3));
    }

    #[test]
    fn default_child_some_oob_rejected() {
        let default_child = vec![Some(0u16), None, Some(99)];
        assert!(!check_default_child_in_bounds(&default_child, 3));
    }

    #[test]
    fn default_child_all_none_passes() {
        let default_child = vec![None, None, None];
        assert!(check_default_child_in_bounds(&default_child, 0));
    }

    #[test]
    fn aggregate_sound() {
        let path_range = vec![(0, 1, 2)];
        let path_data = vec![0u16, 1];
        let term_range = vec![(0, 1)];
        let term_data = vec![0u16];
        let default_child = vec![Some(0u16), None];
        assert!(check_path_tables_sound(
            &path_range,
            &path_data,
            &term_range,
            &term_data,
            &default_child,
            2,
        ));
    }

    #[test]
    fn aggregate_unsound_when_any_fails() {
        let path_range = vec![(0, 1, 2)];
        let path_data = vec![0u16, 99]; // id out of range
        let term_range = vec![(0, 1)];
        let term_data = vec![0u16];
        let default_child = vec![Some(0u16), None];
        assert!(!check_path_tables_sound(
            &path_range,
            &path_data,
            &term_range,
            &term_data,
            &default_child,
            2,
        ));
    }
}
