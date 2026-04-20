//! Pluggable Elm output for HTTP endpoints.
//!
//! [`RequestStyle`] turns one normalized [`EndpointSlots`] into one
//! Elm function declaration plus the imports it needs. Projects with
//! their own request conventions implement this trait.
//!
//! [`DefaultRequestStyle`] emits a plain `elm/http` `Http.request`
//! call. It dispatches on `BodyKind` and `ResponseKind` to pick the
//! right `Http.xBody` / `Http.expectX` and the right Elm field types
//! for `body` and the `toMsg` argument. It still skips query params
//! and headers and treats the error body as opaque; richer error
//! decoding belongs in a custom `RequestStyle`.

use elm_ast::builder::{
    app, func_with_sig, pvar, qualified, spanned, string, tfunc, tname, tunit, tvar, var,
};
use elm_ast::declaration::Declaration;
use elm_ast::expr::Expr;
use elm_ast::import::Import;
use elm_ast::node::Spanned;
use elm_ast::operator::InfixDirection;
use elm_ast::span::Span;
use elm_ast::type_annotation::TypeAnnotation;
use elm_codegen_core::ElmTypeRepr;
use elm_codegen_http::{BodyKind, ResponseKind};

use crate::decoder::{decoder_for_type, lcfirst};
use crate::endpoint::{BodySlot, EndpointSlots, PathSegment};
use crate::helpers::{import_as, import_exposing, record_multiline, tqualified, trecord};
use crate::name_map::NameMap;

/// One Elm function (the request function) plus the imports it needs.
pub struct RequestFunctionOutput {
    pub declarations: Vec<Spanned<Declaration>>,
    pub imports: Vec<Spanned<Import>>,
}

/// Render one normalized endpoint as one Elm declaration.
///
/// `current_module` is the module path the request function will be
/// emitted into. Implementations should skip imports for any type
/// whose `NameMap` entry already lives in this module (to avoid
/// self-imports when the request function is co-located with its
/// types).
///
/// Implementations can ignore parts of `EndpointSlots` they don't
/// support (the default ignores `query` and `headers`).
pub trait RequestStyle {
    fn build_request_function(
        &self,
        endpoint: &EndpointSlots<'_>,
        names: &NameMap,
        current_module: &[&str],
    ) -> RequestFunctionOutput;
}

/// Reference impl: vanilla `elm/http` `Http.request` call.
///
/// Limitations:
/// - Skips query params and headers.
/// - Treats the error body as opaque (`Result Http.Error a`).
/// - Body is encoded with `encode<TypeName>` (looked up via NameMap).
pub struct DefaultRequestStyle;

impl RequestStyle for DefaultRequestStyle {
    fn build_request_function(
        &self,
        endpoint: &EndpointSlots<'_>,
        names: &NameMap,
        current_module: &[&str],
    ) -> RequestFunctionOutput {
        let current: Vec<String> = current_module.iter().map(|s| s.to_string()).collect();
        let mut imports: Vec<Spanned<Import>> = vec![spanned(Import {
            module_name: spanned(vec!["Http".to_string()]),
            alias: None,
            exposing: None,
        })];

        let response = build_response_handling(endpoint, names, &current, &mut imports);
        let body = build_body_handling(endpoint, names, &current, &mut imports);

        let signature = build_signature(endpoint, &response, &body, names);
        let body_expr = build_request_body(
            endpoint,
            response.expect_expr.clone(),
            body.body_expr.clone(),
        );
        let decl = func_with_sig(
            endpoint.function_name,
            vec![pvar("params")],
            body_expr,
            signature,
        );

        RequestFunctionOutput {
            declarations: vec![decl],
            imports,
        }
    }
}

// ---------------------------------------------------------------------------
// Response and body kind dispatch
// ---------------------------------------------------------------------------

/// Output of dispatching on `endpoint.success_kind`.
struct ResponseHandling {
    /// `Http.expectX ...` expression.
    expect_expr: Spanned<Expr>,
    /// Type annotation for the success arm of `toMsg`'s `Result`.
    /// `None` means `Result Http.Error () -> msg` (Empty kind).
    success_type: Option<Spanned<TypeAnnotation>>,
}

