//! Integration tests for the CLI library surface and shipped binary.
//!
//! Library tests drive `run_codegen` directly with fixture types so we
//! can assert on rendered output and on-disk layout without linking
//! the fixtures into the published binary. Binary tests shell out to
//! the compiled `elm-client-gen` executable to check argument handling
//! and the "empty registry" exit behavior.

use std::path::PathBuf;

use assert_cmd::Command;
use elm_client_gen_cli::{run_codegen, CodegenOptions, CodegenOutcome};
use elm_client_gen_core::{ElmType, ElmTypeInfo};
use predicates::str::contains;
use test_better::prelude::*;
use test_better::ErrorKind;

fn fail(msg: impl Into<String>) -> TestError {
    TestError::new(ErrorKind::Assertion).with_message(msg.into())
}

#[derive(ElmType)]
#[elm(module = "Fixtures.Person", name = "Person")]
#[allow(dead_code)]
pub struct FixturePerson {
    pub id: String,
    pub name: String,
}

#[derive(ElmType)]
#[elm(module = "Fixtures.Order", name = "Order")]
#[allow(dead_code)]
pub struct FixtureOrder {
    pub id: String,
    pub total: f64,
}

fn fixtures() -> Vec<ElmTypeInfo> {
    vec![
        FixturePerson::elm_type_info(),
        FixtureOrder::elm_type_info(),
    ]
}

#[test]
fn dry_run_renders_modules_with_banner_per_module() -> TestResult {
    let outcome = run_codegen(
        fixtures(),
        CodegenOptions {
            output: PathBuf::new(),
            filter_names: &[],
            dry_run: true,
        },
    )
    .map_err(|e| fail(format!("dry run with fixtures should succeed: {e}")))?;

    let CodegenOutcome::DryRun(output) = outcome else {
        return Err(fail("expected DryRun outcome"));
    };

    check!(output.as_str()).satisfies(contains_str("-- Fixtures.Order.elm --"))?;
    check!(output.as_str()).satisfies(contains_str("-- Fixtures.Person.elm --"))?;
    check!(output.as_str()).satisfies(contains_str("module Fixtures.Person exposing"))?;
    check!(output.as_str()).satisfies(contains_str("module Fixtures.Order exposing"))?;
    check!(output.as_str()).satisfies(contains_str("type alias Person ="))?;
    check!(output.as_str()).satisfies(contains_str("type alias Order ="))?;
    Ok(())
}

#[test]
fn writes_one_file_per_module_in_nested_directories() -> TestResult {
    let tmp = tempfile::tempdir().or_fail_with("tempdir")?;
    let output = tmp.path().to_path_buf();

    let outcome = run_codegen(
        fixtures(),
        CodegenOptions {
            output: output.clone(),
            filter_names: &[],
            dry_run: false,
        },
    )
    .map_err(|e| fail(format!("write run with fixtures should succeed: {e}")))?;

    let CodegenOutcome::Wrote { module_count, root } = outcome else {
        return Err(fail("expected Wrote outcome"));
    };
    check!(module_count).satisfies(eq(2))?;
    check!(&root).satisfies(eq(&output))?;

    let person = output.join("Fixtures").join("Person.elm");
    let order = output.join("Fixtures").join("Order.elm");
    check!(person.exists())
        .satisfies(is_true())
        .context(format!("missing {}", person.display()))?;
    check!(order.exists())
        .satisfies(is_true())
        .context(format!("missing {}", order.display()))?;

    let person_src = std::fs::read_to_string(&person).or_fail_with("read Person.elm")?;
    check!(person_src.as_str()).satisfies(contains_str("module Fixtures.Person exposing"))?;
    check!(person_src.as_str()).satisfies(contains_str("type alias Person ="))?;
    Ok(())
}

#[test]
fn filter_narrows_output_to_requested_type_names() -> TestResult {
    let tmp = tempfile::tempdir().or_fail_with("tempdir")?;
    let output = tmp.path().to_path_buf();

    let filter = vec![String::from("Person")];
    let outcome = run_codegen(
        fixtures(),
        CodegenOptions {
            output: output.clone(),
            filter_names: &filter,
            dry_run: false,
        },
    )
    .map_err(|e| fail(format!("filter run should succeed: {e}")))?;

    let CodegenOutcome::Wrote { module_count, .. } = outcome else {
        return Err(fail("expected Wrote outcome"));
    };
    check!(module_count).satisfies(eq(1))?;
    check!(output.join("Fixtures").join("Person.elm").exists()).satisfies(is_true())?;
    check!(output.join("Fixtures").join("Order.elm").exists()).satisfies(is_false())?;
    Ok(())
}

#[test]
fn empty_registry_is_a_hard_error() -> TestResult {
    let Err(err) = run_codegen(
        Vec::new(),
        CodegenOptions {
            output: PathBuf::new(),
            filter_names: &[],
            dry_run: true,
        },
    ) else {
        return Err(fail("empty registry should error"));
    };
    check!(err.to_string().as_str()).satisfies(contains_str("No types matched"))?;
    Ok(())
}

#[test]
fn binary_help_lists_expected_flags() -> TestResult {
    Command::cargo_bin("elm-client-gen")
        .or_fail_with("binary built")?
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--output"))
        .stdout(contains("--types"))
        .stdout(contains("--dry-run"));
    Ok(())
}

#[test]
fn binary_exits_nonzero_when_no_types_are_registered() -> TestResult {
    // The published binary has no linked schema crate, so its registry
    // is empty. The CLI should error out with the standard "did you
    // link a schema crate?" message.
    let tmp = tempfile::tempdir().or_fail_with("tempdir")?;
    Command::cargo_bin("elm-client-gen")
        .or_fail_with("binary built")?
        .arg("--dry-run")
        .arg("--output")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(contains("No types matched"));
    Ok(())
}
