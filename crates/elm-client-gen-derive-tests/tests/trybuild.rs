//! Compile-time tests for `#[derive(ElmType)]`.
//!
//! `compile_fail/` pins the error messages emitted for each rejected
//! Rust shape so changes to the derive either keep the diagnostic or
//! deliberately update the pinned `.stderr`. `compile_pass/` exercises
//! accepted shapes so a regression that narrows the accepted set fails
//! loudly even without a runtime assertion.
//!
//! Run `TRYBUILD=overwrite cargo test --test trybuild` to regenerate
//! pinned `.stderr` files after an intentional message change.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}

#[test]
fn compile_pass() {
    let t = trybuild::TestCases::new();
    t.pass("tests/compile_pass/*.rs");
}
