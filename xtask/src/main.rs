//! Workspace task runner. Run via `cargo xtask <subcommand>`.
//!
//! `cargo --all-features` does not work in this workspace: the
//! `axum-0-6` / `axum-0-7` / `axum-0-8` features in `elm-client-gen-http`
//! pull in incompatible trait impls on shared types (StatusCode, Bytes).
//! These tasks iterate the supported feature combinations one by one.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

/// Mutually exclusive feature combinations the workspace must build clean
/// under. `cargo --all-features` will not work — see crate doc.
const FEATURE_COMBOS: &[(&str, &str)] = &[
    ("baseline (no axum)", "derive,uuid"),
    ("axum 0.6", "derive,uuid,axum-extra-0-7"),
    ("axum 0.7", "derive,uuid,axum-extra-0-9"),
    ("axum 0.8", "derive,uuid,axum-extra-0-10"),
];

#[derive(Parser)]
#[command(
    name = "xtask",
    about = "Workspace task runner for elm-client-gen",
    bin_name = "cargo xtask"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// `cargo clippy -D warnings` across every supported feature combo.
    CheckFeatures,
    /// `cargo test` across every supported feature combo.
    TestFeatures,
    /// `cargo fmt --all -- --check`.
    FmtCheck,
    /// fmt-check + check-features + test-features. Use before publishing.
    Preflight,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::CheckFeatures => check_features(),
        Cmd::TestFeatures => test_features(),
        Cmd::FmtCheck => fmt_check(),
        Cmd::Preflight => preflight(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR points at xtask/, parent is the workspace root.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .context("CARGO_MANIFEST_DIR not set; run via `cargo xtask`")?;
    Ok(Path::new(&manifest_dir)
        .parent()
        .context("xtask must be a workspace member")?
        .to_path_buf())
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn run(label: &str, args: &[&str]) -> Result<()> {
    println!("\n=== {label}: cargo {} ===", args.join(" "));
    let status = Command::new(cargo())
        .args(args)
        .current_dir(workspace_root()?)
        .status()
        .with_context(|| format!("failed to spawn cargo for: {label}"))?;
    if !status.success() {
        bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn check_features() -> Result<()> {
    for (label, features) in FEATURE_COMBOS {
        run(
            &format!("clippy {label}"),
            &[
                "clippy",
                "--workspace",
                "--exclude",
                "xtask",
                "--all-targets",
                "--no-default-features",
                "--features",
                features,
                "--",
                "-D",
                "warnings",
            ],
        )?;
    }
    println!("\nAll feature combos passed clippy.");
    Ok(())
}

fn test_features() -> Result<()> {
    for (label, features) in FEATURE_COMBOS {
        run(
            &format!("test {label}"),
            &[
                "test",
                "--workspace",
                "--exclude",
                "xtask",
                "--no-default-features",
                "--features",
                features,
            ],
        )?;
    }
    println!("\nAll feature combos passed tests.");
    Ok(())
}

fn fmt_check() -> Result<()> {
    run("fmt --check", &["fmt", "--all", "--", "--check"])
}

fn preflight() -> Result<()> {
    fmt_check()?;
    check_features()?;
    test_features()?;
    println!("\nPreflight clean.");
    Ok(())
}
