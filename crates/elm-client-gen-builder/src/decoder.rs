use elm_ast::builder::{app, func_with_sig, int, lambda, pvar, spanned, string, tname, tuple, var};
use elm_ast::expr::{CaseBranch, Expr, RecordSetter};
use elm_ast::literal::Literal;
use elm_ast::node::Spanned;
use elm_ast::pattern::Pattern;
use elm_client_gen_core::{
    ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
    EnumRepresentation,
};

use crate::helpers::{pipeline_chain, tqualified};
use crate::name_map::NameMap;

/// Build a decoder function declaration for the given type info.
/// Dispatches on `info.kind` and, for enums, on the wire
/// representation.
pub fn build_decoder(
    info: &ElmTypeInfo,
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    match &info.kind {
        ElmTypeKind::Record { fields } => build_record_decoder(info, fields, names),
        ElmTypeKind::Enum {
            variants,
            representation,
        } => match representation {
            EnumRepresentation::BareString => build_bare_string_enum_decoder(info, variants),
            EnumRepresentation::InternallyTagged { tag_key } => {
                build_tagged_enum_decoder(info, variants, tag_key, names)
            }
            EnumRepresentation::Untagged => build_untagged_enum_decoder(info, variants, names),
        },
        ElmTypeKind::Newtype { inner } => build_newtype_decoder(info, inner, names),
    }
}

/// `nameDecoder = innerDecoder` — Elm `type alias` is transparent so
/// the inner decoder produces a value of the alias type directly.
fn build_newtype_decoder(
    info: &ElmTypeInfo,
    inner: &ElmTypeRepr,
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let decoder_name = format!("{}Decoder", lcfirst(info.type_name));
    let body = decoder_for_type(inner, names);
    let sig = tqualified(&["Decode"], "Decoder", vec![tname(info.type_name, vec![])]);
    func_with_sig(&decoder_name, vec![], body, sig)
}

fn build_record_decoder(
    info: &ElmTypeInfo,
    fields: &[ElmFieldInfo],
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let decoder_name = format!("{}Decoder", lcfirst(info.type_name));

    let seed = app(
        elm_ast::builder::qualified(&["Decode"], "succeed"),
        vec![var(info.type_name)],
    );
    let steps: Vec<_> = fields
        .iter()
        .map(|field| build_field_decoder_step(field, names))
        .collect();
    let expr = pipeline_chain(seed, steps);

    let sig = tqualified(&["Decode"], "Decoder", vec![tname(info.type_name, vec![])]);

    func_with_sig(&decoder_name, vec![], expr, sig)
}

/// `<typeName>Decoder = Decode.string |> Decode.andThen (\s -> case s of ...)`.
///
/// Used for enums that have no internal tag key (every variant is a unit
/// variant serialized as a bare string).
fn build_bare_string_enum_decoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
) -> Spanned<elm_ast::declaration::Declaration> {
    let decoder_name = format!("{}Decoder", lcfirst(info.type_name));

    let mut branches: Vec<CaseBranch> = Vec::with_capacity(variants.len() + 1);
    for v in variants {
        branches.push(CaseBranch {
            pattern: spanned(Pattern::Literal(Literal::String(v.json_tag.to_string()))),
            body: app(
                elm_ast::builder::qualified(&["Decode"], "succeed"),
                vec![var(v.elm_name)],
            ),
        });
    }
    branches.push(unknown_tag_branch(info.type_name, "s"));

    let case_expr = spanned(Expr::CaseOf {
        expr: Box::new(var("s")),
        branches,
    });
    let lam = lambda(vec![pvar("s")], case_expr);

    let pipeline_step = app(
        elm_ast::builder::qualified(&["Decode"], "andThen"),
        vec![lam],
    );
    let body = pipeline_chain(
        elm_ast::builder::qualified(&["Decode"], "string"),
        vec![pipeline_step],
    );

    let sig = tqualified(&["Decode"], "Decoder", vec![tname(info.type_name, vec![])]);
    func_with_sig(&decoder_name, vec![], body, sig)
}

