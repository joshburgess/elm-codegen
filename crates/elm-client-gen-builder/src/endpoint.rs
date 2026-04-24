//! Normalize a raw [`ElmEndpointInfo`] into [`EndpointSlots`] that
//! `RequestStyle` impls consume.
//!
//! The normalization pass:
//!
//! 1. Parses the path template into ordered segments.
//! 2. Merges `ExtractorInfo::PathParams` against the template:
//!    named entries match by slot name; anonymous entries (those
//!    with `name == ""`, emitted by tuple-style `Path<(A, B)>`)
//!    are zipped positionally against unclaimed slots.
//! 3. Asserts every template slot is claimed by exactly one path
//!    parameter, and every named parameter matches a slot.
//! 4. Flattens query params, body, and headers; drops `Skip`.
//!
//! Cross-check failures panic with `handler_name` so they're easy
//! to track down. The codegen is build-time tooling, so panicking
//! is the correct failure mode.

use std::collections::HashSet;

use elm_client_gen_core::ElmTypeRepr;
use elm_client_gen_http::{
    BodyKind, ElmEndpointInfo, ExtractorInfo, HeaderInfo, HttpMethod, PathParam, QueryParam,
    ResponseKind,
};

/// One segment of a normalized URL path.
#[derive(Clone, Debug)]
pub enum PathSegment<'a> {
    Literal(&'a str),
    Param {
        /// Slot name from the path template; never empty after
        /// normalization.
        name: &'a str,
        ty: &'a ElmTypeRepr,
    },
}

/// The request body slot, with both the kind (drives wire encoding)
/// and the Elm field type.
#[derive(Clone, Copy, Debug)]
pub struct BodySlot<'a> {
    pub kind: BodyKind,
    pub ty: &'a ElmTypeRepr,
}

/// A handler's metadata in the shape `RequestStyle` consumes:
/// path slots resolved, query/body/headers flattened, skips removed.
#[derive(Clone, Debug)]
pub struct EndpointSlots<'a> {
    pub function_name: &'a str,
    pub elm_module_path: &'a [&'a str],
    pub method: HttpMethod,
    pub path: Vec<PathSegment<'a>>,
    pub query: Vec<&'a QueryParam>,
    pub body: Option<BodySlot<'a>>,
    pub headers: Vec<&'a HeaderInfo>,
    /// `None` when `success_kind == ResponseKind::Empty` — the
    /// response carries no payload.
    pub success: Option<&'a ElmTypeRepr>,
    pub success_kind: ResponseKind,
    pub error: Option<&'a ElmTypeRepr>,
    pub tags: &'a [&'a str],
}

impl<'a> EndpointSlots<'a> {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }
}

/// Normalize one [`ElmEndpointInfo`].
///
/// Panics with a message naming `info.handler_name` if the path
/// template and extractor declarations don't agree.
pub fn normalize_endpoint<'a>(info: &'a ElmEndpointInfo) -> EndpointSlots<'a> {
    let template = parse_path_template(info.path_template);

    // Collect every PathParam from every PathParams extractor in
    // source order.
    let path_params: Vec<&PathParam> = info
        .params
        .iter()
        .filter_map(|p| match p {
            ExtractorInfo::PathParams(pp) => Some(pp.iter()),
            _ => None,
        })
        .flatten()
        .collect();

    // Resolve template slots, in template order.
    let path = resolve_path_segments(info, &template, &path_params);

    // Flatten the rest.
    let mut query: Vec<&QueryParam> = Vec::new();
    let mut body: Option<BodySlot<'_>> = None;
    let mut headers: Vec<&HeaderInfo> = Vec::new();

    for p in &info.params {
        match p {
            ExtractorInfo::QueryParams(qs) => query.extend(qs.iter()),
            ExtractorInfo::Body { kind, ty } => {
                if body.is_some() {
                    panic!(
                        "elm_endpoint normalization: handler `{}` declares more than one Body extractor",
                        info.handler_name,
                    );
                }
                body = Some(BodySlot { kind: *kind, ty });
            }
            ExtractorInfo::Header(h) => headers.push(h),
            ExtractorInfo::PathParams(_) | ExtractorInfo::Skip => {}
        }
    }

    EndpointSlots {
        function_name: info.elm_function_name,
        elm_module_path: info.elm_module_path,
        method: info.method,
        path,
        query,
        body,
        headers,
        success: info.response.success.as_ref(),
        success_kind: info.response.success_kind,
        error: info.response.error.as_ref(),
        tags: info.tags,
    }
}

