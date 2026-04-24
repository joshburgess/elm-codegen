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
fn dry_run_renders_modules_with_banner_per_module() {
    let outcome = run_codegen(
        fixtures(),
        CodegenOptions {
            output: PathBuf::new(),
            filter_names: &[],
            dry_run: true,
        },
    )
    .expect("dry run with fixtures should succeed");

    let CodegenOutcome::DryRun(output) = outcome else {
        panic!("expected DryRun outcome");
    };

    assert!(output.contains("-- Fixtures.Order.elm --"));
    assert!(output.contains("-- Fixtures.Person.elm --"));
    assert!(output.contains("module Fixtures.Person exposing"));
    assert!(output.contains("module Fixtures.Order exposing"));
    assert!(output.contains("type alias Person ="));
    assert!(output.contains("type alias Order ="));
}

#[test]
fn writes_one_file_per_module_in_nested_directories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = tmp.path().to_path_buf();

    let outcome = run_codegen(
        fixtures(),
        CodegenOptions {
            output: output.clone(),
            filter_names: &[],
            dry_run: false,
        },
    )
    .expect("write run with fixtures should succeed");

    let CodegenOutcome::Wrote { module_count, root } = outcome else {
        panic!("expected Wrote outcome");
    };
    assert_eq!(module_count, 2);
    assert_eq!(root, output);

    let person = output.join("Fixtures").join("Person.elm");
    let order = output.join("Fixtures").join("Order.elm");
    assert!(person.exists(), "missing {}", person.display());
    assert!(order.exists(), "missing {}", order.display());

    let person_src = std::fs::read_to_string(&person).expect("read Person.elm");
    assert!(person_src.contains("module Fixtures.Person exposing"));
    assert!(person_src.contains("type alias Person ="));
}

#[test]
fn filter_narrows_output_to_requested_type_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
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
    .expect("filter run should succeed");

    let CodegenOutcome::Wrote { module_count, .. } = outcome else {
        panic!("expected Wrote outcome");
    };
    assert_eq!(module_count, 1);
    assert!(output.join("Fixtures").join("Person.elm").exists());
    assert!(!output.join("Fixtures").join("Order.elm").exists());
}

#[test]
fn empty_registry_is_a_hard_error() {
    let err = run_codegen(
        Vec::new(),
        CodegenOptions {
            output: PathBuf::new(),
            filter_names: &[],
            dry_run: true,
        },
    )
    .expect_err("empty registry should error");
    assert!(err.to_string().contains("No types matched"));
}

#[test]
fn binary_help_lists_expected_flags() {
    Command::cargo_bin("elm-client-gen")
        .expect("binary built")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--output"))
        .stdout(contains("--types"))
        .stdout(contains("--dry-run"));
}

#[test]
fn binary_exits_nonzero_when_no_types_are_registered() {
    // The published binary has no linked schema crate, so its registry
    // is empty. The CLI should error out with the standard "did you
    // link a schema crate?" message.
    let tmp = tempfile::tempdir().expect("tempdir");
    Command::cargo_bin("elm-client-gen")
        .expect("binary built")
        .arg("--dry-run")
        .arg("--output")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(contains("No types matched"));
}