/// Decoder for an internally-tagged enum (`#[serde(tag = "<key>")]`).
///
/// The shape is:
///
/// ```elm
/// fooDecoder : Decoder Foo
/// fooDecoder =
///     Decode.field "<key>" Decode.string
///         |> Decode.andThen
///             (\tag ->
///                 case tag of
///                     "VariantA" ->
///                         Decode.succeed (\f1 f2 -> VariantA { field1 = f1, field2 = f2 })
///                             |> required "field1" ...
///                             |> required "field2" ...
///                     "UnitVariant" ->
///                         Decode.succeed UnitVariant
///                     _ ->
///                         Decode.fail ("Unknown Foo: " ++ tag)
///             )
/// ```
fn build_tagged_enum_decoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
    tag_key: &str,
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let decoder_name = format!("{}Decoder", lcfirst(info.type_name));

    let mut branches: Vec<CaseBranch> = Vec::with_capacity(variants.len() + 1);
    for v in variants {
        branches.push(CaseBranch {
            pattern: spanned(Pattern::Literal(Literal::String(v.json_tag.to_string()))),
            body: build_variant_decoder_body(v, names),
        });
    }
    branches.push(unknown_tag_branch(info.type_name, "tag"));

    let case_expr = spanned(Expr::CaseOf {
        expr: Box::new(var("tag")),
        branches,
    });
    let lam = lambda(vec![pvar("tag")], case_expr);

    let andthen_step = app(
        elm_ast::builder::qualified(&["Decode"], "andThen"),
        vec![lam],
    );

    let field_lookup = app(
        elm_ast::builder::qualified(&["Decode"], "field"),
        vec![
            string(tag_key),
            elm_ast::builder::qualified(&["Decode"], "string"),
        ],
    );
    let body = pipeline_chain(field_lookup, vec![andthen_step]);

    let sig = tqualified(&["Decode"], "Decoder", vec![tname(info.type_name, vec![])]);
    func_with_sig(&decoder_name, vec![], body, sig)
}

/// `_ -> Decode.fail ("Unknown <TypeName>: " ++ <bound_var>)`. The
/// `bound_var` refers to the parameter introduced by the surrounding
/// `andThen` lambda (`tag` for internally-tagged enums, `s` for
/// bare-string enums) which is still in scope under the wildcard.
fn unknown_tag_branch(type_name: &str, bound_var: &str) -> CaseBranch {
    let fail_msg_expr = elm_ast::builder::binop(
        "++",
        string(format!("Unknown {}: ", type_name)),
        var(bound_var),
    );
    CaseBranch {
        pattern: elm_ast::builder::pwild(),
        body: app(
            elm_ast::builder::qualified(&["Decode"], "fail"),
            vec![fail_msg_expr],
        ),
    }
}

/// Per-variant decoder body inside an internally-tagged `andThen`
/// `case` branch.
///
/// - Unit variant: `Decode.succeed VariantName`
/// - Struct variant: `Decode.succeed (\f1 f2 -> VariantName { ... })
///   |> required "..." ... |> optional "..." ... ...`
/// - Newtype variant `Foo(Inner)`: `Decode.map Foo <innerDecoder>`.
///   Relies on the inner type serializing to a JSON object so its
///   fields sit next to the discriminator on the same envelope the
///   outer decoder already has in scope.
fn build_variant_decoder_body(v: &ElmVariantInfo, names: &NameMap) -> Spanned<Expr> {
    let fields = match &v.payload {
        ElmVariantPayload::Unit => {
            return app(
                elm_ast::builder::qualified(&["Decode"], "succeed"),
                vec![var(v.elm_name)],
            );
        }
        ElmVariantPayload::Struct(fs) => fs,
        ElmVariantPayload::Newtype(inner) => {
            return app(
                elm_ast::builder::qualified(&["Decode"], "map"),
                vec![var(v.elm_name), decoder_for_type(inner, names)],
            );
        }
    };

    let lam_args: Vec<Spanned<Pattern>> = fields.iter().map(|f| pvar(f.elm_name)).collect();
    let record_setters: Vec<Spanned<RecordSetter>> = fields
        .iter()
        .map(|f| {
            spanned(RecordSetter {
                field: spanned(f.elm_name.to_string()),
                value: var(f.elm_name),
                trailing_comment: None,
            })
        })
        .collect();
    let record_expr = spanned(Expr::Record(record_setters));
    let ctor_call = app(var(v.elm_name), vec![record_expr]);
    let lam = lambda(lam_args, ctor_call);

    let seed = app(
        elm_ast::builder::qualified(&["Decode"], "succeed"),
        vec![lam],
    );
    let steps: Vec<_> = fields
        .iter()
        .map(|field| build_field_decoder_step(field, names))
        .collect();
    pipeline_chain(seed, steps)
}

