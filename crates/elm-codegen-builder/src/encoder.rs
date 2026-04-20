use elm_ast::builder::{
    app, func_with_sig, lambda, pctor, pvar, spanned, string, tname, tuple, var,
};
use elm_ast::expr::{CaseBranch, Expr};
use elm_ast::node::Spanned;
use elm_ast::pattern::Pattern;
use elm_codegen_core::{
    ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
    EnumRepresentation,
};

use crate::helpers::{list_multiline, record_access};
use crate::module_builder::MaybeEncoderRef;
use crate::name_map::NameMap;

/// Name of the module-local helper that prepends an internally-tagged
/// discriminator to the JSON object produced by a newtype-variant
/// inner encoder. Emitted only in modules that need it.
pub const MERGE_TAGGED_OBJECT_HELPER: &str = "mergeTaggedObject";

/// Returns `true` if any variant on `info` is a newtype under an
/// internally-tagged representation. The encoder emits those as calls
/// to [`MERGE_TAGGED_OBJECT_HELPER`], so a module with such an info
/// needs the helper declaration.
pub fn needs_merge_tagged_object_helper(info: &ElmTypeInfo) -> bool {
    let ElmTypeKind::Enum {
        variants,
        representation: EnumRepresentation::InternallyTagged { .. },
    } = &info.kind
    else {
        return false;
    };
    variants
        .iter()
        .any(|v| matches!(v.payload, ElmVariantPayload::Newtype(_)))
}

/// Build the declaration for [`MERGE_TAGGED_OBJECT_HELPER`]. The helper
/// decodes the inner object back into key-value pairs, prepends the
/// tag, and re-encodes. Requires `Json.Decode as Decode` and
/// `Json.Encode as Encode` imports (already added when any encoder or
/// decoder is emitted in the module).
pub fn build_merge_tagged_object_helper() -> Spanned<elm_ast::declaration::Declaration> {
    let tag_key_var = "tagKey";
    let tag_value_var = "tagValue";
    let inner_var = "inner";

    let decoded_pairs = app(
        elm_ast::builder::qualified(&["Decode"], "decodeValue"),
        vec![
            app(
                elm_ast::builder::qualified(&["Decode"], "keyValuePairs"),
                vec![elm_ast::builder::qualified(&["Decode"], "value")],
            ),
            var(inner_var),
        ],
    );
    let pairs_with_default = app(
        elm_ast::builder::qualified(&["Result"], "withDefault"),
        vec![elm_ast::builder::list(vec![]), decoded_pairs],
    );

    let tag_pair = tuple(vec![
        var(tag_key_var),
        app(
            elm_ast::builder::qualified(&["Encode"], "string"),
            vec![var(tag_value_var)],
        ),
    ]);

    let combined_list = elm_ast::builder::binop("::", tag_pair, pairs_with_default);
    let body = app(
        elm_ast::builder::qualified(&["Encode"], "object"),
        vec![combined_list],
    );

    let sig = elm_ast::builder::tfunc(
        tname("String", vec![]),
        elm_ast::builder::tfunc(
            tname("String", vec![]),
            elm_ast::builder::tfunc(tname("Value", vec![]), tname("Value", vec![])),
        ),
    );
    func_with_sig(
        MERGE_TAGGED_OBJECT_HELPER,
        vec![pvar(tag_key_var), pvar(tag_value_var), pvar(inner_var)],
        body,
        sig,
    )
}

