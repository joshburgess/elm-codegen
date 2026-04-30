//! Traits handlers' parameter and return types implement to teach
//! the codegen how each one maps to Elm.

use elm_client_gen_core::{ElmType, ElmTypeRepr};

#[cfg(any(feature = "axum-0-6", feature = "axum-0-7", feature = "axum-0-8"))]
use crate::{BodyKind, ResponseKind};
use crate::{ExtractorInfo, PathParam, QueryParam, ResponseInfo};

// ---------------------------------------------------------------------------
// axum version selector mutual exclusion
// ---------------------------------------------------------------------------
//
// Enable exactly one of `axum-0-6`, `axum-0-7`, `axum-0-8`. The trait impls
// are emitted per-version and would compile across multiple versions, but
// pulling in two copies of axum at once is almost never what callers want.

#[cfg(any(
    all(feature = "axum-0-6", feature = "axum-0-7"),
    all(feature = "axum-0-6", feature = "axum-0-8"),
    all(feature = "axum-0-7", feature = "axum-0-8"),
))]
compile_error!("elm-client-gen-http: enable exactly one of `axum-0-6`, `axum-0-7`, `axum-0-8`.");

/// Implemented by every type used as a parameter on an annotated
/// handler. Covers `Path<T>`, `Query<T>`, `Json<T>`, custom
/// extractors, and types that should be skipped (`State<T>`,
/// session extractors, etc.).
///
/// The bundled `axum-0-X` features provide impls for the standard
/// Axum extractors. Users impl this for their own extractor wrappers.
pub trait ElmExtractor {
    fn elm_extractor_info() -> ExtractorInfo;
}

/// Implemented by handler return types so the codegen can find the
/// success and error body types. Default impls cover
/// `Result<Json<T>, E>`; users override for richer error encoding.
pub trait ElmResponse {
    fn elm_response_info() -> ResponseInfo;
}

/// Helper for `Path<T>`: how a `T` decomposes into one or more path
/// parameters. Tuple impls are provided for arities up to 8 over
/// `ElmScalar`. Users impl this directly for single-typed structs
/// pulled from the path.
pub trait ElmPathParams {
    fn path_params() -> Vec<PathParam>;
}

/// Helper for `Query<T>`: every named field of `T` becomes one
/// query parameter. Blanket-implemented for any `T: ElmType`, so
/// `#[derive(ElmType)]` on the query struct is sufficient.
pub trait ElmQueryStruct {
    fn query_params() -> Vec<QueryParam>;
}

/// Maps a Rust scalar type to its Elm representation. Implemented
/// for the common primitives. Users add impls for newtype IDs that
/// appear in path slots (`PersonId`, `Sqid`, etc.).
pub trait ElmScalar {
    fn elm_scalar_type() -> ElmTypeRepr;
}

// ---------------------------------------------------------------------------
// ElmScalar primitive impls
// ---------------------------------------------------------------------------

macro_rules! impl_elm_scalar {
    ($($ty:ty => $repr:expr),* $(,)?) => {
        $(
            impl ElmScalar for $ty {
                fn elm_scalar_type() -> ElmTypeRepr { $repr }
            }
        )*
    };
}

impl_elm_scalar! {
    String  => ElmTypeRepr::String,
    bool    => ElmTypeRepr::Bool,
    i8      => ElmTypeRepr::Int,
    i16     => ElmTypeRepr::Int,
    i32     => ElmTypeRepr::Int,
    i64     => ElmTypeRepr::Int,
    isize   => ElmTypeRepr::Int,
    u8      => ElmTypeRepr::Int,
    u16     => ElmTypeRepr::Int,
    u32     => ElmTypeRepr::Int,
    u64     => ElmTypeRepr::Int,
    usize   => ElmTypeRepr::Int,
    f32     => ElmTypeRepr::Float,
    f64     => ElmTypeRepr::Float,
}

// ---------------------------------------------------------------------------
// ElmPathParams: single scalar and tuples
// ---------------------------------------------------------------------------

/// Single-scalar `Path<T>`. Slot name is filled in positionally by
/// the normalization pass.
impl<T: ElmScalar> ElmPathParams for T {
    fn path_params() -> Vec<PathParam> {
        vec![PathParam {
            name: "",
            ty: T::elm_scalar_type(),
        }]
    }
}

