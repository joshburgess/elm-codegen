//! Reference CLI for `elm-codegen`.
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

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use elm_codegen_builder::{
    build_merged_module, group_by_module, DefaultStrategy, MaybeEncoderRef, NameMap, TypeOverrides,
};
use elm_codegen_core::{registered_types, ElmTypeInfo};

#[derive(Parser)]
#[command(name = "elm-codegen", about)]
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    let overrides = TypeOverrides::new();
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let all = collect_types(&overrides);
    let names = NameMap::from_types(&all);

    let targets: Vec<ElmTypeInfo> = if cli.types.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|t| cli.types.iter().any(|n| n == t.type_name))
            .collect()
    };

    if targets.is_empty() {
        eprintln!("No types matched. Did you `use my_schema_crate as _;` in main.rs?");
        std::process::exit(1);
    }

    let groups = group_by_module(&targets);

    if cli.dry_run {
        for (module_path, group) in &groups {
            let module = build_merged_module(module_path, group, &names, &strategy, &maybe);
            println!(
                "-- {}.elm --\n{}\n",
                module_path.join("."),
                elm_ast::pretty_print(&module)
            );
        }
        return Ok(());
    }

    for (module_path, group) in &groups {
        let module = build_merged_module(module_path, group, &names, &strategy, &maybe);
        write_module(&cli.output, module_path, &module)?;
        println!("Generated {}", module_path.join("."));
    }
    println!("\nGenerated {} type modules in {}", targets.len(), cli.output.display());
    Ok(())
}

fn collect_types(overrides: &TypeOverrides) -> Vec<ElmTypeInfo> {
    let mut types: Vec<ElmTypeInfo> = registered_types()
        .into_iter()
        .map(|t| overrides.apply(t))
        .collect();
    types.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then_with(|| a.type_name.cmp(b.type_name))
    });
    types
}

fn write_module(output_dir: &Path, module_path: &[&str], module: &elm_ast::file::ElmModule) -> Result<()> {
    let mut file_path = output_dir.to_path_buf();
    for segment in module_path {
        file_path.push(segment);
    }
    file_path.set_extension("elm");

    let parent = file_path
        .parent()
        .with_context(|| format!("no parent directory for {}", file_path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating directory {}", parent.display()))?;
    std::fs::write(&file_path, elm_ast::pretty_print(module))
        .with_context(|| format!("writing {}", file_path.display()))?;
    Ok(())
}