/// Build an encoder function declaration. Dispatches on `info.kind`
/// and, for enums, on the wire representation.
pub fn build_encoder(
    info: &ElmTypeInfo,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::declaration::Declaration> {
    match &info.kind {
        ElmTypeKind::Record { fields } => build_record_encoder(info, fields, names, maybe),
        ElmTypeKind::Enum {
            variants,
            representation,
        } => match representation {
            EnumRepresentation::BareString => build_bare_string_enum_encoder(info, variants),
            EnumRepresentation::InternallyTagged { tag_key } => {
                build_tagged_enum_encoder(info, variants, tag_key, names, maybe)
            }
            EnumRepresentation::Untagged => {
                build_untagged_enum_encoder(info, variants, names, maybe)
            }
        },
        ElmTypeKind::Newtype { inner } => build_newtype_encoder(info, inner, names, maybe),
    }
}

/// `encodeName value = <inner-encoder> value` — type alias is
/// transparent so the inner encoder accepts the alias value directly.
fn build_newtype_encoder(
    info: &ElmTypeInfo,
    inner: &ElmTypeRepr,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::declaration::Declaration> {
    let encoder_name = format!("encode{}", info.type_name);
    let param = "value";
    let body = encoder_for_type(inner, var(param), names, maybe);
    let sig = elm_ast::builder::tfunc(tname(info.type_name, vec![]), tname("Value", vec![]));
    func_with_sig(&encoder_name, vec![pvar(param)], body, sig)
}

fn build_record_encoder(
    info: &ElmTypeInfo,
    fields: &[ElmFieldInfo],
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::declaration::Declaration> {
    let encoder_name = format!("encode{}", info.type_name);
    let param = "value";

    let field_pairs: Vec<Spanned<Expr>> = fields
        .iter()
        .map(|f| build_field_pair(f, var(param), names, maybe))
        .collect();

    let body = app(
        elm_ast::builder::qualified(&["Encode"], "object"),
        vec![list_multiline(field_pairs)],
    );

    let sig = elm_ast::builder::tfunc(tname(info.type_name, vec![]), tname("Value", vec![]));

    func_with_sig(&encoder_name, vec![pvar(param)], body, sig)
}

/// `encode<TypeName> v = case v of <Variant> -> Encode.string "<json_tag>" ...`
///
/// Used for bare-string enums (no internal tag key, every variant unit).
fn build_bare_string_enum_encoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
) -> Spanned<elm_ast::declaration::Declaration> {
    let encoder_name = format!("encode{}", info.type_name);
    let param = "value";

    let branches: Vec<CaseBranch> = variants
        .iter()
        .map(|v| CaseBranch {
            pattern: pctor(v.elm_name, vec![]),
            body: app(
                elm_ast::builder::qualified(&["Encode"], "string"),
                vec![string(v.json_tag)],
            ),
        })
        .collect();

    let body = spanned(Expr::CaseOf {
        expr: Box::new(var(param)),
        branches,
    });

    let sig = elm_ast::builder::tfunc(tname(info.type_name, vec![]), tname("Value", vec![]));
    func_with_sig(&encoder_name, vec![pvar(param)], body, sig)
}

/// Encoder for an internally-tagged enum (`#[serde(tag = "<key>")]`).
///
/// The shape is:
///
/// ```elm
/// encodeFoo : Foo -> Encode.Value
/// encodeFoo value =
///     case value of
///         VariantA payload ->
///             Encode.object
///                 [ ("<key>", Encode.string "VariantA")
///                 , ("field1", ...payload.field1...)
///                 , ("field2", ...payload.field2...)
///                 ]
///         UnitVariant ->
///             Encode.object [ ("<key>", Encode.string "UnitVariant") ]
/// ```
fn build_tagged_enum_encoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
    tag_key: &str,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::declaration::Declaration> {
    let encoder_name = format!("encode{}", info.type_name);
    let param = "value";

    let branches: Vec<CaseBranch> = variants
        .iter()
        .map(|v| build_tagged_variant_branch(v, tag_key, names, maybe))
        .collect();

    let body = spanned(Expr::CaseOf {
        expr: Box::new(var(param)),
        branches,
    });

    let sig = elm_ast::builder::tfunc(tname(info.type_name, vec![]), tname("Value", vec![]));
    func_with_sig(&encoder_name, vec![pvar(param)], body, sig)
}

fn build_tagged_variant_branch(
    v: &ElmVariantInfo,
    tag_key: &str,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> CaseBranch {
    let tag_pair = tuple(vec![
        string(tag_key),
        app(
            elm_ast::builder::qualified(&["Encode"], "string"),
            vec![string(v.json_tag)],
        ),
    ]);

    let fields = match &v.payload {
        ElmVariantPayload::Unit => {
            let body = app(
                elm_ast::builder::qualified(&["Encode"], "object"),
                vec![list_multiline(vec![tag_pair])],
            );
            return CaseBranch {
                pattern: pctor(v.elm_name, vec![]),
                body,
            };
        }
        ElmVariantPayload::Struct(fs) => fs,
        // Newtype payload: inner must serialize to a JSON object. The
        // outer tag is prepended to that object via the per-module
        // `mergeTaggedObject` helper emitted by `module_builder`.
        ElmVariantPayload::Newtype(inner) => {
            let inner_var = "inner";
            let inner_encoded = encoder_for_type(inner, var(inner_var), names, maybe);
            let body = app(
                var(MERGE_TAGGED_OBJECT_HELPER),
                vec![string(tag_key), string(v.json_tag), inner_encoded],
            );
            return CaseBranch {
                pattern: pctor(v.elm_name, vec![pvar(inner_var)]),
                body,
            };
        }
    };

    let payload_var = "payload";
    let mut pairs: Vec<Spanned<Expr>> = Vec::with_capacity(fields.len() + 1);
    pairs.push(tag_pair);
    for f in fields {
        pairs.push(build_field_pair(f, var(payload_var), names, maybe));
    }
    let body = app(
        elm_ast::builder::qualified(&["Encode"], "object"),
        vec![list_multiline(pairs)],
    );

    CaseBranch {
        pattern: pctor(v.elm_name, vec![pvar(payload_var)]),
        body,
    }
}

/// Encoder for an untagged enum (`#[serde(untagged)]`).
///
/// The shape is:
///
/// ```elm
/// encodeFoo : Foo -> Encode.Value
/// encodeFoo value =
///     case value of
///         VariantA inner -> <encoder-for-inner> inner
///         VariantB { f1, f2 } -> Encode.object [ ("f1", ...), ("f2", ...) ]
///         UnitVariant -> Encode.null
/// ```
fn build_untagged_enum_encoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::declaration::Declaration> {
    let encoder_name = format!("encode{}", info.type_name);
    let param = "value";

    let branches: Vec<CaseBranch> = variants
        .iter()
        .map(|v| build_untagged_variant_branch(v, names, maybe))
        .collect();

    let body = spanned(Expr::CaseOf {
        expr: Box::new(var(param)),
        branches,
    });

    let sig = elm_ast::builder::tfunc(tname(info.type_name, vec![]), tname("Value", vec![]));
    func_with_sig(&encoder_name, vec![pvar(param)], body, sig)
}

fn build_untagged_variant_branch(
    v: &ElmVariantInfo,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> CaseBranch {
    match &v.payload {
        ElmVariantPayload::Unit => CaseBranch {
            pattern: pctor(v.elm_name, vec![]),
            body: elm_ast::builder::qualified(&["Encode"], "null"),
        },
        ElmVariantPayload::Newtype(inner) => {
            let inner_var = "inner";
            let body = encoder_for_type(inner, var(inner_var), names, maybe);
            CaseBranch {
                pattern: pctor(v.elm_name, vec![pvar(inner_var)]),
                body,
            }
        }
        ElmVariantPayload::Struct(fields) => {
            let payload_var = "payload";
            let pairs: Vec<Spanned<Expr>> = fields
                .iter()
                .map(|f| build_field_pair(f, var(payload_var), names, maybe))
                .collect();
            let body = app(
                elm_ast::builder::qualified(&["Encode"], "object"),
                vec![list_multiline(pairs)],
            );
            CaseBranch {
                pattern: pctor(v.elm_name, vec![pvar(payload_var)]),
                body,
            }
        }
    }
}

/// `("<rust_name>", <encoder_for_type> <record>.<elm_name>)` pair used
/// inside an `Encode.object` list. Shared between record encoders (where
/// `record` is `value`) and tagged variant branches (where `record` is
/// `payload`).
///
/// `#[elm(encoder = "...")]` substitutes the type-driven encoder with
/// the given Elm function applied to the field accessor:
///   `("rust_name", customEncoder record.fieldName)`.
fn build_field_pair(
    f: &ElmFieldInfo,
    record: Spanned<Expr>,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<Expr> {
    let accessor = record_access(record, f.elm_name);
    let encoded = if let Some(custom) = f.custom_encoder {
        app(var(custom), vec![accessor])
    } else {
        encoder_for_type(&f.elm_type, accessor, names, maybe)
    };
    tuple(vec![string(f.rust_name), encoded])
}

fn encoder_for_type(
    repr: &ElmTypeRepr,
    accessor: Spanned<elm_ast::expr::Expr>,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::expr::Expr> {
    match repr {
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => app(
            elm_ast::builder::qualified(&["Encode"], "string"),
            vec![accessor],
        ),
        ElmTypeRepr::Int => app(
            elm_ast::builder::qualified(&["Encode"], "int"),
            vec![accessor],
        ),
        ElmTypeRepr::Float => app(
            elm_ast::builder::qualified(&["Encode"], "float"),
            vec![accessor],
        ),
        ElmTypeRepr::Bool => app(
            elm_ast::builder::qualified(&["Encode"], "bool"),
            vec![accessor],
        ),
        ElmTypeRepr::Posix => app(
            elm_ast::builder::qualified(&["Iso8601"], "encode"),
            vec![accessor],
        ),
        ElmTypeRepr::Value => accessor,
        ElmTypeRepr::Maybe(inner) => {
            let inner_encoder = encoder_fn_for_type(inner, names, maybe);
            app(maybe_encoder_var(maybe), vec![inner_encoder, accessor])
        }
        ElmTypeRepr::List(inner) => {
            let inner_encoder = encoder_fn_for_type(inner, names, maybe);
            app(
                elm_ast::builder::qualified(&["Encode"], "list"),
                vec![inner_encoder, accessor],
            )
        }
        ElmTypeRepr::Dict(inner) => {
            let inner_encoder = encoder_fn_for_type(inner, names, maybe);
            app(
                elm_ast::builder::qualified(&["Encode"], "dict"),
                vec![
                    elm_ast::builder::qualified(&["Basics"], "identity"),
                    inner_encoder,
                    accessor,
                ],
            )
        }
        ElmTypeRepr::Tuple(elems) => {
            app(build_tuple_encoder_fn(elems, names, maybe), vec![accessor])
        }
        ElmTypeRepr::Custom(rust_name) => {
            let elm_name = names.resolve(rust_name);
            app(var(format!("encode{}", elm_name)), vec![accessor])
        }
    }
}

/// `\( a, b ) -> Encode.list identity [ encA a, encB b ]` — the standalone
/// encoder function for a tuple type. Used both by the accessor path
/// (applied to a value) and by the function-position path (passed as
/// an `inner` encoder to `Encode.list`/`Encode.dict` at outer arity).
fn build_tuple_encoder_fn(
    elems: &[ElmTypeRepr],
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<Expr> {
    let arity = elems.len();
    debug_assert!(
        arity == 2 || arity == 3,
        "tuple arity {} should have been rejected at derive time",
        arity,
    );
    let names_chars: &[&str] = if arity == 2 {
        &["a", "b"]
    } else {
        &["a", "b", "c"]
    };
    let pattern_elems: Vec<Spanned<Pattern>> = names_chars.iter().map(|n| pvar(*n)).collect();
    let tuple_pattern = spanned(Pattern::Tuple(pattern_elems));

    let encoded_elements: Vec<Spanned<Expr>> = elems
        .iter()
        .zip(names_chars.iter())
        .map(|(e, n)| encoder_for_type(e, var(*n), names, maybe))
        .collect();
    let body = app(
        elm_ast::builder::qualified(&["Encode"], "list"),
        vec![
            elm_ast::builder::qualified(&["Basics"], "identity"),
            list_multiline(encoded_elements),
        ],
    );

    lambda(vec![tuple_pattern], body)
}

fn encoder_fn_for_type(
    repr: &ElmTypeRepr,
    names: &NameMap,
    maybe: &MaybeEncoderRef,
) -> Spanned<elm_ast::expr::Expr> {
    match repr {
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => {
            elm_ast::builder::qualified(&["Encode"], "string")
        }
        ElmTypeRepr::Int => elm_ast::builder::qualified(&["Encode"], "int"),
        ElmTypeRepr::Float => elm_ast::builder::qualified(&["Encode"], "float"),
        ElmTypeRepr::Bool => elm_ast::builder::qualified(&["Encode"], "bool"),
        ElmTypeRepr::Posix => elm_ast::builder::qualified(&["Iso8601"], "encode"),
        ElmTypeRepr::Value => elm_ast::builder::qualified(&["Basics"], "identity"),
        ElmTypeRepr::Custom(rust_name) => {
            let elm_name = names.resolve(rust_name);
            var(format!("encode{}", elm_name))
        }
        ElmTypeRepr::Maybe(inner) => app(
            maybe_encoder_var(maybe),
            vec![encoder_fn_for_type(inner, names, maybe)],
        ),
        ElmTypeRepr::List(inner) => app(
            elm_ast::builder::qualified(&["Encode"], "list"),
            vec![encoder_fn_for_type(inner, names, maybe)],
        ),
        ElmTypeRepr::Dict(inner) => app(
            elm_ast::builder::qualified(&["Encode"], "dict"),
            vec![
                elm_ast::builder::qualified(&["Basics"], "identity"),
                encoder_fn_for_type(inner, names, maybe),
            ],
        ),
        ElmTypeRepr::Tuple(elems) => build_tuple_encoder_fn(elems, names, maybe),
    }
}

fn maybe_encoder_var(maybe: &MaybeEncoderRef) -> Spanned<elm_ast::expr::Expr> {
    var(maybe.function_name.as_str())
}