fn build_response_handling(
    endpoint: &EndpointSlots<'_>,
    names: &NameMap,
    current: &[String],
    imports: &mut Vec<Spanned<Import>>,
) -> ResponseHandling {
    match endpoint.success_kind {
        ResponseKind::Json => {
            let success_repr = endpoint
                .success
                .expect("ResponseKind::Json requires a success type");
            // Walk the repr for every `Custom(name)` it references and
            // import its type alias plus per-type decoder. Wrapper kinds
            // (Maybe/List/Dict/Tuple) don't contribute a name themselves
            // but are composed inline via `Decode.<wrapper>` calls.
            import_custom_refs(success_repr, names, current, imports, ImportMode::Decoder);
            // Any wrapper kind needs the `Decode` module alias so
            // expressions like `Decode.list` resolve. Bare `Custom` only
            // references `<name>Decoder`, which is imported above.
            if repr_needs_decode_alias(success_repr) {
                imports.push(import_as(&["Json", "Decode"], "Decode"));
            }
            let decoder_expr = decoder_for_type(success_repr, names);
            let expect_expr = app(
                qualified(&["Http"], "expectJson"),
                vec![field("toMsg"), decoder_expr],
            );
            let success_type = Some(elm_type_annotation(success_repr, names));
            ResponseHandling {
                expect_expr,
                success_type,
            }
        }
        ResponseKind::Text => {
            let expect_expr = app(qualified(&["Http"], "expectString"), vec![field("toMsg")]);
            ResponseHandling {
                expect_expr,
                success_type: Some(tname("String", vec![])),
            }
        }
        ResponseKind::Empty => {
            let expect_expr = app(qualified(&["Http"], "expectWhatever"), vec![field("toMsg")]);
            ResponseHandling {
                expect_expr,
                success_type: None,
            }
        }
        ResponseKind::Bytes => {
            // Pass-through bytes: keep the response body as-is.
            // `Http.expectBytesResponse` requires a function that
            // converts `Http.Response Bytes` to `Result x a`. Emit
            // an inline `\\response -> ...` that yields the raw
            // bytes on a `GoodStatus_` and surfaces a meaningful
            // `Http.Error` otherwise. Users wanting different
            // semantics should plug in a custom RequestStyle.
            imports.push(elm_ast::builder::import(vec!["Bytes"]));
            let expect_expr = app(
                qualified(&["Http"], "expectBytesResponse"),
                vec![field("toMsg"), var("identity")],
            );
            ResponseHandling {
                expect_expr,
                success_type: Some(tqualified(&["Bytes"], "Bytes", vec![])),
            }
        }
    }
}

/// Output of dispatching on `endpoint.body`.
struct BodyHandling {
    /// `Http.xBody ...` expression for the request `body` field.
    body_expr: Spanned<Expr>,
    /// Type annotation for the params record's `body` field.
    /// `None` means no `body` field (no body extractor).
    body_field_type: Option<Spanned<TypeAnnotation>>,
}

