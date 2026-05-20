use std::collections::{BTreeMap, BTreeSet};

use elm_ast::builder::spanned;
use elm_ast::declaration::Declaration;
use elm_ast::exposing::{ExposedItem, Exposing};
use elm_ast::file::ElmModule;
use elm_ast::import::Import;
use elm_ast::module_header::ModuleHeader;
use elm_ast::node::Spanned;
use elm_client_gen_core::{ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantPayload};
use elm_client_gen_http::ElmEndpointInfo;

use crate::request_style::RequestFunctionOutput;

use crate::decoder::{build_decoder, lcfirst};
use crate::encoder::{
    build_encoder, build_merge_tagged_object_helper, needs_merge_tagged_object_helper,
};
use crate::helpers::{import_as, import_as_exposing, import_exposing, on_distinct_lines};
use crate::name_map::NameMap;
use crate::strategy::BuildStrategy;
use crate::type_builder::build_type_declaration;

/// Where to find the `encodeMaybe` helper in the consumer's Elm
/// codebase. The encoder generator emits a call to `function_name`
/// (unqualified), and the import is added automatically.
#[derive(Clone)]
pub struct MaybeEncoderRef {
    pub module_path: Vec<String>,
    pub function_name: String,
}

impl MaybeEncoderRef {
    pub fn new(module_path: Vec<&str>, function_name: &str) -> Self {
        Self {
            module_path: module_path.into_iter().map(String::from).collect(),
            function_name: function_name.to_string(),
        }
    }
}

/// Group `ElmTypeInfo` values by their target module path.
pub fn group_by_module<'a>(
    types: &'a [ElmTypeInfo],
) -> BTreeMap<Vec<&'a str>, Vec<&'a ElmTypeInfo>> {
    let mut groups: BTreeMap<Vec<&'a str>, Vec<&'a ElmTypeInfo>> = BTreeMap::new();
    for info in types {
        let key: Vec<&str> = info.module_path.to_vec();
        groups.entry(key).or_default().push(info);
    }
    groups
}

/// Group `ElmEndpointInfo` values by their target module path, with
/// entries within each module sorted by Elm function name.
///
/// `inventory` yields endpoints in link order, which shifts whenever
/// dependencies change. Sorting here keeps emitted request functions
/// in stable alphabetical order so regenerated output doesn't drift
/// across unrelated rebuilds.
pub fn group_endpoints_by_module<'a>(
    endpoints: &'a [ElmEndpointInfo],
) -> BTreeMap<Vec<&'a str>, Vec<&'a ElmEndpointInfo>> {
    let mut groups: BTreeMap<Vec<&'a str>, Vec<&'a ElmEndpointInfo>> = BTreeMap::new();
    for ep in endpoints {
        groups
            .entry(ep.elm_module_path.to_vec())
            .or_default()
            .push(ep);
    }
    for eps in groups.values_mut() {
        eps.sort_by_key(|ep| ep.elm_function_name);
    }
    groups
}

