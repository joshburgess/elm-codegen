# HTTP endpoint codegen: design sketch

Status: design draft. No code lands from this document.

Goal: extend the elm-codegen crates so a Rust web service can also emit
Elm HTTP request functions for its endpoints, with the same ergonomics
as `#[derive(ElmType)]` for types.

## Crate layout

```
elm-codegen-core           (unchanged)
elm-codegen-derive         (unchanged)
elm-codegen-builder        (extended: RequestStyle trait, default impl)
elm-codegen-http           (new: ElmExtractor, ElmResponse, attribute macro,
                                bundled framework impls under feature flags)
elm-codegen-cli            (extended: optional flag to also emit endpoints)
```

`elm-codegen-http` ships impls for popular framework types behind feature
flags (`axum`, `actix`, `rocket`). Pinata only enables `axum`.

## Source of truth: an attribute macro on the handler

```rust
#[elm_endpoint(
    GET, "/api/v1/organizations/{organization_id}/persons/{person_id}",
    module = ["Api", "Generated", "Person"],
    name   = "getPerson",        // optional; default = camelCase(fn name)
    tags("read"),                // optional; same role as ElmType tags
)]
async fn get_person(
    State(ctx):              State<ApiContext>,
    Extract(org_id):         Extract<OrganizationId>,
    Path(person_id):         Path<PersonId>,
    Query(filters):          Query<PersonFilters>,
) -> Result<Json<Person>, HttpError<()>> { ... }
```

The macro does no parsing of extractor types beyond textually copying them
into trait dispatches. All interpretation is done by trait impls.

What it expands to (sketch):

```rust
inventory::submit! {
    elm_codegen_http::ElmEndpointRegistration {
        build: || {
            let mut params = Vec::new();
            params.push(<State<ApiContext>        as ElmExtractor>::elm_extractor_info());
            params.push(<Extract<OrganizationId>  as ElmExtractor>::elm_extractor_info());
            params.push(<Path<PersonId>           as ElmExtractor>::elm_extractor_info());
            params.push(<Query<PersonFilters>     as ElmExtractor>::elm_extractor_info());

            ElmEndpointInfo {
                handler_name:      "get_person",
                elm_function_name: "getPerson",
                elm_module_path:   &["Api", "Generated", "Person"],
                method:            HttpMethod::Get,
                path_template:     "/api/v1/organizations/{organization_id}/persons/{person_id}",
                params,
                response: <Result<Json<Person>, HttpError<()>>
                              as ElmResponse>::elm_response_info(),
                tags:    &["read"],
            }
        }
    }
}
```

Drift surfaces and where each is caught:

| Surface                              | Caught at      |
|--------------------------------------|----------------|
| Extractor type lacks `ElmType` bound | rustc          |
| Custom extractor lacks `ElmExtractor`| rustc          |
| Path slot names don't match params   | codegen panic  |
| Path string differs from `.route()`  | router test    |

## Data model (in `elm-codegen-http`)

```rust
pub trait ElmExtractor {
    fn elm_extractor_info() -> ExtractorInfo;
}

pub trait ElmResponse {
    fn elm_response_info() -> ResponseInfo;
}

pub enum ExtractorInfo {
    PathParams(Vec<PathParam>),
    QueryParams(Vec<QueryParam>),
    Body(ElmTypeRef),
    Header { name: &'static str, ty: ElmTypeRef, required: bool },
    Skip,
}

pub struct PathParam  { pub name: &'static str, pub ty: ElmTypeRef }
pub struct QueryParam { pub name: &'static str, pub ty: ElmTypeRef, pub required: bool }
pub struct ResponseInfo { pub success: ElmTypeRef, pub error: Option<ElmTypeRef> }

pub enum HttpMethod { Get, Post, Put, Patch, Delete }

pub struct ElmEndpointInfo {
    pub handler_name:      &'static str,
    pub elm_function_name: &'static str,
    pub elm_module_path:   &'static [&'static str],
    pub method:            HttpMethod,
    pub path_template:     &'static str,
    pub params:            Vec<ExtractorInfo>,
    pub response:          ResponseInfo,
    pub tags:              &'static [&'static str],
}

pub struct ElmEndpointRegistration { pub build: fn() -> ElmEndpointInfo }
inventory::collect!(ElmEndpointRegistration);

pub fn registered_endpoints() -> Vec<ElmEndpointInfo> { ... }
```