fn build_body_handling(
    endpoint: &EndpointSlots<'_>,
    names: &NameMap,
    current: &[String],
    imports: &mut Vec<Spanned<Import>>,
) -> BodyHandling {
    let Some(slot) = endpoint.body else {
        return BodyHandling {
            body_expr: qualified(&["Http"], "emptyBody"),
            body_field_type: None,
        };
    };
    let BodySlot { kind, ty } = slot;
    let elm_field_type = elm_type_annotation(ty, names);
    match kind {
        BodyKind::Json => {
            let body_elm_name = elm_type_name(ty, names);
            let encoder = encoder_name(&body_elm_name);
            if let Some(entry) = name_entry(ty, names) {
                if entry.module_path != *current {
                    let module: Vec<&str> = entry.module_path.iter().map(String::as_str).collect();
                    imports.push(import_exposing(&module, vec![&entry.elm_name, &encoder]));
                }
            }
            imports.push(import_as(&["Json", "Encode"], "Encode"));
            BodyHandling {
                body_expr: app(
                    qualified(&["Http"], "jsonBody"),
                    vec![app(var(&encoder), vec![field("body")])],
                ),
                body_field_type: Some(elm_field_type),
            }
        }
        BodyKind::Text => BodyHandling {
            body_expr: app(
                qualified(&["Http"], "stringBody"),
                vec![string("text/plain"), field("body")],
            ),
            body_field_type: Some(tname("String", vec![])),
        },
        BodyKind::Form => {
            // The default style assumes the caller has already
            // URL-encoded the form. The Elm field type is just
            // `String`. Projects that prefer a typed body can
            // implement a custom `RequestStyle` that runs a real
            // form encoder.
            BodyHandling {
                body_expr: app(
                    qualified(&["Http"], "stringBody"),
                    vec![string("application/x-www-form-urlencoded"), field("body")],
                ),
                body_field_type: Some(tname("String", vec![])),
            }
        }
        BodyKind::Bytes => {
            imports.push(elm_ast::builder::import(vec!["Bytes"]));
            BodyHandling {
                body_expr: app(
                    qualified(&["Http"], "bytesBody"),
                    vec![string("application/octet-stream"), field("body")],
                ),
                body_field_type: Some(tqualified(&["Bytes"], "Bytes", vec![])),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Type signature
// ---------------------------------------------------------------------------

fn build_signature(
    endpoint: &EndpointSlots<'_>,
    response: &ResponseHandling,
    body: &BodyHandling,
    names: &NameMap,
) -> Spanned<TypeAnnotation> {
    let mut record_fields: Vec<(&str, Spanned<TypeAnnotation>)> =
        vec![("baseUrl", tname("String", vec![]))];

    for seg in &endpoint.path {
        if let PathSegment::Param { name, ty } = seg {
            record_fields.push((
                Box::leak(snake_to_camel(name).into_boxed_str()),
                elm_type_annotation(ty, names),
            ));
        }
    }

    if let Some(body_ty) = body.body_field_type.clone() {
        record_fields.push(("body", body_ty));
    }

    let success_arm = response.success_type.clone().unwrap_or_else(tunit);
    record_fields.push((
        "toMsg",
        tfunc(
            tqualified(
                &[],
                "Result",
                vec![tqualified(&["Http"], "Error", vec![]), success_arm],
            ),
            tvar("msg"),
        ),
    ));

    let record_ann = trecord(record_fields);
    let record_end_line = record_ann.span.end.line;
    // The signature printer's `type_ann_spans_multi_lines` walks
    // `FunctionType { from, to }` and combines `from`'s start line
    // with the END of `to`'s content. For a `Typed` arm, that "end"
    // comes from the inner `name.span` (or the last arg's content),
    // not the outer wrapper span — so pinning the wrapper alone
    // doesn't help. Build the return arm manually with the `name`'s
    // span on a line past the record, so the signature reads as
    // multi-line and the record arm gets expanded.
    let return_ann = if record_end_line != 0 {
        let line = record_end_line + 1;
        let pos = elm_ast::span::Position {
            offset: 0,
            line,
            column: 1,
        };
        let later_span = Span::new(pos, pos);
        // `type_ann_content_lines` for a Typed arm uses the LAST arg's
        // content end line (or the name's end line when there are no
        // args). Pin both so the signature reads as multi-line whether
        // or not the arm has args.
        let mut msg_arg = tvar("msg");
        msg_arg.span = later_span;
        spanned(TypeAnnotation::Typed {
            module_name: Vec::new(),
            name: Spanned::new(later_span, "Cmd".to_string()),
            args: vec![msg_arg],
        })
    } else {
        tqualified(&[], "Cmd", vec![tvar("msg")])
    };
    tfunc(record_ann, return_ann)
}

fn elm_type_annotation(repr: &ElmTypeRepr, names: &NameMap) -> Spanned<TypeAnnotation> {
    match repr {
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => tname("String", vec![]),
        ElmTypeRepr::Int => tname("Int", vec![]),
        ElmTypeRepr::Float => tname("Float", vec![]),
        ElmTypeRepr::Bool => tname("Bool", vec![]),
        ElmTypeRepr::Posix => tqualified(&["Time"], "Posix", vec![]),
        ElmTypeRepr::Value => tqualified(&["Encode"], "Value", vec![]),
        ElmTypeRepr::Maybe(inner) => tname("Maybe", vec![elm_type_annotation(inner, names)]),
        ElmTypeRepr::List(inner) => tname("List", vec![elm_type_annotation(inner, names)]),
        ElmTypeRepr::Dict(inner) => tname(
            "Dict",
            vec![tname("String", vec![]), elm_type_annotation(inner, names)],
        ),
        ElmTypeRepr::Tuple(elems) => spanned(TypeAnnotation::Tupled(
            elems
                .iter()
                .map(|e| elm_type_annotation(e, names))
                .collect(),
        )),
        ElmTypeRepr::Custom(rust) => {
            let resolved = names.resolve(rust).to_string();
            tname(Box::leak(resolved.into_boxed_str()) as &str, vec![])
        }
    }
}

// ---------------------------------------------------------------------------
// Body expression
// ---------------------------------------------------------------------------

fn build_request_body(
    endpoint: &EndpointSlots<'_>,
    expect_expr: Spanned<Expr>,
    body_expr: Spanned<Expr>,
) -> Spanned<Expr> {
    let url_expr = build_url(&endpoint.path);

    let request_record = record_multiline(vec![
        ("method", string(endpoint.method.as_str())),
        ("headers", elm_ast::builder::list(vec![])),
        ("url", url_expr),
        ("body", body_expr),
        ("expect", expect_expr),
        ("timeout", var("Nothing")),
        ("tracker", var("Nothing")),
    ]);

    app(qualified(&["Http"], "request"), vec![request_record])
}

/// Build the URL expression by coalescing literal segments and
/// interleaving them with path-param record accesses joined by `++`.
fn build_url(segments: &[PathSegment<'_>]) -> Spanned<Expr> {
    let mut chunks: Vec<Spanned<Expr>> = Vec::new();
    let mut current_literal = String::new();

    for seg in segments {
        match seg {
            PathSegment::Literal(l) => {
                current_literal.push('/');
                current_literal.push_str(l);
            }
            PathSegment::Param { name, ty } => {
                current_literal.push('/');
                chunks.push(string(current_literal.clone()));
                current_literal.clear();
                chunks.push(stringify_param(field(&snake_to_camel(name)), ty));
            }
        }
    }

    if !current_literal.is_empty() {
        chunks.push(string(current_literal));
    }

    // baseUrl ++ chunk1 ++ chunk2 ++ ... built right-associatively
    // since `++` in Elm is `infixr 5`. Pin each chunk to a distinct
    // line so the printer breaks long URL chains across lines instead
    // of running them all together.
    let mut all = vec![field("baseUrl")];
    all.extend(chunks);
    for (i, c) in all.iter_mut().enumerate() {
        let pos = elm_ast::span::Position {
            offset: 0,
            line: (i + 1) as u32,
            column: 1,
        };
        c.span = Span::new(pos, pos);
    }
    let mut iter = all.into_iter().rev();
    let mut acc = iter.next().expect("at least baseUrl");
    for left in iter {
        acc = concat(left, acc);
    }
    acc
}

/// Wrap a path-param accessor so it concatenates as `String`. Strings
/// pass through; numeric and boolean params get coerced via the
/// matching `String.from*` helper.
fn stringify_param(accessor: Spanned<Expr>, ty: &ElmTypeRepr) -> Spanned<Expr> {
    match ty {
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => accessor,
        ElmTypeRepr::Int => app(qualified(&["String"], "fromInt"), vec![accessor]),
        ElmTypeRepr::Float => app(qualified(&["String"], "fromFloat"), vec![accessor]),
        // Bool, Posix, Value, Maybe, List, Dict, Custom: out of scope
        // for URL path slots. Pass through and let Elm's type checker
        // surface the issue at the call site.
        _ => accessor,
    }
}

fn concat(left: Spanned<Expr>, right: Spanned<Expr>) -> Spanned<Expr> {
    let span = Span::new(left.span.start, right.span.end);
    Spanned::new(
        span,
        Expr::OperatorApplication {
            operator: "++".to_string(),
            direction: InfixDirection::Right,
            left: Box::new(left),
            right: Box::new(right),
        },
    )
}

fn field(name: &str) -> Spanned<Expr> {
    spanned(Expr::RecordAccess {
        record: Box::new(var("params")),
        field: spanned(name.to_string()),
    })
}

// ---------------------------------------------------------------------------
// Name helpers
// ---------------------------------------------------------------------------

fn elm_type_name(repr: &ElmTypeRepr, names: &NameMap) -> String {
    match repr {
        ElmTypeRepr::Custom(rust) => names.resolve(rust).to_string(),
        ElmTypeRepr::String | ElmTypeRepr::IsoDate => "String".to_string(),
        ElmTypeRepr::Int => "Int".to_string(),
        ElmTypeRepr::Float => "Float".to_string(),
        ElmTypeRepr::Bool => "Bool".to_string(),
        ElmTypeRepr::Posix => "Time.Posix".to_string(),
        ElmTypeRepr::Value => "Encode.Value".to_string(),
        ElmTypeRepr::Maybe(inner) => format!("Maybe {}", elm_type_name(inner, names)),
        ElmTypeRepr::List(inner) => format!("List {}", elm_type_name(inner, names)),
        ElmTypeRepr::Dict(inner) => format!("Dict String {}", elm_type_name(inner, names)),
        ElmTypeRepr::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| elm_type_name(e, names)).collect();
            format!("({})", inner.join(", "))
        }
    }
}

fn name_entry<'a>(
    repr: &ElmTypeRepr,
    names: &'a NameMap,
) -> Option<&'a crate::name_map::NameEntry> {
    if let ElmTypeRepr::Custom(rust) = repr {
        names.lookup(rust)
    } else {
        None
    }
}

fn encoder_name(elm_name: &str) -> String {
    format!("encode{}", elm_name)
}

/// Whether emitting a decoder expression for this repr references the
/// `Decode` module alias (e.g. `Decode.list`, `Decode.nullable`). Bare
/// `Custom(name)` reduces to `<name>Decoder` — unqualified — so it
/// doesn't need the alias import.
fn repr_needs_decode_alias(repr: &ElmTypeRepr) -> bool {
    !matches!(repr, ElmTypeRepr::Custom(_))
}

/// What to expose when importing a `Custom` type referenced by a
/// request/response repr.
#[derive(Clone, Copy)]
enum ImportMode {
    Decoder,
    #[allow(dead_code)]
    Encoder,
}

/// Walk `repr` for every `Custom(rust_name)` it contains and push an
/// import of each referenced type's module with the matching
/// type-alias and decoder/encoder exposed. Skips entries whose module
/// matches `current` (which would be a self-import).
fn import_custom_refs(
    repr: &ElmTypeRepr,
    names: &NameMap,
    current: &[String],
    imports: &mut Vec<Spanned<Import>>,
    mode: ImportMode,
) {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    collect_custom_refs(repr, &mut seen);
    for rust_name in &seen {
        let Some(entry) = names.lookup(rust_name) else {
            continue;
        };
        if entry.module_path == *current {
            continue;
        }
        let module_refs: Vec<&str> = entry.module_path.iter().map(String::as_str).collect();
        let decoder = format!("{}Decoder", lcfirst(&entry.elm_name));
        let encoder = format!("encode{}", entry.elm_name);
        let exposed: Vec<&str> = match mode {
            ImportMode::Decoder => vec![&entry.elm_name, decoder.as_str()],
            ImportMode::Encoder => vec![&entry.elm_name, encoder.as_str()],
        };
        imports.push(import_exposing(&module_refs, exposed));
    }
}

fn collect_custom_refs(repr: &ElmTypeRepr, out: &mut std::collections::BTreeSet<String>) {
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

fn snake_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::normalize_endpoint;
    use elm_ast::builder::module;
    use elm_codegen_core::ElmTypeInfo;
    use elm_codegen_http::{
        BodyKind, ElmEndpointInfo, ExtractorInfo, HttpMethod, PathParam, ResponseInfo, ResponseKind,
    };

    fn json_response(name: &str) -> ResponseInfo {
        ResponseInfo {
            success: Some(ElmTypeRepr::Custom(name.to_string())),
            success_kind: ResponseKind::Json,
            error: None,
        }
    }

    fn name_map() -> NameMap {
        let person = ElmTypeInfo {
            rust_name: "Person",
            module_path: vec!["Api", "Person"],
            type_name: "Person",
            tags: vec![],
            kind: elm_codegen_core::ElmTypeKind::Record { fields: vec![] },
        };
        let create = ElmTypeInfo {
            rust_name: "CreatePerson",
            module_path: vec!["Api", "Person"],
            type_name: "CreatePerson",
            tags: vec![],
            kind: elm_codegen_core::ElmTypeKind::Record { fields: vec![] },
        };
        NameMap::from_types(&[person, create])
    }

    fn render(endpoint: &ElmEndpointInfo, names: &NameMap) -> String {
        // Render into a different module from where the types live, so
        // the import-skip logic doesn't fire and assertions can check
        // the imports.
        let current_module: Vec<&str> = vec!["Api", "Generated", "Requests", "Person"];
        let slots = normalize_endpoint(endpoint);
        let style = DefaultRequestStyle;
        let out = style.build_request_function(&slots, names, &current_module);
        let m = module(current_module.clone(), out.declarations);
        let mut m = m;
        m.imports = out.imports;
        elm_ast::pretty_print(&m)
    }

    #[test]
    fn renders_get_with_path_param() {
        let info = ElmEndpointInfo {
            handler_name: "get_person",
            elm_function_name: "getPerson",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Get,
            path_template: "/api/v1/persons/{person_id}",
            params: vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "person_id",
                ty: ElmTypeRepr::String,
            }])],
            response: json_response("Person"),
            tags: &[],
        };
        let out = render(&info, &name_map());

        // Spot checks: this is enough to verify the shape without
        // pinning whitespace in a brittle golden file.
        assert!(out.contains("getPerson :"), "missing signature:\n{out}");
        assert!(
            out.contains("baseUrl : String"),
            "missing baseUrl field:\n{out}"
        );
        assert!(
            out.contains("personId : String"),
            "missing personId field:\n{out}"
        );
        assert!(
            out.contains("toMsg : Result Http.Error Person -> msg"),
            "missing toMsg:\n{out}"
        );
        assert!(out.contains("-> Cmd msg"), "missing return type:\n{out}");
        assert!(out.contains("Http.request"), "missing Http.request:\n{out}");
        assert!(out.contains(r#"method = "GET""#), "missing method:\n{out}");
        assert!(
            out.contains("params.baseUrl"),
            "missing baseUrl operand:\n{out}"
        );
        assert!(
            out.contains(r#"++ "/api/v1/persons/""#),
            "missing literal segment:\n{out}",
        );
        assert!(
            out.contains("++ params.personId"),
            "missing path-param operand:\n{out}"
        );
        assert!(out.contains("Http.emptyBody"), "missing empty body:\n{out}");
        assert!(
            out.contains("Http.expectJson params.toMsg personDecoder"),
            "expect not built correctly:\n{out}",
        );
        assert!(
            out.contains("import Api.Person exposing (Person, personDecoder)"),
            "missing decoder import:\n{out}",
        );
    }

    #[test]
    fn renders_post_with_body() {
        let info = ElmEndpointInfo {
            handler_name: "create_person",
            elm_function_name: "createPerson",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Post,
            path_template: "/api/v1/persons",
            params: vec![ExtractorInfo::Body {
                kind: BodyKind::Json,
                ty: ElmTypeRepr::Custom("CreatePerson".into()),
            }],
            response: json_response("Person"),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(out.contains(r#"method = "POST""#), "wrong method:\n{out}");
        assert!(
            out.contains("body : CreatePerson"),
            "missing body field:\n{out}"
        );
        assert!(
            out.contains("Http.jsonBody (encodeCreatePerson params.body)"),
            "body not encoded correctly:\n{out}",
        );
        assert!(
            out.contains("CreatePerson"),
            "missing CreatePerson exposure:\n{out}"
        );
        assert!(
            out.contains("encodeCreatePerson"),
            "missing encodeCreatePerson exposure:\n{out}",
        );
    }

    #[test]
    fn skips_imports_when_request_lives_in_same_module_as_types() {
        let info = ElmEndpointInfo {
            handler_name: "get_person",
            elm_function_name: "getPerson",
            elm_module_path: &["Api", "Person"],
            method: HttpMethod::Get,
            path_template: "/api/v1/persons/{person_id}",
            params: vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "person_id",
                ty: ElmTypeRepr::String,
            }])],
            response: json_response("Person"),
            tags: &[],
        };
        // Render directly into the same module the type lives in.
        let slots = normalize_endpoint(&info);
        let style = DefaultRequestStyle;
        let names = name_map();
        let out = style.build_request_function(&slots, &names, &["Api", "Person"]);
        let m = module(vec!["Api", "Person"], out.declarations);
        let mut m = m;
        m.imports = out.imports;
        let rendered = elm_ast::pretty_print(&m);

        assert!(
            !rendered.contains("import Api.Person"),
            "should not self-import:\n{rendered}",
        );
        assert!(
            !rendered.contains("import Json.Decode"),
            "default style should not import Json.Decode (only Http is used directly):\n{rendered}",
        );
    }

    fn empty_response() -> ResponseInfo {
        ResponseInfo {
            success: None,
            success_kind: ResponseKind::Empty,
            error: None,
        }
    }

    fn text_response() -> ResponseInfo {
        ResponseInfo {
            success: Some(ElmTypeRepr::String),
            success_kind: ResponseKind::Text,
            error: None,
        }
    }

    fn bytes_response() -> ResponseInfo {
        ResponseInfo {
            success: Some(ElmTypeRepr::Custom("Bytes.Bytes".into())),
            success_kind: ResponseKind::Bytes,
            error: None,
        }
    }

    #[test]
    fn renders_empty_response_with_unit_signature_and_expect_whatever() {
        let info = ElmEndpointInfo {
            handler_name: "delete_person",
            elm_function_name: "deletePerson",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Delete,
            path_template: "/api/v1/persons/{person_id}",
            params: vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "person_id",
                ty: ElmTypeRepr::String,
            }])],
            response: empty_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(
            out.contains("toMsg : Result Http.Error () -> msg"),
            "expected unit toMsg signature:\n{out}",
        );
        assert!(
            out.contains("Http.expectWhatever params.toMsg"),
            "expected expectWhatever:\n{out}",
        );
        assert!(out.contains("Http.emptyBody"), "expected emptyBody:\n{out}");
    }

    #[test]
    fn renders_text_response_with_string_and_expect_string() {
        let info = ElmEndpointInfo {
            handler_name: "ping",
            elm_function_name: "ping",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Get,
            path_template: "/api/v1/ping",
            params: vec![],
            response: text_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(
            out.contains("toMsg : Result Http.Error String -> msg"),
            "expected String toMsg:\n{out}",
        );
        assert!(
            out.contains("Http.expectString params.toMsg"),
            "expected expectString:\n{out}",
        );
    }

    #[test]
    fn renders_text_body_with_string_body_text_plain() {
        let info = ElmEndpointInfo {
            handler_name: "post_text",
            elm_function_name: "postText",
            elm_module_path: &["Api", "Generated", "Note"],
            method: HttpMethod::Post,
            path_template: "/api/v1/notes",
            params: vec![ExtractorInfo::Body {
                kind: BodyKind::Text,
                ty: ElmTypeRepr::String,
            }],
            response: empty_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(
            out.contains("body : String"),
            "expected String body field:\n{out}"
        );
        assert!(
            out.contains(r#"Http.stringBody "text/plain" params.body"#),
            "expected stringBody text/plain:\n{out}",
        );
    }

    #[test]
    fn renders_form_body_with_string_body_form_urlencoded() {
        let info = ElmEndpointInfo {
            handler_name: "submit_form",
            elm_function_name: "submitForm",
            elm_module_path: &["Api", "Generated", "Form"],
            method: HttpMethod::Post,
            path_template: "/api/v1/form",
            params: vec![ExtractorInfo::Body {
                kind: BodyKind::Form,
                ty: ElmTypeRepr::Custom("FormPayload".into()),
            }],
            response: empty_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        // Default style takes a pre-encoded String for forms.
        assert!(
            out.contains("body : String"),
            "expected String body field for form:\n{out}"
        );
        assert!(
            out.contains(r#"Http.stringBody "application/x-www-form-urlencoded" params.body"#),
            "expected form-urlencoded stringBody:\n{out}",
        );
    }

    #[test]
    fn renders_bytes_response_with_bytes_signature_and_import() {
        let info = ElmEndpointInfo {
            handler_name: "download",
            elm_function_name: "download",
            elm_module_path: &["Api", "Generated", "Files"],
            method: HttpMethod::Get,
            path_template: "/api/v1/files/{id}",
            params: vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "id",
                ty: ElmTypeRepr::String,
            }])],
            response: bytes_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(
            out.contains("import Bytes"),
            "expected Bytes import:\n{out}"
        );
        assert!(
            out.contains("toMsg : Result Http.Error Bytes.Bytes -> msg"),
            "expected Bytes.Bytes toMsg:\n{out}",
        );
        assert!(
            out.contains("Http.expectBytesResponse params.toMsg identity"),
            "expected expectBytesResponse:\n{out}",
        );
    }

    #[test]
    fn renders_bytes_body_with_bytes_body() {
        let info = ElmEndpointInfo {
            handler_name: "upload",
            elm_function_name: "upload",
            elm_module_path: &["Api", "Generated", "Files"],
            method: HttpMethod::Post,
            path_template: "/api/v1/files",
            params: vec![ExtractorInfo::Body {
                kind: BodyKind::Bytes,
                ty: ElmTypeRepr::Custom("Bytes.Bytes".into()),
            }],
            response: empty_response(),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(
            out.contains("body : Bytes.Bytes"),
            "expected Bytes.Bytes body field:\n{out}",
        );
        assert!(
            out.contains(r#"Http.bytesBody "application/octet-stream" params.body"#),
            "expected bytesBody:\n{out}",
        );
    }

    #[test]
    fn renders_patch_with_body_and_path_param() {
        let info = ElmEndpointInfo {
            handler_name: "patch_person",
            elm_function_name: "patchPerson",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Patch,
            path_template: "/api/v1/persons/{person_id}",
            params: vec![
                ExtractorInfo::PathParams(vec![PathParam {
                    name: "person_id",
                    ty: ElmTypeRepr::String,
                }]),
                ExtractorInfo::Body {
                    kind: BodyKind::Json,
                    ty: ElmTypeRepr::Custom("CreatePerson".into()),
                },
            ],
            response: json_response("Person"),
            tags: &[],
        };
        let out = render(&info, &name_map());
        assert!(out.contains(r#"method = "PATCH""#), "wrong method:\n{out}");
        assert!(
            out.contains("body : CreatePerson"),
            "missing body field:\n{out}"
        );
        assert!(
            out.contains("Http.jsonBody (encodeCreatePerson params.body)"),
            "body not encoded correctly:\n{out}",
        );
        assert!(
            out.contains("toMsg : Result Http.Error Person -> msg"),
            "missing toMsg:\n{out}",
        );
        assert!(
            out.contains("++ params.personId"),
            "missing path param:\n{out}"
        );
    }

    #[test]
    fn renders_url_with_two_path_params() {
        let info = ElmEndpointInfo {
            handler_name: "get_atom",
            elm_function_name: "getAtom",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Get,
            path_template: "/api/v1/persons/{person_id}/atoms/{atom_sqid}",
            params: vec![ExtractorInfo::PathParams(vec![
                PathParam {
                    name: "person_id",
                    ty: ElmTypeRepr::String,
                },
                PathParam {
                    name: "atom_sqid",
                    ty: ElmTypeRepr::String,
                },
            ])],
            response: json_response("Person"),
            tags: &[],
        };
        let out = render(&info, &name_map());
        for needle in [
            "params.baseUrl",
            r#"++ "/api/v1/persons/""#,
            "++ params.personId",
            r#"++ "/atoms/""#,
            "++ params.atomSqid",
        ] {
            assert!(out.contains(needle), "missing `{needle}` in:\n{out}");
        }
    }
}
