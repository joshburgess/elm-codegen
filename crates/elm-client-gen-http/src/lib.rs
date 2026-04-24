//! HTTP endpoint metadata for Elm code generation.
//!
//! Handlers annotated with `#[elm_endpoint(...)]` register an
//! [`ElmEndpointInfo`] builder into a global `inventory` collector at
//! link time. Downstream tooling (e.g. `elm-client-gen-builder`) iterates
//! the registry to emit Elm request functions.
//!
//! This crate defines the data model only. Traits, framework impls,
//! and the attribute macro are added in subsequent layers.

#[doc(hidden)]
pub use inventory;

pub use elm_client_gen_core::ElmTypeRepr;

#[cfg(feature = "derive")]
pub use elm_client_gen_derive::elm_endpoint;

mod traits;
pub use traits::{ElmExtractor, ElmPathParams, ElmQueryStruct, ElmResponse, ElmScalar};

/// HTTP method on an endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }
}

/// One path parameter contributed by an extractor.
///
/// `name` matches a `{slot}` in the path template. For tuple-style
/// extractors that don't know slot names (e.g. `Path<(A, B)>`), this
/// is left empty and filled in positionally by the normalization pass.
#[derive(Clone, Debug)]
pub struct PathParam {
    pub name: &'static str,
    pub ty: ElmTypeRepr,
}

/// One query parameter contributed by an extractor.
#[derive(Clone, Debug)]
pub struct QueryParam {
    pub name: &'static str,
    pub ty: ElmTypeRepr,
    /// `false` if the corresponding Rust field is `Option<T>`.
    pub required: bool,
}

/// One header an endpoint expects in the Elm function signature.
///
/// Auth tokens and similar implicit headers should not appear here;
/// they're handled by the `RequestStyle` impl.
#[derive(Clone, Debug)]
pub struct HeaderInfo {
    pub name: &'static str,
    pub ty: ElmTypeRepr,
    pub required: bool,
}

/// How a request body is encoded on the wire. Drives which
/// `Http.xBody` constructor the codegen emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// `application/json`. Body is encoded with the type's
    /// generated `encode<T>`. Default style emits `Http.jsonBody`.
    Json,
    /// `text/plain`. Body is the raw string itself. Default
    /// style emits `Http.stringBody "text/plain" body`.
    Text,
    /// `application/x-www-form-urlencoded`. Body field type is
    /// the typed payload `T`; the consumer is responsible for
    /// providing a form encoder via a custom `RequestStyle`. The
    /// default style emits a `stringBody` with this content-type.
    Form,
    /// `application/octet-stream` (or other binary). Body field
    /// type is `Bytes.Bytes` from `elm/bytes`. Default style
    /// emits `Http.bytesBody`.
    Bytes,
}

/// How a successful response body is decoded on the Elm side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseKind {
    /// JSON, decoded with the success type's generated decoder.
    /// Default style emits `Http.expectJson params.toMsg <decoder>`.
    Json,
    /// Plain text. Success type is `String`. Default style emits
    /// `Http.expectString params.toMsg`.
    Text,
    /// No payload. Success type is `()`. Default style emits
    /// `Http.expectWhatever params.toMsg` and `toMsg : Result Http.Error () -> msg`.
    Empty,
    /// Raw bytes. Success type is `Bytes.Bytes`. Default style
    /// emits `Http.expectBytesResponse` with a passthrough handler.
    Bytes,
}

/// What one extractor in a handler signature contributes to the Elm
/// function. Returned by `ElmExtractor::elm_extractor_info()` in
/// later layers.
#[derive(Clone, Debug)]
pub enum ExtractorInfo {
    /// Zero or more path parameters.
    PathParams(Vec<PathParam>),
    /// Zero or more query parameters.
    QueryParams(Vec<QueryParam>),
    /// Request body, with the type the Elm field exposes and the
    /// kind that drives wire encoding.
    Body { kind: BodyKind, ty: ElmTypeRepr },
    /// A header that should appear in the Elm function signature.
    Header(HeaderInfo),
    /// Extractor doesn't show up in the Elm signature
    /// (e.g. `State`, sessions, request id).
    Skip,
}