/// Build a complete Elm module for one group of types that share a
/// module path.
pub fn build_merged_module<S: BuildStrategy>(
    module_path: &[&str],
    types: &[&ElmTypeInfo],
    names: &NameMap,
    strategy: &S,
    maybe: &MaybeEncoderRef,
) -> ElmModule {
    let mut declarations: Vec<Spanned<Declaration>> = Vec::new();
    let mut exposed_items: Vec<Spanned<ExposedItem>> = Vec::new();
    let mut all_reprs: Vec<&ElmTypeRepr> = Vec::new();
    let mut needs_encoder = false;
    let mut needs_merge_tagged_helper = false;
    let mut uses_required = false;
    let mut uses_optional = false;

    for info in types {
        collect_all_reprs(info, &mut all_reprs);
        if strategy.should_emit_decoder(info) {
            collect_pipeline_flags(info, &mut uses_required, &mut uses_optional);
        }
        if strategy.should_emit_encoder(info) && needs_merge_tagged_object_helper(info) {
            needs_merge_tagged_helper = true;
        }

        declarations.push(build_type_declaration(info, names));
        // Enums need to expose their constructors with `(..)` so callers
        // can pattern-match on them. Records expose just the alias name.
        if info.is_enum() {
            exposed_items.push(spanned(ExposedItem::TypeExpose {
                name: info.type_name.to_string(),
                open: Some(elm_ast::span::Span::dummy()),
            }));
        } else {
            exposed_items.push(spanned(ExposedItem::TypeOrAlias(
                info.type_name.to_string(),
            )));
        }

        if strategy.should_emit_decoder(info) {
            let decoder_name = format!("{}Decoder", lcfirst(info.type_name));
            declarations.push(build_decoder(info, names));
            exposed_items.push(spanned(ExposedItem::Function(decoder_name)));
        }

        if strategy.should_emit_encoder(info) {
            let encoder_name = format!("encode{}", info.type_name);
            declarations.push(build_encoder(info, names, maybe));
            exposed_items.push(spanned(ExposedItem::Function(encoder_name)));
            needs_encoder = true;
        }
    }

    if needs_merge_tagged_helper {
        declarations.push(build_merge_tagged_object_helper());
    }

    let module_name: Vec<String> = module_path.iter().map(|s| s.to_string()).collect();
    // Pin each exposed item to its own line so the printer breaks the
    // module exposing list across lines instead of running everything
    // together on one long header line.
    let exposed_items = on_distinct_lines(exposed_items);
    let header = spanned(ModuleHeader::Normal {
        name: spanned(module_name),
        exposing: spanned(Exposing::Explicit {
            items: exposed_items,
            trailing_comments: Vec::new(),
        }),
    });

    let owned_reprs: Vec<ElmTypeRepr> = all_reprs.into_iter().cloned().collect();
    let imports = build_imports(
        &owned_reprs,
        needs_encoder,
        uses_required,
        uses_optional,
        module_path,
        names,
        maybe,
    );

    ElmModule {
        header,
        module_documentation: None,
        imports,
        declarations,
        comments: Vec::new(),
    }
}