macro_rules! impl_path_params_tuple {
    ($($t:ident),+) => {
        impl<$($t: ElmScalar),+> ElmPathParams for ($($t,)+) {
            fn path_params() -> Vec<PathParam> {
                vec![
                    $( PathParam { name: "", ty: $t::elm_scalar_type() }, )+
                ]
            }
        }
    };
}

impl_path_params_tuple!(A, B);
impl_path_params_tuple!(A, B, C);
impl_path_params_tuple!(A, B, C, D);
impl_path_params_tuple!(A, B, C, D, E);
impl_path_params_tuple!(A, B, C, D, E, F);
impl_path_params_tuple!(A, B, C, D, E, F, G);
impl_path_params_tuple!(A, B, C, D, E, F, G, H);

// ---------------------------------------------------------------------------
// ElmQueryStruct: derived from ElmType field info
// ---------------------------------------------------------------------------

impl<T: ElmType> ElmQueryStruct for T {
    fn query_params() -> Vec<QueryParam> {
        T::elm_type_info()
            .fields()
            .iter()
            .map(|f| QueryParam {
                name: f.rust_name,
                ty: unwrap_maybe(&f.elm_type).clone(),
                required: !f.is_optional,
            })
            .collect()
    }
}

fn unwrap_maybe(t: &ElmTypeRepr) -> &ElmTypeRepr {
    match t {
        ElmTypeRepr::Maybe(inner) => inner,
        other => other,
    }
}

// ---------------------------------------------------------------------------
// External-crate ElmScalar impls (behind opt-in features)
// ---------------------------------------------------------------------------

#[cfg(feature = "uuid")]
impl ElmScalar for uuid::Uuid {
    fn elm_scalar_type() -> ElmTypeRepr {
        ElmTypeRepr::String
    }
}

// ---------------------------------------------------------------------------
// Axum-version-agnostic impls
// ---------------------------------------------------------------------------
//
// `String` and `()` aren't axum types, but the original module gated them
// behind the axum feature because they only make sense as part of an axum
// handler signature. We keep that gate (via "any axum version") so the
// surface stays identical for callers that opt in.

#[cfg(any(feature = "axum-0-6", feature = "axum-0-7", feature = "axum-0-8"))]
mod axum_common_impls {
    use super::{
        BodyKind, ElmExtractor, ElmResponse, ElmTypeRepr, ExtractorInfo, ResponseInfo, ResponseKind,
    };

    /// Bare `String` extractor: plain-text request body. The Elm
    /// `body` field is just `String`.
    impl ElmExtractor for String {
        fn elm_extractor_info() -> ExtractorInfo {
            ExtractorInfo::Body {
                kind: BodyKind::Text,
                ty: ElmTypeRepr::String,
            }
        }
    }

    impl ElmResponse for String {
        fn elm_response_info() -> ResponseInfo {
            ResponseInfo {
                success: Some(ElmTypeRepr::String),
                success_kind: ResponseKind::Text,
                error: None,
            }
        }
    }

    impl ElmResponse for () {
        fn elm_response_info() -> ResponseInfo {
            ResponseInfo {
                success: None,
                success_kind: ResponseKind::Empty,
                error: None,
            }
        }
    }

    /// Blanket: any `Result<T, E>` whose `T` is itself an `ElmResponse`
    /// reports the same shape — error bodies are user-defined and decoded
    /// out-of-band.
    impl<T: ElmResponse, E> ElmResponse for Result<T, E> {
        fn elm_response_info() -> ResponseInfo {
            T::elm_response_info()
        }
    }
}

// ---------------------------------------------------------------------------
// Axum-version-specific impls
// ---------------------------------------------------------------------------
//
// Each block below targets one axum minor version. `$axum` is the renamed
// crate (`axum_06`, `axum_07`, `axum_08`); the module name keeps the version
// in scope at the symbol level too, so error messages point at the right
// version.