/// Decoder for an untagged enum (`#[serde(untagged)]`).
///
/// The shape is:
///
/// ```elm
/// fooDecoder : Decoder Foo
/// fooDecoder =
///     Decode.oneOf
///         [ Decode.map VariantA <inner-decoder>
///         , Decode.map VariantB <inner-decoder>
///         , Decode.null UnitVariant
///         ]
/// ```
///
/// Variant order matches Rust source order so the "first match wins"
/// semantics match serde.
fn build_untagged_enum_decoder(
    info: &ElmTypeInfo,
    variants: &[ElmVariantInfo],
    names: &NameMap,
) -> Spanned<elm_ast::declaration::Declaration> {
    let decoder_name = format!("{}Decoder", lcfirst(info.type_name));

    let alternatives: Vec<Spanned<Expr>> = variants
        .iter()
        .map(|v| build_untagged_variant_decoder(v, names))
        .collect();

    let body = app(
        elm_ast::builder::qualified(&["Decode"], "oneOf"),
        vec![crate::helpers::list_multiline(alternatives)],
    );

    let sig = tqualified(&["Decode"], "Decoder", vec![tname(info.type_name, vec![])]);
    func_with_sig(&decoder_name, vec![], body, sig)
}

fn build_untagged_variant_decoder(v: &ElmVariantInfo, names: &NameMap) -> Spanned<Expr> {
    match &v.payload {
        // Unit variant: serde encodes as JSON `null`. Match `Decode.null`.
        ElmVariantPayload::Unit => app(
            elm_ast::builder::qualified(&["Decode"], "null"),
            vec![var(v.elm_name)],
        ),
        // Newtype: decode the inner type and lift into the constructor.
        ElmVariantPayload::Newtype(inner) => {
            let inner_decoder = decoder_for_type(inner, names);
            app(
                elm_ast::builder::qualified(&["Decode"], "map"),
                vec![var(v.elm_name), inner_decoder],
            )
        }
        // Struct variant: build an inline pipeline decoder for the
        // record and lift into the constructor.
        ElmVariantPayload::Struct(fields) => {
            let inner = build_struct_variant_decoder_expr(v, fields, names);
            app(
                elm_ast::builder::qualified(&["Decode"], "map"),
                vec![var(v.elm_name), inner],
            )
        }
    }
}

/// `Decode.succeed (\f1 f2 -> { f1 = f1, f2 = f2 }) |> required ... |> optional ...`
///
/// Used inside an untagged variant's `Decode.map Ctor <expr>`. Produces
/// the anonymous record value that the constructor will wrap.
fn build_struct_variant_decoder_expr(
    v: &ElmVariantInfo,
    fields: &[ElmFieldInfo],
    names: &NameMap,
) -> Spanned<Expr> {
    let lam_args: Vec<Spanned<Pattern>> = fields.iter().map(|f| pvar(f.elm_name)).collect();
    let record_setters: Vec<Spanned<RecordSetter>> = fields
        .iter()
        .map(|f| {
            spanned(RecordSetter {
                field: spanned(f.elm_name.to_string()),
                value: var(f.elm_name),
                trailing_comment: None,
            })
        })
        .collect();
    let record_expr = spanned(Expr::Record(record_setters));
    let lam = lambda(lam_args, record_expr);
    let _ = v;

    let seed = app(
        elm_ast::builder::qualified(&["Decode"], "succeed"),
        vec![lam],
    );
    let steps: Vec<_> = fields
        .iter()
        .map(|field| build_field_decoder_step(field, names))
        .collect();
    pipeline_chain(seed, steps)
}