fn build_imports(
    reprs: &[ElmTypeRepr],
    needs_encoder: bool,
    uses_required: bool,
    uses_optional: bool,
    current_module: &[&str],
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Vec<Spanned<Import>> {
    // `import Json.Decode as Decode` (no exposing). Every decoder we
    // emit refers to the type as `Decode.Decoder`, so the previously
    // exposed bare `Decoder` was dead.
    let mut imports = vec![import_as(&["Json", "Decode"], "Decode")];

    // Only import Json.Decode.Pipeline when we actually generate a
    // pipeline-style decoder (records or enum struct variants), and
    // only expose the helpers we actually reference.
    if uses_required || uses_optional {
        let mut exposed: Vec<&str> = Vec::new();
        if uses_required {
            exposed.push("required");
        }
        if uses_optional {
            exposed.push("optional");
        }
        imports.push(import_exposing(&["Json", "Decode", "Pipeline"], exposed));
    }

    if needs_encoder || reprs.iter().any(repr_uses_value) {
        imports.push(import_as_exposing(
            &["Json", "Encode"],
            "Encode",
            vec!["Value"],
        ));
    }

    if reprs.iter().any(repr_uses_posix) {
        imports.push(elm_ast::builder::import(vec!["Time"]));
        imports.push(elm_ast::builder::import(vec!["Iso8601"]));
    }

    if reprs.iter().any(repr_uses_dict) {
        imports.push(import_exposing(&["Dict"], vec!["Dict"]));
    }

    let has_maybe = reprs.iter().any(repr_uses_maybe);
    if has_maybe && needs_encoder {
        let module_refs: Vec<&str> = maybe.module_path.iter().map(|s| s.as_str()).collect();
        imports.push(import_exposing(
            &module_refs,
            vec![maybe.function_name.as_str()],
        ));
    }

    let current: Vec<String> = current_module.iter().map(|s| s.to_string()).collect();
    let mut custom_refs: BTreeSet<String> = BTreeSet::new();
    for r in reprs {
        collect_custom_refs(r, &mut custom_refs);
    }

    // Group exposed names by their target module path so multiple Rust
    // types living in the same Elm module collapse into a single
    // `import M exposing (..)` statement. (Without this, registering
    // both `Patch` and `PatchNullable` in the same `Api.Patch` module
    // would emit two duplicate imports of `Api.Patch`.)
    let mut by_module: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
    for rust_name in &custom_refs {
        let Some(entry) = names.lookup(rust_name) else {
            continue;
        };
        if entry.module_path == current {
            continue;
        }
        let bucket = by_module.entry(entry.module_path.clone()).or_default();
        if let Some(overrides) = entry.exposed_overrides.as_ref() {
            for s in overrides {
                bucket.insert(s.clone());
            }
        } else {
            bucket.insert(entry.elm_name.clone());
            bucket.insert(format!("{}Decoder", lcfirst(&entry.elm_name)));
            if needs_encoder {
                bucket.insert(format!("encode{}", entry.elm_name));
            }
        }
    }
    for (module_path, exposed) in &by_module {
        let module_refs: Vec<&str> = module_path.iter().map(|s| s.as_str()).collect();
        let exposed_refs: Vec<&str> = exposed.iter().map(|s| s.as_str()).collect();
        imports.push(import_exposing(&module_refs, exposed_refs));
    }

    imports
}

/// Append a request function (from a [`crate::RequestStyle`]) into an
/// existing module: pushes its declarations onto the end and merges
/// its imports into the module's import list, deduplicating by
/// module name and unioning the `exposing` sets.
///
/// Use this when you want the request function to live in the same
/// `.elm` file as its types/decoders/encoders. The caller is
/// responsible for ensuring the request function's `current_module`
/// matches the target module's path so it doesn't self-import.
pub fn merge_request_into_module(module: &mut ElmModule, output: RequestFunctionOutput) {
    let new_fn_names: Vec<String> = output
        .declarations
        .iter()
        .filter_map(declaration_function_name)
        .collect();

    module.declarations.extend(output.declarations);

    let mut by_name: BTreeMap<Vec<String>, Spanned<Import>> = BTreeMap::new();
    for imp in module.imports.drain(..).chain(output.imports) {
        let key = imp.value.module_name.value.clone();
        by_name
            .entry(key)
            .and_modify(|existing| merge_import(existing, &imp))
            .or_insert(imp);
    }
    module.imports = by_name.into_values().collect();

    add_to_exposing(&mut module.header, &new_fn_names);
}

fn declaration_function_name(decl: &Spanned<Declaration>) -> Option<String> {
    match &decl.value {
        Declaration::FunctionDeclaration(f) => Some(f.declaration.value.name.value.clone()),
        _ => None,
    }
}

fn add_to_exposing(header: &mut Spanned<ModuleHeader>, names: &[String]) {
    let exposing = match &mut header.value {
        ModuleHeader::Normal { exposing, .. } => exposing,
        ModuleHeader::Port { exposing, .. } => exposing,
        ModuleHeader::Effect { exposing, .. } => exposing,
    };
    if let Exposing::Explicit { items, .. } = &mut exposing.value {
        let seen: BTreeSet<String> = items.iter().map(exposed_key).collect();
        // Continue assigning distinct line spans past whatever the
        // initial header pass set, so the multi-line layout the printer
        // chose still applies after the merge.
        let mut next_line = items.iter().map(|i| i.span.end.line).max().unwrap_or(0) + 1;
        for n in names {
            let k = format!("fn:{n}");
            if !seen.contains(&k) {
                let mut item = spanned(ExposedItem::Function(n.clone()));
                let pos = elm_ast::span::Position {
                    offset: 0,
                    line: next_line,
                    column: 1,
                };
                item.span = elm_ast::span::Span::new(pos, pos);
                next_line += 1;
                items.push(item);
            }
        }
    }
}

fn merge_import(into: &mut Spanned<Import>, other: &Spanned<Import>) {
    if into.value.alias.is_none() {
        into.value.alias = other.value.alias.clone();
    }
    if let Some(other_exp) = &other.value.exposing {
        match &mut into.value.exposing {
            None => into.value.exposing = Some(other_exp.clone()),
            Some(existing) => merge_exposing(existing, other_exp),
        }
    }
}

fn merge_exposing(into: &mut Spanned<Exposing>, other: &Spanned<Exposing>) {
    use elm_ast::exposing::Exposing::*;
    match (&mut into.value, &other.value) {
        // `exposing (..)` swallows any explicit list.
        (All(_), _) => {}
        (_, All(_)) => into.value = other.value.clone(),
        (Explicit { items: a, .. }, Explicit { items: b, .. }) => {
            let mut seen: BTreeSet<String> = a.iter().map(exposed_key).collect();
            for it in b {
                let k = exposed_key(it);
                if !seen.contains(&k) {
                    seen.insert(k);
                    a.push(it.clone());
                }
            }
        }
    }
}

fn exposed_key(item: &Spanned<ExposedItem>) -> String {
    match &item.value {
        ExposedItem::Function(n) => format!("fn:{n}"),
        ExposedItem::TypeOrAlias(n) => format!("ty:{n}"),
        ExposedItem::TypeExpose { name, .. } => format!("tyc:{name}"),
        ExposedItem::Infix(n) => format!("op:{n}"),
    }
}

/// Push every `ElmTypeRepr` reachable from this type onto `out`. For
/// records that's the field types; for enums it's the union of every
/// variant's payload type(s). Drives the import-detection helpers
/// below (`repr_uses_posix`, `repr_uses_value`, `repr_uses_dict`,
/// `repr_uses_maybe`) and the custom-ref collector.
fn collect_all_reprs<'a>(info: &'a ElmTypeInfo, out: &mut Vec<&'a ElmTypeRepr>) {
    match &info.kind {
        ElmTypeKind::Record { fields } => {
            for f in fields {
                out.push(&f.elm_type);
            }
        }
        ElmTypeKind::Enum { variants, .. } => {
            for v in variants {
                match &v.payload {
                    ElmVariantPayload::Unit => {}
                    ElmVariantPayload::Newtype(repr) => out.push(repr),
                    ElmVariantPayload::Struct(fields) => {
                        for f in fields {
                            out.push(&f.elm_type);
                        }
                    }
                }
            }
        }
        ElmTypeKind::Newtype { inner } => out.push(inner),
    }
}