#[allow(unused_macros)]
macro_rules! impl_axum_extractors_and_responses {
    ($mod_name:ident, $axum:ident) => {
        mod $mod_name {
            use super::{
                BodyKind, ElmExtractor, ElmPathParams, ElmQueryStruct, ElmResponse, ElmType,
                ElmTypeRepr, ExtractorInfo, ResponseInfo, ResponseKind,
            };

            // -----------------------------------------------------------------
            // Request extractors
            // -----------------------------------------------------------------

            /// Application state never appears in the Elm signature.
            impl<T> ElmExtractor for $axum::extract::State<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::Skip
                }
            }

            /// `Json<T>` becomes a JSON request body. `T` must derive
            /// `ElmType`. The body type is composed via `elm_type_repr()`
            /// so wrappers like `Vec<T>` render as `List T` at the call
            /// site.
            impl<T: ElmType> ElmExtractor for $axum::Json<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::Body {
                        kind: BodyKind::Json,
                        ty: T::elm_type_repr(),
                    }
                }
            }

            /// `Form<T>` becomes a form-encoded body. `T` must derive
            /// `ElmType`; downstream RequestStyle decides how to encode it.
            impl<T: ElmType> ElmExtractor for $axum::extract::Form<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::Body {
                        kind: BodyKind::Form,
                        ty: T::elm_type_repr(),
                    }
                }
            }

            /// `Bytes` extractor: raw binary request body. The Elm `body`
            /// field is `Bytes.Bytes` (from `elm/bytes`).
            impl ElmExtractor for $axum::body::Bytes {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::Body {
                        kind: BodyKind::Bytes,
                        // Surface a marker custom name so downstream codegen
                        // can recognize "this is bytes" without conflating it
                        // with String. Builders match on `BodyKind::Bytes`
                        // directly; this name is informational.
                        ty: ElmTypeRepr::Custom("Bytes.Bytes".to_string()),
                    }
                }
            }

            /// `Query<T>` expands to one `QueryParam` per field of `T`.
            impl<T: ElmQueryStruct> ElmExtractor for $axum::extract::Query<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::QueryParams(T::query_params())
                }
            }

            /// `Path<T>` expands to one or more `PathParam`s.
            impl<T: ElmPathParams> ElmExtractor for $axum::extract::Path<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::PathParams(T::path_params())
                }
            }

            // -----------------------------------------------------------------
            // Response types
            //
            // The matrix here covers axum's standard return shapes. For each
            // payload kind (Json<T> / Html<T> / Bytes / StatusCode) we cover
            // the cross product of:
            //
            //   - bare T
            //   - (StatusCode, T)
            //   - Result<T, E>
            //   - Result<(StatusCode, T), E>
            //
            // (The `Result<_, E>` blanket lives in `axum_common_impls`; the
            // `(StatusCode, _)` blanket lives here because StatusCode is an
            // axum re-export.)
            // -----------------------------------------------------------------

            fn json_response<T: ElmType>() -> ResponseInfo {
                if T::IS_UNIT {
                    return ResponseInfo {
                        success: None,
                        success_kind: ResponseKind::Empty,
                        error: None,
                    };
                }
                ResponseInfo {
                    success: Some(T::elm_type_repr()),
                    success_kind: ResponseKind::Json,
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

            fn empty_response() -> ResponseInfo {
                ResponseInfo {
                    success: None,
                    success_kind: ResponseKind::Empty,
                    error: None,
                }
            }

            fn bytes_response() -> ResponseInfo {
                ResponseInfo {
                    success: Some(ElmTypeRepr::Custom("Bytes.Bytes".to_string())),
                    success_kind: ResponseKind::Bytes,
                    error: None,
                }
            }

            /// `Json<T>` covers any `T: ElmType`. The unit case (`Json<()>`)
            /// is handled via `T::IS_UNIT` inside `json_response`, which
            /// short-circuits to an empty body without needing a separate
            /// (and orphan-rule-troublesome) concrete impl.
            impl<T: ElmType> ElmResponse for $axum::Json<T> {
                fn elm_response_info() -> ResponseInfo {
                    json_response::<T>()
                }
            }

            impl<T> ElmResponse for $axum::response::Html<T> {
                fn elm_response_info() -> ResponseInfo {
                    text_response()
                }
            }

            impl ElmResponse for $axum::http::StatusCode {
                fn elm_response_info() -> ResponseInfo {
                    empty_response()
                }
            }

            impl ElmResponse for $axum::body::Bytes {
                fn elm_response_info() -> ResponseInfo {
                    bytes_response()
                }
            }

            /// `(StatusCode, T)` adopts `T`'s response shape. Combined with
            /// the `Result<T, E>` blanket in `axum_common_impls`, this covers
            /// `Result<(StatusCode, T), E>` for free.
            impl<T: ElmResponse> ElmResponse for ($axum::http::StatusCode, T) {
                fn elm_response_info() -> ResponseInfo {
                    T::elm_response_info()
                }
            }
        }
    };
}

