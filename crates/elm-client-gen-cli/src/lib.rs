//! Core of the reference CLI, extracted from `main.rs` so integration
//! tests can drive it with fixture types instead of having to link a
//! separate binary that carries them.
//!
//! `main.rs` collects types from `registered_types()` and calls
//! [`run_codegen`]; tests construct their own `&[ElmTypeInfo]` and call
//! it directly with a tempdir as output.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use elm_client_gen_builder::{
    build_merged_module, group_by_module, DefaultStrategy, MaybeEncoderRef, NameMap, TypeOverrides,
};
use elm_client_gen_core::ElmTypeInfo;

/// Everything the CLI's request-handling layer needs to know.
pub struct CodegenOptions<'a> {
    /// Output directory for generated `.elm` files. Unused when
    /// `dry_run` is `true`.
    pub output: PathBuf,
    /// Filter to specific Elm type names. Empty means no filter.
    pub filter_names: &'a [String],
    /// If `true`, return the rendered modules as a string instead of
    /// writing to disk.
    pub dry_run: bool,
}

/// Outcome of a codegen run.
#[derive(Debug)]
pub enum CodegenOutcome {
    /// Files were written to disk. Reports how many modules and the
    /// output root for status-line rendering.
    Wrote { module_count: usize, root: PathBuf },
    /// Dry run: the rendered modules concatenated in the same format
    /// `main.rs` prints to stdout.
    DryRun(String),
}

/// Run codegen over an explicit list of types. Callers that want the
/// default "all registered types" behavior should pass
/// `registered_types()` themselves.
pub fn run_codegen(types: Vec<ElmTypeInfo>, options: CodegenOptions<'_>) -> Result<CodegenOutcome> {
    let overrides = TypeOverrides::new();
    let types: Vec<ElmTypeInfo> = types.into_iter().map(|t| overrides.apply(t)).collect();
    let mut types = types;
    types.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then_with(|| a.type_name.cmp(b.type_name))
    });

    let names = NameMap::from_types(&types);

    let targets: Vec<ElmTypeInfo> = if options.filter_names.is_empty() {
        types
    } else {
        types
            .into_iter()
            .filter(|t| options.filter_names.iter().any(|n| n == t.type_name))
            .collect()
    };

    if targets.is_empty() {
        anyhow::bail!("No types matched. Did you `use my_schema_crate as _;` in main.rs?");
    }

    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&targets);

    if options.dry_run {
        let mut buffer = String::new();
        for (module_path, group) in &groups {
            let module = build_merged_module(module_path, group, &names, &strategy, &maybe);
            writeln!(
                &mut buffer,
                "-- {}.elm --\n{}\n",
                module_path.join("."),
                elm_ast::pretty_print(&module)
            )
            .expect("writing to String can't fail");
        }
        return Ok(CodegenOutcome::DryRun(buffer));
    }

    for (module_path, group) in &groups {
        let module = build_merged_module(module_path, group, &names, &strategy, &maybe);
        write_module(&options.output, module_path, &module)?;
    }
    Ok(CodegenOutcome::Wrote {
        module_count: groups.len(),
        root: options.output,
    })
}

fn write_module(
    output_dir: &Path,
    module_path: &[&str],
    module: &elm_ast::file::ElmModule,
) -> Result<()> {
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
