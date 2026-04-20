# elm-codegen-http

HTTP endpoint metadata for [`elm-codegen`](https://github.com/joshburgess/elm-codegen).

`#[derive(ElmType)]` (from `elm-codegen-core`) describes Rust types
that should appear as Elm types. This crate adds the matching layer
for HTTP endpoints:

- the `#[elm_endpoint(...)]` attribute macro for handler functions,
- the `ElmExtractor` trait that maps each handler parameter (`Path<T>`,
  `Query<T>`, `Json<T>`, custom wrappers, skipped state extractors) to
  an Elm function parameter,
- the `ElmResponse` trait that maps each handler return type to an
  Elm response (success body, kind, optional error body),
- a link-time inventory of registered endpoints,
- optional Axum integration behind the `axum` feature.

Pair with `elm-codegen-builder`'s `RequestStyle` trait to render the
collected endpoints as Elm request functions in whatever shape your
project uses.

## Usage

```rust
use elm_codegen_http::elm_endpoint;

#[elm_endpoint(
    POST, "/api/v1/persons",
    module = "Api.Generated.Person",
    tags   = "upsert",
)]
async fn create_person(
    State(app): State<AppState>,
    Json(body): Json<CreatePersonApi>,
) -> Result<Json<PersonApi>, HttpError> { /* ... */ }
```

At codegen time, iterate `registered_endpoints()` and feed each one
into a `RequestStyle` impl to get back an Elm function declaration
plus its imports.

## Data model

### `ElmEndpointInfo`

The root record emitted by the macro:

```rust
pub struct ElmEndpointInfo {
    pub handler_name:      &'static str,                // Rust fn name (diagnostics)
    pub elm_function_name: &'static str,                // Elm fn name (defaults to camelCase)
    pub elm_module_path:   &'static [&'static str],     // e.g. ["Api", "Generated", "Person"]
    pub method:            HttpMethod,
    pub path_template:     &'static str,                // e.g. "/api/v1/persons/{person_id}"
    pub params:            Vec<ExtractorInfo>,          // one per handler parameter, in order
    pub response:          ResponseInfo,
    pub tags:              &'static [&'static str],
}
```

### Parameters (`ExtractorInfo`)

```rust
pub enum ExtractorInfo {
    PathParams(Vec<PathParam>),
    QueryParams(Vec<QueryParam>),
    Body { kind: BodyKind, ty: ElmTypeRepr },
    Header(HeaderInfo),
    Skip,                       // e.g. State<T>, sessions, request id
}
```

Body kinds drive which `Http.xBody` constructor the codegen emits:

| `BodyKind` | Wire | Elm body field type |
| --- | --- | --- |
| `Json` | `application/json` | the typed payload `T` |
| `Text` | `text/plain` | `String` |
| `Form` | `application/x-www-form-urlencoded` | the typed payload `T` |
| `Bytes` | `application/octet-stream` | `Bytes.Bytes` |

### Responses (`ResponseInfo`)

```rust
pub struct ResponseInfo {
    pub success:      Option<ElmTypeRepr>,   // None for Empty
    pub success_kind: ResponseKind,
    pub error:        Option<ElmTypeRepr>,   // None => opaque error body
}

pub enum ResponseKind { Json, Text, Empty, Bytes }
```

`Json` uses the success type's generated `...Decoder`; `Text` uses a
plain `String`; `Empty` skips the decoder and types `toMsg` as
`Result Http.Error () -> msg`; `Bytes` passes through raw bytes.

## Traits

### `ElmExtractor`

```rust
pub trait ElmExtractor {
    fn elm_extractor_info() -> ExtractorInfo;
}
```

Implement for any extractor type used on an annotated handler.

### `ElmResponse`

```rust
pub trait ElmResponse {
    fn elm_response_info() -> ResponseInfo;
}
```

Implement for handler return types. Wrap it yourself if you want
richer error decoding (see "Custom response types" below).

### `ElmPathParams`

```rust
pub trait ElmPathParams {
    fn path_params() -> Vec<PathParam>;
}
```

Helper for `Path<T>`: how a `T` decomposes into one or more path
parameters. Blanket impls provided:

- `impl<T: ElmScalar> ElmPathParams for T`: single-slot `Path<T>`.
- Tuple impls for arities 2 through 8 over `ElmScalar`.

Users implement this directly for single-typed structs pulled from
the path.

### `ElmQueryStruct`

```rust
pub trait ElmQueryStruct {
    fn query_params() -> Vec<QueryParam>;
}
```

Blanket-implemented for any `T: ElmType`. Each named field becomes
one `QueryParam`; `Option<T>` fields are marked `required: false`.
So `#[derive(ElmType)]` on the query struct is sufficient.

### `ElmScalar`

```rust
pub trait ElmScalar {
    fn elm_scalar_type() -> ElmTypeRepr;
}
```

Bundled impls: `String`, `bool`, `i8..=i64`, `u8..=u64`, `usize`,
`isize`, `f32`, `f64`. Add impls for newtype IDs that appear in path
slots (e.g. `PersonId`, `Sqid`).

## Features

- **`derive`** *(default)*: re-exports `#[elm_endpoint(...)]` from
  `elm-codegen-derive`.
- **`axum`**: enables the bundled impls for standard Axum extractors
  and response types.
- **`axum-extra`**: adds the `axum_extra::extract::Query<T>` impl
  (repeated-key query support). Implies `axum`.
- **`uuid`**: adds `ElmScalar for uuid::Uuid` (maps to Elm `String`).

## Bundled Axum impls (`axum` feature)

### Extractors

| Extractor | Produces |
| --- | --- |
| `axum::extract::State<T>` | `ExtractorInfo::Skip` (not shown in Elm signature) |
| `axum::Json<T: ElmType>` | `Body { kind: Json, ty: T::elm_type_repr() }` |
| `axum::extract::Form<T: ElmType>` | `Body { kind: Form, ty: T::elm_type_repr() }` |
| `axum::body::Bytes` | `Body { kind: Bytes, ty: "Bytes.Bytes" }` |
| `String` | `Body { kind: Text, ty: String }` |
| `axum::extract::Query<T: ElmQueryStruct>` | `QueryParams(T::query_params())` |
| `axum_extra::extract::Query<T>` *(axum-extra)* | `QueryParams(...)`, same shape |
| `axum::extract::Path<T: ElmPathParams>` | `PathParams(T::path_params())` |

### Responses

Each leaf response type has a single direct impl; the `(StatusCode, T)`
and `Result<T, E>` shapes are covered by two blanket impls, so adding
support for a custom success type means adding one impl on the leaf.

| Return type | Success | `ResponseKind` |
| --- | --- | --- |
| `axum::Json<T: ElmType>` | `Some(T::elm_type_repr())` | `Json` |
| `axum::Json<()>` | `None` | `Empty` *(via `IS_UNIT` on `()`)* |
| `String` | `Some(String)` | `Text` |
| `axum::response::Html<T>` | `Some(String)` | `Text` |
| `()` | `None` | `Empty` |
| `axum::http::StatusCode` | `None` | `Empty` |
| `axum::body::Bytes` | `Some("Bytes.Bytes")` | `Bytes` |
| `(StatusCode, T)` | same as `T` | same as `T` |
| `Result<T, E>` | same as `T` | same as `T` |

## Custom extractors

```rust
use elm_codegen_http::{ElmExtractor, ExtractorInfo, PathParam};
use elm_codegen_core::ElmTypeRepr;

pub struct AuthedUser(pub String);

impl ElmExtractor for AuthedUser {
    fn elm_extractor_info() -> ExtractorInfo {
        // Auth is injected by the request style; hide from Elm.
        ExtractorInfo::Skip
    }
}
```

## Custom response types

When your API wraps success and error in a custom envelope, implement
`ElmResponse` once on your leaf type:

```rust
pub struct ApiResult<T>(pub Result<T, ApiError>);

impl<T: ElmType> ElmResponse for ApiResult<T> {
    fn elm_response_info() -> ResponseInfo {
        ResponseInfo {
            success:      Some(T::elm_type_repr()),
            success_kind: ResponseKind::Json,
            error:        Some(ElmTypeRepr::Custom("ApiError".to_string())),
        }
    }
}
```

`Result<ApiResult<T>, E>` and `(StatusCode, ApiResult<T>)` both work
automatically via the blanket impls.

## The registry

```rust
pub fn registered_endpoints() -> Vec<ElmEndpointInfo>;
```

Iterates the global `inventory` collector. Call once from your codegen
binary. Iteration order follows link order; sort if you need
deterministic output.

As with `#[derive(ElmType)]`, any crate whose only contribution is
annotated handlers must be referenced from the binary that runs
codegen (`use my_handlers as _;`).

## Stability

Pre-1.0. The data model and trait surface are still moving. Pin to a
specific minor range (`"0.2"`) in your `Cargo.toml`.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
