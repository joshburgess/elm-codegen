use elm_ast::builder::{custom_type, spanned, tname};
use elm_ast::node::Spanned;
use elm_ast::type_annotation::TypeAnnotation;
use elm_client_gen_core::{
    ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
};

use crate::helpers::{tqualified, trecord};
use crate::name_map::NameMap;

fn build_type_annotation(repr: &ElmTypeRepr, names: &NameMap) -> Spanned<TypeAnnotation> {
    match repr {
        ElmTypeRepr::String => tname("String", vec![]),
        ElmTypeRepr::Int => tname("Int", vec![]),
        ElmTypeRepr::Float => tname("Float", vec![]),
        ElmTypeRepr::Bool => tname("Bool", vec![]),
        ElmTypeRepr::Posix => tqualified(&["Time"], "Posix", vec![]),
        ElmTypeRepr::IsoDate => tname("String", vec![]),
        ElmTypeRepr::Value => tqualified(&["Encode"], "Value", vec![]),
        ElmTypeRepr::Maybe(inner) => tname("Maybe", vec![build_type_annotation(inner, names)]),
        ElmTypeRepr::List(inner) => tname("List", vec![build_type_annotation(inner, names)]),
        ElmTypeRepr::Dict(inner) => tname(
            "Dict",
            vec![tname("String", vec![]), build_type_annotation(inner, names)],
        ),
        ElmTypeRepr::Tuple(elems) => spanned(TypeAnnotation::Tupled(
            elems
                .iter()
                .map(|e| build_type_annotation(e, names))
                .collect(),
        )),
        ElmTypeRepr::Custom(rust_name) => {
            let elm_name = names.resolve(rust_name);
            tname(elm_name, vec![])
        }
    }
}

/// Build either a `type alias` (records / newtypes) or a `type`
/// declaration (enums) from `ElmTypeInfo`. Dispatches on `info.kind`.
pub fn build_type_declaration(
    info: &ElmTypeInfo,
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    match &info.kind {
        ElmTypeKind::Record { fields } => build_record_alias(info, fields, names),
        ElmTypeKind::Enum { variants, .. } => build_custom_type(info, variants, names),
        ElmTypeKind::Newtype { inner } => build_newtype_alias(info, inner, names),
    }
}

/// `type alias UserId = String` for `struct UserId(String);`. Elm
/// treats type aliases transparently, so callers can keep using the
/// named type without any wrapping/unwrapping ceremony.
fn build_newtype_alias(
    info: &ElmTypeInfo,
    inner: &ElmTypeRepr,
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let annotation = build_type_annotation(inner, names);
    elm_ast::builder::type_alias(info.type_name, Vec::<String>::new(), annotation)
}

fn build_record_alias(
    info: &ElmTypeInfo,
    fields: &[ElmFieldInfo],
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let fields: Vec<(&str, Spanned<TypeAnnotation>)> = fields
        .iter()
        .map(|f| (f.elm_name, build_type_annotation(&f.elm_type, names)))
        .collect();

    elm_ast::builder::type_alias(info.type_name, Vec::<String>::new(), trecord(fields))
}

fn build_custom_type(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let ctors: Vec<(String, Vec<Spanned<TypeAnnotation>>)> = variants
        .iter()
        .map(|v| {
            let args: Vec<Spanned<TypeAnnotation>> = match &v.payload {
                ElmVariantPayload::Unit => Vec::new(),
                ElmVariantPayload::Newtype(inner) => {
                    vec![build_type_annotation(inner, names)]
                }
                ElmVariantPayload::Struct(fields) => {
                    let fields: Vec<(&str, Spanned<TypeAnnotation>)> = fields
                        .iter()
                        .map(|f| (f.elm_name, build_type_annotation(&f.elm_type, names)))
                        .collect();
                    vec![trecord(fields)]
                }
            };
            (v.elm_name.to_string(), args)
        })
        .collect();
    custom_type(info.type_name, Vec::<String>::new(), ctors)
}
