//! Correspondence test: the Pearlite-verified path-table validator
//! evaluates the *same predicate* as the `const _: () = { ... }` block
//! that `hsmc-macros` emits in every generated chart.
//!
//! Why this matters: the safety chain for the unsafe blocks in
//! `step()` / `apply_path()` / `do_terminate()` / `follow_defaults()` is
//!
//!   (a) hsmc-macros emits __PATH_RANGE / __PATH_DATA / __TERMINATE_*
//!       / __DEFAULT_CHILD with the values produced by
//!       `compute_transition_paths` and `compute_terminate_paths`.
//!   (b) A const-eval block runs `assert!(s <= m && m <= e && e <= len
//!       && id < n_states && ...)` over those tables at user build
//!       time. Build fails if any chart's tables violate the predicate.
//!   (c) The Pearlite-annotated validator in
//!       `hsmc_verification::paths` proves (under Creusot) that the
//!       *function* that runs those checks correctly decides P1 + P2.
//!
//! The remaining trust point is "(b) and (c) are checking the same
//! thing". This test pins that down by exercising the validator on
//! the exact predicate the const-eval block uses, on inputs that
//! mirror codegen's output shape — both well-formed and adversarial.
//!
//! When this test passes alongside the const-eval block compiling,
//! every chart that compiles has been verified by a function whose
//! correctness is in turn formally proven (when `just verify` runs).

use hsmc_verification::paths::{
    check_default_child_in_bounds, check_path_range_invariant, check_path_tables_sound,
    check_state_ids_in_bounds, check_terminate_range_invariant,
};

/// Shape that mirrors `compute_transition_paths`'s output for a small
/// chart with N=4 states:
///
///   __Root
///   ├── A (id 1, depth 1)
///   ├── B (id 2, depth 1)
///   └── C (id 3, depth 1, default child = none)
///
/// Encoded paths for transition A→B (lateral, LCA = root):
///   exit_path = [A]; enter_path = [B]
/// __PATH_RANGE[A * N + B] = (s=0, m=1, e=2)
/// __PATH_DATA = [A, B] = [1, 2]
// `1 * 4 + 2` etc. are deliberately written `row * stride + col` so the
// __PATH_RANGE indexing scheme matches the codegen formula
// `from * N_STATES + to`; reducing to literals would obscure intent.
// The helper's 6-tuple shape mirrors the codegen outputs we're
// validating; factoring into a struct would just rename the same data.
#[allow(clippy::identity_op, clippy::type_complexity)]
fn small_chart_tables() -> (
    Vec<(u16, u16, u16)>, // path_range
    Vec<u16>,             // path_data
    Vec<(u16, u16)>,      // terminate_range
    Vec<u16>,             // terminate_data
    Vec<Option<u16>>,     // default_child
    usize,                // n_states (incl. root)
) {
    // 4 states (0=root, 1=A, 2=B, 3=C). 16-entry path_range.
    // Identity transitions self → self use (s, m, e) = (i*2, i*2, i*2+1)
    // — exit empty, enter [self]. Lateral A→B and B→A point at the same
    // path_data slice. The exact values don't matter for this test;
    // what matters is the structural invariant holds.
    let path_data: Vec<u16> = vec![1, 2, 2, 1];
    // 4*4 = 16 entries; only a few are populated, rest are (0, 0, 0)
    // which is the "no transition" sentinel. (s, m, e) = (0, 0, 0)
    // satisfies s <= m <= e <= len trivially.
    let mut path_range: Vec<(u16, u16, u16)> = vec![(0, 0, 0); 16];
    path_range[1 * 4 + 2] = (0, 1, 2); // A→B: exit [A], enter [B]
    path_range[2 * 4 + 1] = (2, 3, 4); // B→A: exit [B], enter [A]

    // 4 states, each terminate path is the state itself plus root.
    let terminate_data: Vec<u16> = vec![1, 0, 2, 0, 3, 0]; // A,root | B,root | C,root
    let terminate_range: Vec<(u16, u16)> = vec![
        (0, 0), // root: empty
        (0, 2), // A: [A, root]
        (2, 4), // B: [B, root]
        (4, 6), // C: [C, root]
    ];

    let default_child: Vec<Option<u16>> = vec![Some(1), None, None, None];

    (
        path_range,
        path_data,
        terminate_range,
        terminate_data,
        default_child,
        4,
    )
}