#[cfg(feature = "axum-0-6")]
impl_axum_extractors_and_responses!(axum_06_impls, axum_06);
#[cfg(feature = "axum-0-7")]
impl_axum_extractors_and_responses!(axum_07_impls, axum_07);
#[cfg(feature = "axum-0-8")]
impl_axum_extractors_and_responses!(axum_08_impls, axum_08);

// ---------------------------------------------------------------------------
// axum-extra: Query<T> with repeated keys
// ---------------------------------------------------------------------------
//
// `axum_extra::extract::Query<T>` has the same shape as
// `axum::extract::Query<T>` but supports repeated keys for `Vec<_>` fields.
// From the Elm side the param list is identical so we forward to the same
// extractor info.

#[allow(unused_macros)]
macro_rules! impl_axum_extra_query {
    ($mod_name:ident, $axum_extra:ident) => {
        mod $mod_name {
            use super::{ElmExtractor, ElmQueryStruct, ExtractorInfo};

            impl<T: ElmQueryStruct> ElmExtractor for $axum_extra::extract::Query<T> {
                fn elm_extractor_info() -> ExtractorInfo {
                    ExtractorInfo::QueryParams(T::query_params())
                }
            }
        }
    };
}

#[cfg(feature = "axum-extra-0-7")]
impl_axum_extra_query!(axum_extra_07_impls, axum_extra_07);
#[cfg(feature = "axum-extra-0-9")]
impl_axum_extra_query!(axum_extra_09_impls, axum_extra_09);
#[cfg(feature = "axum-extra-0-10")]
impl_axum_extra_query!(axum_extra_010_impls, axum_extra_010);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use elm_client_gen_core::ElmType;

    #[derive(ElmType)]
    #[elm(module = "Api.Filters", name = "PersonFilters")]
    #[allow(dead_code)]
    struct PersonFilters {
        name: Option<String>,
        active: bool,
    }

    #[test]
    fn elm_scalar_primitives_map_correctly() {
        assert_eq!(
            <String as ElmScalar>::elm_scalar_type(),
            ElmTypeRepr::String
        );
        assert_eq!(<i32 as ElmScalar>::elm_scalar_type(), ElmTypeRepr::Int);
        assert_eq!(<u64 as ElmScalar>::elm_scalar_type(), ElmTypeRepr::Int);
        assert_eq!(<f32 as ElmScalar>::elm_scalar_type(), ElmTypeRepr::Float);
        assert_eq!(<bool as ElmScalar>::elm_scalar_type(), ElmTypeRepr::Bool);
    }

    #[test]
    fn path_params_single_scalar_has_empty_name() {
        let params = <String as ElmPathParams>::path_params();
        assert_eq!(params.len(), 1);
        let first = params.first().expect("one path param");
        assert_eq!(first.name, "");
        assert_eq!(first.ty, ElmTypeRepr::String);
    }

    #[test]
    fn path_params_tuple_lists_each_element_in_order() {
        let params = <(String, i32) as ElmPathParams>::path_params();
        assert_eq!(params.len(), 2);
        let first = params.first().expect("first path param");
        let second = params.get(1).expect("second path param");
        assert_eq!(first.ty, ElmTypeRepr::String);
        assert_eq!(second.ty, ElmTypeRepr::Int);
        assert!(params.iter().all(|p| p.name.is_empty()));
    }

    #[test]
    fn query_struct_blanket_impl_uses_field_metadata() {
        let params = <PersonFilters as ElmQueryStruct>::query_params();
        let by_name: std::collections::HashMap<_, _> = params.iter().map(|p| (p.name, p)).collect();

        let name = by_name.get("name").expect("name param missing");
        assert_eq!(name.ty, ElmTypeRepr::String);
        assert!(!name.required);

        let active = by_name.get("active").expect("active param missing");
        assert_eq!(active.ty, ElmTypeRepr::Bool);
        assert!(active.required);
    }
}