fn collect_custom_refs(repr: &ElmTypeRepr, out: &mut BTreeSet<String>) {
    match repr {
        ElmTypeRepr::Custom(name) => {
            out.insert(name.clone());
        }
        ElmTypeRepr::Maybe(inner) | ElmTypeRepr::List(inner) | ElmTypeRepr::Dict(inner) => {
            collect_custom_refs(inner, out)
        }
        ElmTypeRepr::Tuple(elems) => {
            for e in elems {
                collect_custom_refs(e, out);
            }
        }
        // Type applications: the head is treated as a custom reference
        // (so user-supplied wrappers get imported), and each arg is
        // recursively scanned for further refs.
        ElmTypeRepr::App { head, args } => {
            out.insert(head.clone());
            for a in args {
                collect_custom_refs(a, out);
            }
        }
        _ => {}
    }
}

fn repr_uses_posix(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Posix => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_posix(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_posix),
        ElmTypeRepr::App { args, .. } => args.iter().any(repr_uses_posix),
        _ => false,
    }
}

fn repr_uses_value(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Value => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_value(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_value),
        ElmTypeRepr::App { args, .. } => args.iter().any(repr_uses_value),
        _ => false,
    }
}

fn repr_uses_dict(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Dict(_) => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) => repr_uses_dict(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_dict),
        ElmTypeRepr::App { args, .. } => args.iter().any(repr_uses_dict),
        _ => false,
    }
}

fn repr_uses_maybe(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Maybe(_) => true,
        ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_maybe(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_maybe),
        ElmTypeRepr::App { args, .. } => args.iter().any(repr_uses_maybe),
        _ => false,
    }
}