#[test]
fn well_formed_tables_pass_validator() {
    let (pr, pd, tr, td, dc, n) = small_chart_tables();
    assert!(check_path_range_invariant(&pr, pd.len()));
    assert!(check_terminate_range_invariant(&tr, td.len()));
    assert!(check_state_ids_in_bounds(&pd, n));
    assert!(check_state_ids_in_bounds(&td, n));
    assert!(check_default_child_in_bounds(&dc, n));
    assert!(check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
#[allow(clippy::identity_op)]
fn adversarial_path_range_s_gt_m_caught() {
    // Same shape but with one bad triple. Mirrors what an
    // off-by-one in `compute_transition_paths` would emit.
    let (mut pr, pd, tr, td, dc, n) = small_chart_tables();
    pr[1 * 4 + 2] = (2, 1, 2); // s > m
    assert!(!check_path_range_invariant(&pr, pd.len()));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
#[allow(clippy::identity_op)]
fn adversarial_path_range_e_past_data_caught() {
    let (mut pr, pd, tr, td, dc, n) = small_chart_tables();
    pr[1 * 4 + 2] = (0, 1, 99); // e past data_len
    assert!(!check_path_range_invariant(&pr, pd.len()));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
fn adversarial_path_data_id_oob_caught() {
    let (pr, mut pd, tr, td, dc, n) = small_chart_tables();
    pd[1] = 99; // state id past N_STATES
    assert!(!check_state_ids_in_bounds(&pd, n));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
fn adversarial_terminate_range_e_past_data_caught() {
    let (pr, pd, mut tr, td, dc, n) = small_chart_tables();
    tr[1] = (0, 99); // e past terminate_data_len
    assert!(!check_terminate_range_invariant(&tr, td.len()));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
fn adversarial_terminate_data_id_oob_caught() {
    let (pr, pd, tr, mut td, dc, n) = small_chart_tables();
    td[0] = 99;
    assert!(!check_state_ids_in_bounds(&td, n));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
fn adversarial_default_child_id_oob_caught() {
    let (pr, pd, tr, td, mut dc, n) = small_chart_tables();
    dc[0] = Some(99);
    assert!(!check_default_child_in_bounds(&dc, n));
    assert!(!check_path_tables_sound(&pr, &pd, &tr, &td, &dc, n));
}

#[test]
fn empty_tables_pass_trivially() {
    // Single-state chart (just root, no transitions): all tables
    // either empty or trivial.
    assert!(check_path_tables_sound(
        &[],
        &[],
        &[(0, 0)],
        &[],
        &[None],
        1,
    ));
}

#[test]
fn the_const_eval_block_predicate_matches_the_validator() {
    // The const-eval block emitted by hsmc-macros::codegen runs:
    //
    //   assert!(s as usize <= m as usize, "...");
    //   assert!(m as usize <= e as usize, "...");
    //   assert!(e as usize <= __PATH_DATA.len(), "...");
    //
    // Translated: for every i, range[i] satisfies s ≤ m ≤ e ≤ len.
    //
    // The validator's #[ensures] clause says the same thing as a
    // forall over Int. This test checks that on a representative
    // input, the validator returns true exactly when the const-eval
    // block's three assertions all pass — the same semantics.
    let data_len = 10usize;
    let cases: &[(u16, u16, u16, bool)] = &[
        // (s, m, e, expected_pass)
        (0, 0, 0, true),
        (0, 5, 10, true),
        (5, 5, 5, true),
        (0, 0, 10, true),
        (0, 10, 10, true),
        (1, 0, 5, false),  // s > m
        (0, 6, 5, false),  // m > e
        (0, 5, 11, false), // e > len
    ];
    for &(s, m, e, expected) in cases {
        let const_eval_predicate = (s as usize) <= (m as usize)
            && (m as usize) <= (e as usize)
            && (e as usize) <= data_len;
        let validator_result = check_path_range_invariant(&[(s, m, e)], data_len);
        assert_eq!(
            const_eval_predicate, expected,
            "const-eval predicate disagrees with expectation for ({s}, {m}, {e})"
        );
        assert_eq!(
            validator_result, expected,
            "validator disagrees with expectation for ({s}, {m}, {e})"
        );
        assert_eq!(
            const_eval_predicate, validator_result,
            "const-eval predicate and validator diverge for ({s}, {m}, {e})"
        );
    }
}