Two helper traits cover the common generic extractors:

```rust
/// For `Query<T>`. T is a deserializable struct; each field becomes one
/// query param. Auto-derived from `#[derive(ElmType)]` field info.
pub trait ElmQueryStruct { fn query_params() -> Vec<QueryParam>; }

/// For `Path<T>`. T is either a single `ElmType` (one slot) or a tuple
/// (multiple slots, in path order).
pub trait ElmPathParams { fn path_params() -> Vec<PathParam>; }
```

## Bundled Axum impls (behind `axum` feature)

```rust
impl<T> ElmExtractor for axum::extract::State<T> {
    fn elm_extractor_info() -> ExtractorInfo { ExtractorInfo::Skip }
}

impl<T: ElmType> ElmExtractor for axum::Json<T> {
    fn elm_extractor_info() -> ExtractorInfo { ExtractorInfo::Body(ElmTypeRef::of::<T>()) }
}

impl<T: ElmQueryStruct> ElmExtractor for axum::extract::Query<T> {
    fn elm_extractor_info() -> ExtractorInfo { ExtractorInfo::QueryParams(T::query_params()) }
}

impl<T: ElmPathParams> ElmExtractor for axum::extract::Path<T> {
    fn elm_extractor_info() -> ExtractorInfo { ExtractorInfo::PathParams(T::path_params()) }
}

// Tuple impls of ElmPathParams for (A,), (A,B), ..., up to N=12.
// Each entry has name = "" -- the names are filled in at codegen time
// from the path template (positional zip).

// Standard scalar `ElmPathParams` impls:
impl<T: ElmType> ElmPathParams for (T,) { ... }

// Result<Json<T>, E>: success = T, error = E if E: ElmType, else None.
impl<T: ElmType, E: ElmType> ElmResponse for Result<axum::Json<T>, HttpError<E>> { ... }
impl<T: ElmType>             ElmResponse for Result<axum::Json<T>, HttpError<()>> { ... }
```

Pinata-side impls live next to the extractors they describe:

```rust
impl<T: ElmExtractor> ElmExtractor for crate::extractors::Extract<T> {
    fn elm_extractor_info() -> ExtractorInfo { T::elm_extractor_info() }
}

impl ElmExtractor for OrganizationId {
    fn elm_extractor_info() -> ExtractorInfo {
        ExtractorInfo::PathParams(vec![PathParam {
            name: "organization_id",
            ty:   ElmTypeRef::String,
        }])
    }
}

// ActiveSession is auth state, not part of the URL:
impl ElmExtractor for ActiveSession {
    fn elm_extractor_info() -> ExtractorInfo { ExtractorInfo::Skip }
}
```

## Codegen-time normalization (in `elm-codegen-builder`)

Raw `ElmEndpointInfo` is normalized into `EndpointSlots` before any
`RequestStyle` sees it.

Steps:

1. Split `path_template` into ordered segments (literals + `{slot}`s).
2. Flatten all `ExtractorInfo::PathParams` from `params`. For entries with
   an explicit name, look up the matching slot in the template; assert it
   exists. For entries with `name == ""`, zip them positionally against
   the unclaimed slots.
3. Assert: every template slot is claimed by exactly one extractor.
   Otherwise panic with `handler_name` and a diff.
4. Flatten `QueryParams`, `Body`, `Header` similarly.
5. Drop all `Skip` entries.

```rust
pub struct EndpointSlots<'a> {
    pub function_name: &'a str,
    pub method:        HttpMethod,
    pub path:          Vec<PathSegment<'a>>,
    pub query:         Vec<&'a QueryParam>,
    pub body:          Option<&'a ElmTypeRef>,
    pub headers:       Vec<&'a HeaderInfo>,
    pub success:       &'a ElmTypeRef,
    pub error:         Option<&'a ElmTypeRef>,
}

