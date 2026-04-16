//! Compile-fail tests for `statechart!` validations (§9.9) and
//! compile-pass parse tests (spec §8).
#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
    t.pass("tests/ui_pass/*.rs");
}