// Per-axum-version smoke tests. These are compiled against whichever single
// axum version is currently selected. Run with e.g.
// `cargo test --no-default-features --features "derive,axum-0-7"`.

#[cfg(all(
    test,
    any(feature = "axum-0-6", feature = "axum-0-7", feature = "axum-0-8")
))]
mod axum_tests {
    use super::*;
    use elm_client_gen_core::ElmType;

    #[cfg(feature = "axum-0-6")]
    use axum_06 as axum;
    #[cfg(feature = "axum-0-7")]
    use axum_07 as axum;
    #[cfg(feature = "axum-0-8")]
    use axum_08 as axum;

    #[derive(ElmType)]
    #[elm(module = "Api.Person", name = "Person")]
    #[allow(dead_code)]
    struct Person {
        id: String,
        name: String,
    }

    #[test]
    fn state_extractor_is_skip() {
        let info = <axum::extract::State<()> as ElmExtractor>::elm_extractor_info();
        assert!(matches!(info, ExtractorInfo::Skip));
    }

    #[test]
    fn json_extractor_is_body_with_custom_type_name() {
        let info = <axum::Json<Person> as ElmExtractor>::elm_extractor_info();
        match info {
            ExtractorInfo::Body {
                kind: BodyKind::Json,
                ty: ElmTypeRepr::Custom(name),
            } => {
                assert_eq!(name, "Person");
            }
            other => panic!(
                "expected Body {{ Json, Custom(\"Person\") }}, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn string_extractor_is_text_body() {
        let info = <String as ElmExtractor>::elm_extractor_info();
        assert!(matches!(
            info,
            ExtractorInfo::Body {
                kind: BodyKind::Text,
                ty: ElmTypeRepr::String
            },
        ));
    }

    #[test]
    fn bytes_extractor_is_bytes_body() {
        let info = <axum::body::Bytes as ElmExtractor>::elm_extractor_info();
        assert!(matches!(
            info,
            ExtractorInfo::Body {
                kind: BodyKind::Bytes,
                ..
            },
        ));
    }

    #[test]
    fn path_extractor_delegates_to_elm_path_params() {
        let info = <axum::extract::Path<(String, String)> as ElmExtractor>::elm_extractor_info();
        match info {
            ExtractorInfo::PathParams(params) => {
                assert_eq!(params.len(), 2);
                assert_eq!(
                    params.first().expect("first path param").ty,
                    ElmTypeRepr::String
                );
                assert_eq!(
                    params.get(1).expect("second path param").ty,
                    ElmTypeRepr::String
                );
            }
            other => panic!("expected PathParams, got {:?}", other),
        }
    }

    #[test]
    fn result_json_response_carries_success_type() {
        let info = <Result<axum::Json<Person>, ()> as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Json);
        match info.success {
            Some(ElmTypeRepr::Custom(name)) => assert_eq!(name, "Person"),
            other => panic!("expected Some(Custom(\"Person\")), got {:?}", other),
        }
        assert!(info.error.is_none());
    }

    #[test]
    fn status_code_with_json_carries_success_type() {
        let info = <Result<(axum::http::StatusCode, axum::Json<Person>), ()> as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Json);
        assert!(matches!(info.success, Some(ElmTypeRepr::Custom(_))));
    }

    #[test]
    fn string_responses_are_text_kind() {
        let info = <Result<String, ()> as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Text);
        assert_eq!(info.success, Some(ElmTypeRepr::String));
    }

    #[test]
    fn unit_response_is_empty_kind() {
        let info = <Result<(), ()> as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Empty);
        assert!(info.success.is_none());
    }

    #[test]
    fn status_code_alone_is_empty_kind() {
        let info = <axum::http::StatusCode as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Empty);
        assert!(info.success.is_none());
    }

    #[test]
    fn bytes_response_is_bytes_kind() {
        let info = <Result<axum::body::Bytes, ()> as ElmResponse>::elm_response_info();
        assert_eq!(info.success_kind, ResponseKind::Bytes);
    }
}