fn build_field_decoder_step(field: &ElmFieldInfo, names: &NameMap) -> Spanned<elm_ast::expr::Expr> {
    if let Some(custom) = field.custom_decoder {
        return app(var("required"), vec![string(field.rust_name), var(custom)]);
    }

    if field.is_optional {
        let inner_type = match &field.elm_type {
            ElmTypeRepr::Maybe(inner) => inner.as_ref(),
            other => other,
        };
        let inner_decoder = decoder_for_type(inner_type, names);
        let nullable_decoder = app(
            elm_ast::builder::qualified(&["Decode"], "nullable"),
            vec![inner_decoder],
        );

        app(
            var("optional"),
            vec![string(field.rust_name), nullable_decoder, var("Nothing")],
        )
    } else {
        let field_decoder = decoder_for_type(&field.elm_type, names);
        app(
            var("required"),
            vec![string(field.rust_name), field_decoder],
        )
    }
}

/// Build a decoder expression for a given `ElmTypeRepr`. Recurses into
/// wrapper kinds (`Maybe`, `List`, `Dict`, `Tuple`) and resolves
/// `Custom(rust_name)` to `<elmName>Decoder` via the name map.
/// Exposed so the response-kind dispatch in request_style can reuse
/// the same expression-building logic for wrapped response bodies
/// (e.g. `Json<Vec<PersonApi>>`).
pub fn decoder_for_type(repr: &ElmTypeRepr, names: &NameMap) -> Spanned<elm_ast::expr::Expr> {
    match repr {
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => {
            elm_ast::builder::qualified(&["Decode"], "string")
        }
        ElmTypeRepr::Int => elm_ast::builder::qualified(&["Decode"], "int"),
        ElmTypeRepr::Float => elm_ast::builder::qualified(&["Decode"], "float"),
        ElmTypeRepr::Bool => elm_ast::builder::qualified(&["Decode"], "bool"),
        ElmTypeRepr::Posix => elm_ast::builder::qualified(&["Iso8601"], "decoder"),
        ElmTypeRepr::Value => elm_ast::builder::qualified(&["Decode"], "value"),
        ElmTypeRepr::Maybe(inner) => app(
            elm_ast::builder::qualified(&["Decode"], "nullable"),
            vec![decoder_for_type(inner, names)],
        ),
        ElmTypeRepr::List(inner) => app(
            elm_ast::builder::qualified(&["Decode"], "list"),
            vec![decoder_for_type(inner, names)],
        ),
        ElmTypeRepr::Dict(inner) => app(
            elm_ast::builder::qualified(&["Decode"], "dict"),
            vec![decoder_for_type(inner, names)],
        ),
        ElmTypeRepr::Tuple(elems) => build_tuple_decoder(elems, names),
        ElmTypeRepr::Custom(rust_name) => {
            let elm_name = names.resolve(rust_name);
            var(format!("{}Decoder", lcfirst(elm_name)))
        }
    }
}

/// `Decode.map2 (\a b -> ( a, b ))
///     (Decode.index 0 decA)
///     (Decode.index 1 decB)`
/// (and the analogous `Decode.map3` for 3-tuples). Tuple values arrive
/// as JSON arrays so we read each element by positional index.
fn build_tuple_decoder(elems: &[ElmTypeRepr], names: &NameMap) -> Spanned<Expr> {
    let arity = elems.len();
    debug_assert!(
        arity == 2 || arity == 3,
        "tuple arity {} should have been rejected at derive time",
        arity,
    );
    let map_fn = if arity == 2 { "map2" } else { "map3" };
    let names_chars: &[&str] = if arity == 2 {
        &["a", "b"]
    } else {
        &["a", "b", "c"]
    };

    let lam_args: Vec<Spanned<Pattern>> = names_chars.iter().map(|n| pvar(*n)).collect();
    let tuple_body = tuple(names_chars.iter().map(|n| var(*n)).collect());
    let lam = lambda(lam_args, tuple_body);

    let mut args: Vec<Spanned<Expr>> = Vec::with_capacity(arity + 1);
    args.push(lam);
    for (i, elem) in elems.iter().enumerate() {
        args.push(app(
            elm_ast::builder::qualified(&["Decode"], "index"),
            vec![int(i as i64), decoder_for_type(elem, names)],
        ));
    }

    app(elm_ast::builder::qualified(&["Decode"], map_fn), args)
}

/// Lowercase the first character (used for decoder/function naming).
pub fn lcfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
    }
}
