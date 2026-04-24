//! Reference CLI for `elm-client-gen`.
//!
//! Walks every type registered via `#[derive(ElmType)]`, applies any
//! type-name overrides you configure, groups by Elm module path, and
//! writes one `.elm` file per module to the output directory.
//!
//! # Linking note
//!
//! Each `#[derive(ElmType)]` emits a static initializer that pushes
//! into the global `inventory` collector. If you depend on a crate
//! that *only* contributes via these initializers, you must reference
//! it from your binary so rustc keeps its rlib linked. The cleanest
//! pattern is `use my_schema_crate as _;` at the top of `main.rs`.
//! This CLI is a *reference* and doesn't pull in any user crate by
//! default. Roll your own binary if you need to ship something
//! preconfigured for a specific schema.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use elm_client_gen_cli::{run_codegen, CodegenOptions, CodegenOutcome};
use elm_client_gen_core::registered_types;

#[derive(Parser)]
#[command(name = "elm-client-gen", about)]
struct Cli {
    /// Output directory for generated `.elm` files.
    #[arg(short, long)]
    output: PathBuf,

    /// Filter to specific Elm type names. Omit for all.
    #[arg(short, long)]
    types: Vec<String>,

    /// Print to stdout instead of writing files.
    #[arg(long)]
    dry_run: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = CodegenOptions {
        output: cli.output,
        filter_names: &cli.types,
        dry_run: cli.dry_run,
    };
    match run_codegen(registered_types(), options) {
        Ok(CodegenOutcome::DryRun(output)) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Ok(CodegenOutcome::Wrote { module_count, root }) => {
            println!(
                "Generated {} type modules in {}",
                module_count,
                root.display()
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