/// What a handler returns, as far as Elm cares.
#[derive(Clone, Debug)]
pub struct ResponseInfo {
    /// Type decoded from the response body on success. `None`
    /// when `kind == ResponseKind::Empty` — the codegen emits
    /// `Result Http.Error () -> msg` with no decoder.
    pub success: Option<ElmTypeRepr>,
    /// How to decode the success body.
    pub success_kind: ResponseKind,
    /// Type decoded from the response body on error, if any.
    /// `None` means the error body is opaque to Elm.
    pub error: Option<ElmTypeRepr>,
}

/// Everything the codegen needs to know about one endpoint.
///
/// Built by the expansion of `#[elm_endpoint(...)]` and inventory-
/// collected at link time.
#[derive(Clone, Debug)]
pub struct ElmEndpointInfo {
    /// The Rust handler function name, e.g. `"get_person"`.
    /// Used for diagnostics.
    pub handler_name: &'static str,
    /// The Elm function name, e.g. `"getPerson"`. Defaults to
    /// camelCased `handler_name` if not overridden in the macro.
    pub elm_function_name: &'static str,
    /// The Elm module path the function belongs to, e.g.
    /// `["Api", "Generated", "Person"]`.
    pub elm_module_path: &'static [&'static str],
    /// HTTP method.
    pub method: HttpMethod,
    /// Path template with `{slot}` placeholders, e.g.
    /// `"/api/v1/persons/{person_id}"`.
    pub path_template: &'static str,
    /// One entry per parameter on the handler signature, in source
    /// order. The normalization pass merges these against
    /// `path_template`.
    pub params: Vec<ExtractorInfo>,
    /// What the handler returns.
    pub response: ResponseInfo,
    /// Free-form labels for downstream filtering / strategy
    /// branching, mirroring `ElmTypeInfo::tags`.
    pub tags: &'static [&'static str],
}

impl ElmEndpointInfo {
    /// Returns true if this endpoint carries the given tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }
}

/// Registration entry stored in the global `inventory` collector.
///
/// A function pointer is used because `inventory::submit!` requires
/// a const expression, while `ElmEndpointInfo` owns allocations.
/// Downstream tooling calls `(build)()` to materialize the metadata.
pub struct ElmEndpointRegistration {
    pub build: fn() -> ElmEndpointInfo,
}

inventory::collect!(ElmEndpointRegistration);

/// Materialize every endpoint registered via `#[elm_endpoint(...)]`.
///
/// Iteration order follows link order. Sort the result if you need
/// deterministic output.
pub fn registered_endpoints() -> Vec<ElmEndpointInfo> {
    inventory::iter::<ElmEndpointRegistration>()
        .map(|r| (r.build)())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_method_as_str_round_trip() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Patch.as_str(), "PATCH");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }

    fn fixture_endpoint() -> ElmEndpointInfo {
        ElmEndpointInfo {
            handler_name: "get_person",
            elm_function_name: "getPerson",
            elm_module_path: &["Api", "Generated", "Person"],
            method: HttpMethod::Get,
            path_template: "/api/v1/persons/{person_id}",
            params: vec![
                ExtractorInfo::PathParams(vec![PathParam {
                    name: "person_id",
                    ty: ElmTypeRepr::String,
                }]),
                ExtractorInfo::QueryParams(vec![QueryParam {
                    name: "include_archived",
                    ty: ElmTypeRepr::Bool,
                    required: false,
                }]),
                ExtractorInfo::Skip,
            ],
            response: ResponseInfo {
                success: Some(ElmTypeRepr::Custom("Person".to_string())),
                success_kind: ResponseKind::Json,
                error: None,
            },
            tags: &["read"],
        }
    }

    #[test]
    fn endpoint_info_has_tag() {
        let e = fixture_endpoint();
        assert!(e.has_tag("read"));
        assert!(!e.has_tag("write"));
    }

    inventory::submit! {
        ElmEndpointRegistration { build: fixture_endpoint }
    }

    #[test]
    fn registered_endpoints_includes_inventory_submissions() {
        let endpoints = registered_endpoints();
        assert!(
            endpoints.iter().any(|e| e.handler_name == "get_person"),
            "fixture endpoint not picked up by inventory"
        );
    }
}