/// Record `Json.Decode.Pipeline` helpers referenced by a type's
/// decoder. `required` is emitted for every non-optional field;
/// `optional` only when at least one field is `Option<_>`. Enum
/// struct variants are included because their decoders also use
/// the pipeline form. Bare-string enums, untagged enums, newtype
/// kinds, and unit variants contribute nothing.
fn collect_pipeline_flags(info: &ElmTypeInfo, uses_required: &mut bool, uses_optional: &mut bool) {
    let note = |fields: &[ElmFieldInfo], uses_required: &mut bool, uses_optional: &mut bool| {
        for field in fields {
            if field.is_optional {
                *uses_optional = true;
            } else {
                *uses_required = true;
            }
        }
    };
    match &info.kind {
        ElmTypeKind::Record { fields } => note(fields, uses_required, uses_optional),
        ElmTypeKind::Enum { variants, .. } => {
            for variant in variants {
                if let ElmVariantPayload::Struct(fields) = &variant.payload {
                    note(fields, uses_required, uses_optional);
                }
            }
        }
        ElmTypeKind::Newtype { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elm_client_gen_http::{HttpMethod, ResponseInfo, ResponseKind};
    use test_better::prelude::*;

    fn endpoint(
        elm_function_name: &'static str,
        elm_module_path: &'static [&'static str],
    ) -> ElmEndpointInfo {
        ElmEndpointInfo {
            handler_name: "handler",
            elm_function_name,
            elm_module_path,
            method: HttpMethod::Get,
            path_template: "/",
            params: vec![],
            response: ResponseInfo {
                success: None,
                success_kind: ResponseKind::Empty,
                error: None,
            },
            tags: &[],
        }
    }

    // ── Pipeline import shape ────────────────────────────────────
    use crate::strategy::DefaultStrategy;
    use elm_client_gen_core::{
        ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
        EnumRepresentation,
    };

    fn render_pipeline_line(info: ElmTypeInfo) -> Option<String> {
        let types = vec![info];
        let names = NameMap::from_types(&types);
        let strategy = DefaultStrategy;
        let maybe = MaybeEncoderRef::new(vec!["Api", "Encode"], "encodeMaybe");
        let refs: Vec<&ElmTypeInfo> = types.iter().collect();
        let module = build_merged_module(&["Api", "M"], &refs, &names, &strategy, &maybe);
        elm_ast::pretty_print(&module)
            .lines()
            .find(|line| line.contains("Json.Decode.Pipeline"))
            .map(|s| s.to_string())
    }

    fn record(fields: Vec<ElmFieldInfo>) -> ElmTypeInfo {
        ElmTypeInfo {
            rust_name: "R",
            module_path: vec!["Api", "M"],
            type_name: "R",
            tags: vec![],
            kind: ElmTypeKind::Record { fields },
        }
    }

    fn field(name: &'static str, ty: ElmTypeRepr, is_optional: bool) -> ElmFieldInfo {
        ElmFieldInfo {
            rust_name: name,
            elm_name: name,
            elm_type: ty,
            is_optional,
            custom_decoder: None,
            custom_encoder: None,
            decoder_step: None,
            encoder_pairs: None,
        }
    }

    #[test]
    fn pipeline_import_exposes_only_required_when_no_optional_fields() -> TestResult {
        let info = record(vec![
            field("id", ElmTypeRepr::String, false),
            field("name", ElmTypeRepr::String, false),
        ]);
        let line = render_pipeline_line(info).or_fail_with("pipeline import present")?;
        check!(line.contains("exposing (required)"))
            .satisfies(is_true())
            .context(format!("expected `exposing (required)` only, got: {line}"))?;
        check!(line.contains("optional")).satisfies(is_false())?;
        Ok(())
    }

    #[test]
    fn pipeline_import_exposes_both_when_any_optional_field() -> TestResult {
        let info = record(vec![
            field("id", ElmTypeRepr::String, false),
            field(
                "nickname",
                ElmTypeRepr::Maybe(Box::new(ElmTypeRepr::String)),
                true,
            ),
        ]);
        let line = render_pipeline_line(info).or_fail_with("pipeline import present")?;
        check!(line.contains("required"))
            .satisfies(is_true())
            .context(line.clone())?;
        check!(line.contains("optional"))
            .satisfies(is_true())
            .context(line)?;
        Ok(())
    }

    #[test]
    fn pipeline_import_exposes_only_optional_when_all_fields_optional() -> TestResult {
        let info = record(vec![field(
            "nickname",
            ElmTypeRepr::Maybe(Box::new(ElmTypeRepr::String)),
            true,
        )]);
        let line = render_pipeline_line(info).or_fail_with("pipeline import present")?;
        check!(line.contains("exposing (optional)"))
            .satisfies(is_true())
            .context(format!("expected `exposing (optional)` only, got: {line}"))?;
        check!(line.contains("required")).satisfies(is_false())?;
        Ok(())
    }

    #[test]
    fn pipeline_import_omitted_for_bare_string_enum_only_module() {
        let info = ElmTypeInfo {
            rust_name: "Status",
            module_path: vec!["Api", "M"],
            type_name: "Status",
            tags: vec![],
            kind: ElmTypeKind::Enum {
                representation: EnumRepresentation::BareString,
                variants: vec![
                    ElmVariantInfo {
                        rust_name: "Active",
                        elm_name: "Active",
                        json_tag: "active",
                        payload: ElmVariantPayload::Unit,
                    },
                    ElmVariantInfo {
                        rust_name: "Archived",
                        elm_name: "Archived",
                        json_tag: "archived",
                        payload: ElmVariantPayload::Unit,
                    },
                ],
            },
        };
        assert!(
            render_pipeline_line(info).is_none(),
            "bare-string-only module should not import Json.Decode.Pipeline"
        );
    }

    #[test]
    fn pipeline_import_omitted_for_newtype_only_module() {
        let info = ElmTypeInfo {
            rust_name: "UserId",
            module_path: vec!["Api", "M"],
            type_name: "UserId",
            tags: vec![],
            kind: ElmTypeKind::Newtype {
                inner: ElmTypeRepr::String,
            },
        };
        assert!(
            render_pipeline_line(info).is_none(),
            "newtype-only module should not import Json.Decode.Pipeline"
        );
    }

    // ── group_endpoints_by_module ─────────────────────────────────

    // ── Import emission for App-typed fields ──────────────────────
    //
    // When a record field's repr is `App { head: "Patch", .. }`,
    // `collect_custom_refs` records `Patch` and `build_imports` looks
    // it up in the NameMap. With `register_with_exposed`, the import
    // line uses the registered exposing list verbatim instead of the
    // auto-derived `<elm_name> / <elm_name>Decoder / encode<elm_name>`.

    fn record_with_app_field(head: &'static str, arg: ElmTypeRepr) -> ElmTypeInfo {
        ElmTypeInfo {
            rust_name: "R",
            module_path: vec!["Api", "M"],
            type_name: "R",
            tags: vec![],
            kind: ElmTypeKind::Record {
                fields: vec![field(
                    "f",
                    ElmTypeRepr::App {
                        head: head.to_string(),
                        args: vec![arg],
                    },
                    false,
                )],
            },
        }
    }

    fn render_with(types: Vec<ElmTypeInfo>, names: NameMap) -> String {
        let strategy = DefaultStrategy;
        let maybe = MaybeEncoderRef::new(vec!["Api", "Encode"], "encodeMaybe");
        let refs: Vec<&ElmTypeInfo> = types.iter().collect();
        let module = build_merged_module(&["Api", "M"], &refs, &names, &strategy, &maybe);
        elm_ast::pretty_print(&module)
    }

    #[test]
    fn imports_use_exposed_overrides_when_set() -> TestResult {
        let types = vec![record_with_app_field("Patch", ElmTypeRepr::String)];
        let mut names = NameMap::from_types(&types);
        names.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "Patch".into()],
            vec!["Patch".into(), "patch".into(), "patchPair".into()],
        );
        let rendered = render_with(types, names);
        let import = rendered
            .lines()
            .find(|l| l.contains("import Api.Patch"))
            .or_fail_with(format!("expected `import Api.Patch ...`:\n{rendered}"))?
            .to_string();
        check!(import.contains("Patch"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("patch"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("patchPair"))
            .satisfies(is_true())
            .context(import.clone())?;
        // Auto-derived names that the override list doesn't include
        // must NOT leak through.
        check!(import.contains("patchDecoder"))
            .satisfies(is_false())
            .context(format!(
                "auto-derived patchDecoder should not appear when overrides are set:\n{import}"
            ))?;
        check!(import.contains("encodePatch"))
            .satisfies(is_false())
            .context(format!(
                "auto-derived encodePatch should not appear when overrides are set:\n{import}"
            ))?;
        Ok(())
    }

    #[test]
    fn imports_fall_back_to_auto_names_without_overrides() -> TestResult {
        // Without `exposed_overrides`, the import exposes the
        // auto-derived triple `<elm_name>`, `<elm_name>Decoder`,
        // `encode<elm_name>` (the last only if the module emits any
        // encoder, which a record always does).
        let types = vec![record_with_app_field("Wrap", ElmTypeRepr::String)];
        let mut names = NameMap::from_types(&types);
        names.register("Wrap", "Wrap", vec!["Api".into(), "Wrap".into()]);
        let rendered = render_with(types, names);
        let import = rendered
            .lines()
            .find(|l| l.contains("import Api.Wrap"))
            .or_fail_with(format!("expected `import Api.Wrap ...`:\n{rendered}"))?
            .to_string();
        check!(import.contains("Wrap"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("wrapDecoder"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("encodeWrap"))
            .satisfies(is_true())
            .context(import)?;
        Ok(())
    }

    #[test]
    fn imports_skip_self_references_to_current_module() {
        // A record in `Api.M` that references another type in the same
        // module path must not emit `import Api.M ...` for itself.
        let types = vec![record_with_app_field("Patch", ElmTypeRepr::String)];
        let mut names = NameMap::from_types(&types);
        names.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "M".into()],
            vec!["Patch".into(), "patch".into()],
        );
        let rendered = render_with(types, names);
        assert!(
            !rendered.contains("import Api.M"),
            "self-import should be suppressed:\n{rendered}",
        );
    }

    #[test]
    fn imports_collapse_multiple_app_refs_into_one_per_module() -> TestResult {
        // Two fields, each referencing a different wrapper type that
        // happen to live in the *same* Elm module. They must merge
        // into a single import with a unioned exposing list.
        let info = ElmTypeInfo {
            rust_name: "R",
            module_path: vec!["Api", "M"],
            type_name: "R",
            tags: vec![],
            kind: ElmTypeKind::Record {
                fields: vec![
                    field(
                        "f1",
                        ElmTypeRepr::App {
                            head: "Patch".into(),
                            args: vec![ElmTypeRepr::String],
                        },
                        false,
                    ),
                    field(
                        "f2",
                        ElmTypeRepr::App {
                            head: "PatchNullable".into(),
                            args: vec![ElmTypeRepr::Int],
                        },
                        false,
                    ),
                ],
            },
        };
        let types = vec![info];
        let mut names = NameMap::from_types(&types);
        names.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "Patch".into()],
            vec!["Patch".into(), "patch".into()],
        );
        names.register_with_exposed(
            "PatchNullable",
            "PatchNullable",
            vec!["Api".into(), "Patch".into()],
            vec!["PatchNullable".into(), "patchNullable".into()],
        );
        let rendered = render_with(types, names);
        let count = rendered.matches("import Api.Patch ").count()
            + rendered.matches("import Api.Patch\n").count();
        check!(count).satisfies(eq(1)).context(format!(
            "expected exactly one `import Api.Patch ...`:\n{rendered}"
        ))?;
        let import = rendered
            .lines()
            .find(|l| l.contains("import Api.Patch"))
            .or_fail_with("import line")?
            .to_string();
        check!(import.contains("Patch"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("patch"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("PatchNullable"))
            .satisfies(is_true())
            .context(import.clone())?;
        check!(import.contains("patchNullable"))
            .satisfies(is_true())
            .context(import)?;
        Ok(())
    }

    #[test]
    fn group_endpoints_by_module_partitions_and_sorts_alphabetically() -> TestResult {
        let endpoints = vec![
            endpoint("updateDashboard", &["Api", "Generated", "Dashboard"]),
            endpoint("listPeople", &["Api", "Generated", "People"]),
            endpoint("createDashboard", &["Api", "Generated", "Dashboard"]),
            endpoint("deleteDashboard", &["Api", "Generated", "Dashboard"]),
            endpoint("createPerson", &["Api", "Generated", "People"]),
        ];
        let groups = group_endpoints_by_module(&endpoints);
        let dashboard = groups
            .get(&vec!["Api", "Generated", "Dashboard"])
            .or_fail_with("dashboard group")?;
        let people = groups
            .get(&vec!["Api", "Generated", "People"])
            .or_fail_with("people group")?;
        check!(dashboard
            .iter()
            .map(|e| e.elm_function_name)
            .collect::<Vec<_>>())
        .satisfies(eq(vec![
            "createDashboard",
            "deleteDashboard",
            "updateDashboard",
        ]))?;
        check!(people
            .iter()
            .map(|e| e.elm_function_name)
            .collect::<Vec<_>>())
        .satisfies(eq(vec!["createPerson", "listPeople"]))?;
        Ok(())
    }
}
