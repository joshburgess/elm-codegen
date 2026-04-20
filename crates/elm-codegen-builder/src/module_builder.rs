use std::collections::{BTreeMap, BTreeSet};

use elm_ast::builder::spanned;
use elm_ast::declaration::Declaration;
use elm_ast::exposing::{ExposedItem, Exposing};
use elm_ast::file::ElmModule;
use elm_ast::import::Import;
use elm_ast::module_header::ModuleHeader;
use elm_ast::node::Spanned;
use elm_codegen_core::{ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantPayload};

use crate::request_style::RequestFunctionOutput;

use crate::decoder::{build_decoder, lcfirst};
use crate::encoder::{
    build_encoder, build_merge_tagged_object_helper, needs_merge_tagged_object_helper,
};
use crate::helpers::{import_as_exposing, import_exposing, on_distinct_lines};
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

    for info in types {
        collect_all_reprs(info, &mut all_reprs);
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
    let imports = build_imports(&owned_reprs, needs_encoder, module_path, names, maybe);

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
    current_module: &[&str],
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Vec<Spanned<Import>> {
    let mut imports = vec![
        import_as_exposing(&["Json", "Decode"], "Decode", vec!["Decoder"]),
        import_exposing(
            &["Json", "Decode", "Pipeline"],
            vec!["required", "optional"],
        ),
    ];

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
    for rust_name in &custom_refs {
        let Some(entry) = names.lookup(rust_name) else {
            continue;
        };
        if entry.module_path == current {
            continue;
        }
        let module_refs: Vec<&str> = entry.module_path.iter().map(|s| s.as_str()).collect();
        let decoder = format!("{}Decoder", lcfirst(&entry.elm_name));
        let encoder = format!("encode{}", entry.elm_name);
        let mut exposed = vec![entry.elm_name.as_str(), decoder.as_str()];
        if needs_encoder {
            exposed.push(encoder.as_str());
        }
        imports.push(import_exposing(&module_refs, exposed));
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
        _ => {}
    }
}

fn repr_uses_posix(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Posix => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_posix(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_posix),
        _ => false,
    }
}

fn repr_uses_value(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Value => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_value(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_value),
        _ => false,
    }
}

fn repr_uses_dict(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Dict(_) => true,
        ElmTypeRepr::Maybe(i) | ElmTypeRepr::List(i) => repr_uses_dict(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_dict),
        _ => false,
    }
}

fn repr_uses_maybe(r: &ElmTypeRepr) -> bool {
    match r {
        ElmTypeRepr::Maybe(_) => true,
        ElmTypeRepr::List(i) | ElmTypeRepr::Dict(i) => repr_uses_maybe(i),
        ElmTypeRepr::Tuple(elems) => elems.iter().any(repr_uses_maybe),
        _ => false,
    }
}