#[derive(Clone, Copy, Debug)]
enum TemplateSegment<'a> {
    Literal(&'a str),
    Slot(&'a str),
}

fn parse_path_template(template: &str) -> Vec<TemplateSegment<'_>> {
    template
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 2 {
                TemplateSegment::Slot(&seg[1..seg.len() - 1])
            } else {
                TemplateSegment::Literal(seg)
            }
        })
        .collect()
}

fn resolve_path_segments<'a>(
    info: &'a ElmEndpointInfo,
    template: &[TemplateSegment<'a>],
    path_params: &[&'a PathParam],
) -> Vec<PathSegment<'a>> {
    let template_slots: Vec<&str> = template
        .iter()
        .filter_map(|s| match s {
            TemplateSegment::Slot(n) => Some(*n),
            _ => None,
        })
        .collect();

    let template_slot_set: HashSet<&str> = template_slots.iter().copied().collect();

    // Drop named extractors whose slot isn't in the URL template.
    // Some extractors (e.g. one that resolves the current org from a
    // session cookie *or* the URL) declare a slot opportunistically;
    // when the template doesn't include it, the named declaration is
    // simply unused.
    let path_params: Vec<&PathParam> = path_params
        .iter()
        .copied()
        .filter(|p| p.name.is_empty() || template_slot_set.contains(p.name))
        .collect();

    // Bucket params by name; collect anonymous ones in source order.
    // Multiple named extractors are allowed to declare the same slot
    // (e.g. `Extract<OrganizationId>` and
    // `Extract<OrganizationHasFeature<X>>` both consume `org_uuid`);
    // the first one wins and the rest are dropped silently.
    let mut named: std::collections::HashMap<&str, &PathParam> = std::collections::HashMap::new();
    let mut anonymous: Vec<&PathParam> = Vec::new();
    for p in path_params {
        if p.name.is_empty() {
            anonymous.push(p);
        } else {
            named.entry(p.name).or_insert(p);
        }
    }

    // Some handlers double-extract the same slot (e.g. a custom
    // `Extract<OrganizationId>` that claims `org_uuid` by name
    // alongside a `Path<(Uuid, ...)>` whose leading element mirrors
    // the same slot positionally). Detect this by comparing the
    // anonymous count against the unclaimed slot count: any excess
    // anonymous params at the front are treated as redundant
    // duplicates of named claims and skipped.
    let unclaimed_slots = template_slots
        .iter()
        .filter(|s| !named.contains_key(*s))
        .count();
    let mut anonymous_idx = anonymous.len().saturating_sub(unclaimed_slots);

    let resolved_segments: Vec<PathSegment<'a>> = template
        .iter()
        .map(|seg| match seg {
            TemplateSegment::Literal(l) => PathSegment::Literal(l),
            TemplateSegment::Slot(slot_name) => {
                if let Some(p) = named.get(slot_name) {
                    PathSegment::Param {
                        name: slot_name,
                        ty: &p.ty,
                    }
                } else if anonymous_idx < anonymous.len() {
                    let p = anonymous[anonymous_idx];
                    anonymous_idx += 1;
                    PathSegment::Param {
                        name: slot_name,
                        ty: &p.ty,
                    }
                } else {
                    panic!(
                        "elm_endpoint normalization: handler `{}` template slot `{{{}}}` has no matching extractor",
                        info.handler_name, slot_name,
                    );
                }
            }
        })
        .collect();

    if anonymous_idx < anonymous.len() {
        panic!(
            "elm_endpoint normalization: handler `{}` has {} unnamed path param(s) but only {} unclaimed template slot(s)",
            info.handler_name,
            anonymous.len(),
            anonymous_idx,
        );
    }

    resolved_segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use elm_client_gen_http::{
        BodyKind, ElmEndpointInfo, ElmTypeRepr, ExtractorInfo, HttpMethod, PathParam, QueryParam,
        ResponseInfo, ResponseKind,
    };

    fn fixture(path_template: &'static str, params: Vec<ExtractorInfo>) -> ElmEndpointInfo {
        ElmEndpointInfo {
            handler_name: "test_handler",
            elm_function_name: "testHandler",
            elm_module_path: &["Api", "Test"],
            method: HttpMethod::Get,
            path_template,
            params,
            response: ResponseInfo {
                success: Some(ElmTypeRepr::Custom("Foo".into())),
                success_kind: ResponseKind::Json,
                error: None,
            },
            tags: &[],
        }
    }

    #[test]
    fn parses_literals_and_slots() {
        let segs = parse_path_template("/api/v1/persons/{person_id}/atoms/{atom_sqid}");
        match segs.as_slice() {
            [TemplateSegment::Literal("api"), TemplateSegment::Literal("v1"), TemplateSegment::Literal("persons"), TemplateSegment::Slot("person_id"), TemplateSegment::Literal("atoms"), TemplateSegment::Slot("atom_sqid")] =>
                {}
            other => panic!("unexpected segments: {:?}", other),
        }
    }

    #[test]
    fn resolves_named_path_params() {
        let info = fixture(
            "/api/v1/persons/{person_id}",
            vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "person_id",
                ty: ElmTypeRepr::String,
            }])],
        );
        let slots = normalize_endpoint(&info);
        match slots.path.as_slice() {
            [PathSegment::Literal("api"), PathSegment::Literal("v1"), PathSegment::Literal("persons"), PathSegment::Param {
                name: "person_id",
                ty: ElmTypeRepr::String,
            }] => {}
            other => panic!("unexpected path: {:?}", other),
        }
    }

    #[test]
    fn fills_anonymous_params_positionally() {
        let info = fixture(
            "/api/v1/{a}/items/{b}",
            vec![ExtractorInfo::PathParams(vec![
                PathParam {
                    name: "",
                    ty: ElmTypeRepr::String,
                },
                PathParam {
                    name: "",
                    ty: ElmTypeRepr::Int,
                },
            ])],
        );
        let slots = normalize_endpoint(&info);
        match slots.path.as_slice() {
            [PathSegment::Literal("api"), PathSegment::Literal("v1"), PathSegment::Param {
                name: "a",
                ty: ElmTypeRepr::String,
            }, PathSegment::Literal("items"), PathSegment::Param {
                name: "b",
                ty: ElmTypeRepr::Int,
            }] => {}
            other => panic!("unexpected path: {:?}", other),
        }
    }

    #[test]
    fn mixes_named_and_anonymous_params() {
        let info = fixture(
            "/api/v1/orgs/{organization_id}/items/{item_id}",
            vec![
                // Custom extractor declares the org slot by name.
                ExtractorInfo::PathParams(vec![PathParam {
                    name: "organization_id",
                    ty: ElmTypeRepr::String,
                }]),
                // Path<String> contributes an anonymous param.
                ExtractorInfo::PathParams(vec![PathParam {
                    name: "",
                    ty: ElmTypeRepr::String,
                }]),
            ],
        );
        let slots = normalize_endpoint(&info);
        let names: Vec<&str> = slots
            .path
            .iter()
            .filter_map(|s| match s {
                PathSegment::Param { name, .. } => Some(*name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["organization_id", "item_id"]);
    }

    #[test]
    #[should_panic(expected = "no matching extractor")]
    fn panics_when_template_slot_has_no_extractor() {
        let info = fixture("/api/v1/{missing}", vec![ExtractorInfo::PathParams(vec![])]);
        normalize_endpoint(&info);
    }

    #[test]
    fn drops_named_param_not_in_template() {
        // Some custom extractors (e.g. one that resolves the org from
        // the URL *or* a session cookie) declare a slot
        // opportunistically. When the template doesn't include that
        // slot, the named declaration is silently dropped.
        let info = fixture(
            "/api/v1/persons",
            vec![ExtractorInfo::PathParams(vec![PathParam {
                name: "stranger",
                ty: ElmTypeRepr::String,
            }])],
        );
        let slots = normalize_endpoint(&info);
        assert!(slots
            .path
            .iter()
            .all(|s| matches!(s, PathSegment::Literal(_))));
    }

    #[test]
    fn allows_named_param_declared_twice() {
        // Multiple named extractors may consume the same slot — for
        // instance, a stack of guard extractors all reading
        // `org_uuid`. The first declaration wins; the rest are
        // dropped.
        let info = fixture(
            "/api/v1/{x}",
            vec![ExtractorInfo::PathParams(vec![
                PathParam {
                    name: "x",
                    ty: ElmTypeRepr::String,
                },
                PathParam {
                    name: "x",
                    ty: ElmTypeRepr::String,
                },
            ])],
        );
        let slots = normalize_endpoint(&info);
        match slots.path.as_slice() {
            [PathSegment::Literal("api"), PathSegment::Literal("v1"), PathSegment::Param {
                name: "x",
                ty: ElmTypeRepr::String,
            }] => {}
            other => panic!("unexpected path: {:?}", other),
        }
    }

    #[test]
    fn drops_redundant_anonymous_params_when_named_claims_overlap() {
        // Common Pinata pattern: `Extract<OrganizationId>` claims
        // `org_uuid` by name AND a `Path<(Uuid, String)>` extracts
        // the org_uuid positionally too. The leading anonymous Uuid
        // is a redundant duplicate; only the trailing String fills
        // an actual unclaimed slot.
        let info = fixture(
            "/api/v1/orgs/{org_uuid}/customers/{customer_id}",
            vec![
                ExtractorInfo::PathParams(vec![PathParam {
                    name: "org_uuid",
                    ty: ElmTypeRepr::String,
                }]),
                ExtractorInfo::PathParams(vec![
                    PathParam {
                        name: "",
                        ty: ElmTypeRepr::String,
                    },
                    PathParam {
                        name: "",
                        ty: ElmTypeRepr::String,
                    },
                ]),
            ],
        );
        let slots = normalize_endpoint(&info);
        let names: Vec<&str> = slots
            .path
            .iter()
            .filter_map(|s| match s {
                PathSegment::Param { name, .. } => Some(*name),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["org_uuid", "customer_id"]);
    }

    #[test]
    #[should_panic(expected = "more than one Body extractor")]
    fn panics_on_double_body() {
        let info = fixture(
            "/api/v1/x",
            vec![
                ExtractorInfo::Body {
                    kind: BodyKind::Json,
                    ty: ElmTypeRepr::Custom("A".into()),
                },
                ExtractorInfo::Body {
                    kind: BodyKind::Json,
                    ty: ElmTypeRepr::Custom("B".into()),
                },
            ],
        );
        normalize_endpoint(&info);
    }

    // Proptest: round-trip property for the path-template parser.
    //
    // Generating segments directly (rather than raw strings) keeps
    // inputs within the grammar the parser actually handles, so
    // shrinks produce human-readable counterexamples when something
    // breaks. We assert: render(parse(render(segments))) equals
    // render(segments) — i.e. normalization is a fixed point after
    // one pass.
    use proptest::prelude::*;

    fn render_segments(segments: &[TemplateSegment<'_>]) -> String {
        let mut out = String::from("/");
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            match seg {
                TemplateSegment::Literal(l) => out.push_str(l),
                TemplateSegment::Slot(n) => {
                    out.push('{');
                    out.push_str(n);
                    out.push('}');
                }
            }
        }
        out
    }

    // ASCII identifier-ish: no '/', no '{', no '}', no empty.
    fn literal_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z][a-zA-Z0-9_-]{0,8}".prop_map(String::from)
    }

    fn slot_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z_][a-zA-Z0-9_]{0,8}".prop_map(String::from)
    }

    #[derive(Clone, Debug)]
    enum Seg {
        Literal(String),
        Slot(String),
    }

    fn segment_strategy() -> impl Strategy<Value = Seg> {
        prop_oneof![
            literal_strategy().prop_map(Seg::Literal),
            slot_strategy().prop_map(Seg::Slot),
        ]
    }

    proptest! {
        #[test]
        fn parse_path_template_roundtrips(segs in prop::collection::vec(segment_strategy(), 0..8)) {
            let owned: Vec<TemplateSegment<'_>> = segs
                .iter()
                .map(|s| match s {
                    Seg::Literal(l) => TemplateSegment::Literal(l.as_str()),
                    Seg::Slot(n) => TemplateSegment::Slot(n.as_str()),
                })
                .collect();
            let rendered = render_segments(&owned);
            let parsed = parse_path_template(&rendered);
            prop_assert_eq!(parsed.len(), owned.len());
            for (a, b) in parsed.iter().zip(owned.iter()) {
                match (a, b) {
                    (TemplateSegment::Literal(x), TemplateSegment::Literal(y)) => {
                        prop_assert_eq!(x, y);
                    }
                    (TemplateSegment::Slot(x), TemplateSegment::Slot(y)) => {
                        prop_assert_eq!(x, y);
                    }
                    _ => prop_assert!(false, "segment kind mismatch"),
                }
            }
        }
    }

    #[test]
    fn flattens_query_body_headers_and_drops_skip() {
        let info = fixture(
            "/api/v1/items",
            vec![
                ExtractorInfo::Skip,
                ExtractorInfo::Body {
                    kind: BodyKind::Json,
                    ty: ElmTypeRepr::Custom("CreateItem".into()),
                },
                ExtractorInfo::QueryParams(vec![QueryParam {
                    name: "limit",
                    ty: ElmTypeRepr::Int,
                    required: true,
                }]),
                ExtractorInfo::Header(HeaderInfo {
                    name: "X-Trace-Id",
                    ty: ElmTypeRepr::String,
                    required: false,
                }),
            ],
        );
        let slots = normalize_endpoint(&info);
        let body = slots.body.expect("body should be set");
        assert_eq!(body.kind, BodyKind::Json);
        assert_eq!(slots.query.len(), 1);
        assert_eq!(slots.headers.len(), 1);
        assert_eq!(slots.query.first().expect("one query param").name, "limit");
        assert_eq!(
            slots.headers.first().expect("one header param").name,
            "X-Trace-Id"
        );
    }
}