pub enum PathSegment<'a> { Literal(&'a str), Param(&'a PathParam) }
```

## RequestStyle trait

```rust
pub trait RequestStyle {
    /// Render one normalized endpoint as one Elm declaration
    /// (function + type signature, plus any helper imports it needs).
    fn build_request_function(
        &self,
        endpoint: &EndpointSlots<'_>,
        names:    &NameMap,
    ) -> RequestFunctionOutput;
}

pub struct RequestFunctionOutput {
    pub declarations: Vec<ElmDeclaration>,    // usually one fn
    pub imports:      Vec<ElmImport>,         // e.g. Http, Url.Builder
}
```

The crate ships a `DefaultRequestStyle` that emits a plain
`elm/http`-based `Http.request` call. Anyone who wants a different
shape implements the trait themselves.

## Pinata's `TheAppRequestStyle`

Lives in `bin/elm-codegen/main.rs` next to `PinataStrategy`.
Models the existing convention from `Api/Atom/Account.elm`:

```rust
struct TheAppRequestStyle;

impl RequestStyle for TheAppRequestStyle {
    fn build_request_function(
        &self,
        e: &EndpointSlots<'_>,
        names: &NameMap,
    ) -> RequestFunctionOutput { /* ... */ }
}
```

For the example handler at the top of this doc, output:

```elm
getPerson :
    { token : String
    , baseUrl : String
    , organizationId : String
    , personId : String
    , optionalQueryParameters : List ( String, Maybe QueryParam )
    , tracker : Maybe String
    }
    -> (Result.Result (Effect.Http.HttpError ()) Person -> msg)
    -> Effect.Effect msg
getPerson options onResponse =
    Effect.sendApiRequest
        ({ config =
            { endpoint =
                { baseUrl = options.baseUrl
                , path =
                    [ "api"
                    , "v1"
                    , "organizations"
                    , options.organizationId
                    , "persons"
                    , options.personId
                    ]
                , queryParams = []
                , optionalQueryParams =
                    Query.optionalQueryParams options.optionalQueryParameters
                }
            , decoder = personDecoder
            , errorDecoder = \_ -> Decode.succeed ()
            , onResponse = onResponse
            , token = Just options.token
            , headers = []
            , tracker = options.tracker
            }
         }
            |> Effect.GET
        )
```

Mapping rules used by this style:

| Endpoint feature        | Where it lands in the Elm signature        |
|-------------------------|--------------------------------------------|
| Path param              | Field on `options`, named in camelCase     |
| Required query param    | Field on `options`                         |
| Optional query param    | `optionalQueryParameters` list             |
| `Body`                  | Extra positional arg before `onResponse`   |
| `Header`                | Field on `options`                         |
| Success response type   | Decoder lookup: `<typeName>Decoder`        |
| Error response type     | Always `()` for now (matches current code) |
| Always present          | `token`, `baseUrl`, `tracker`              |

## Router cross-check (Pinata-side test)

```rust
#[test]
fn every_route_has_an_elm_endpoint_registration() {
    let registered: HashSet<(Method, String)> = registered_endpoints()
        .into_iter()
        .map(|e| (e.method.into(), e.path_template.to_string()))
        .collect();

    for route in walk_router(build_app()) {
        assert!(
            registered.contains(&(route.method, route.path)),
            "route {} {} has no #[elm_endpoint]",
            route.method, route.path,
        );
    }
}
```

## Open questions

1. **Error decoder.** Pinata's existing handlers all use `errorDecoder =
   \_ -> Decode.succeed ()`. Do we want to start emitting real error
   decoders from `HttpError<E>`, or stay matching the current style?

2. **Module placement.** Generated request modules collide with hand-written
   `Api.Atom.Account` modules today. Two options:
   (a) emit to `Api.Generated.Account` and let TheApp re-export selectively,
   (b) emit to `Api.Atom.Account.Generated` to keep them next to hand code.
   I'd lean (a) since it mirrors how generated types already land.

3. **Migration cadence.** Big-bang replace all hand-written request fns,
   or roll out one element type at a time and delete hand-written code as
   each is verified?

4. **Should the macro also generate the Axum `.route(...)` call?** Would
   eliminate the path-string drift surface entirely, at the cost of a
   more opinionated macro. Probably not worth it for v1.
