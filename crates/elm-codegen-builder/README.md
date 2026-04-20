# elm-codegen-builder

Build [`elm-ast`](https://crates.io/crates/elm-ast) modules (records,
decoders, encoders, imports, and optional HTTP request functions) from
`elm-codegen-core` and `elm-codegen-http` metadata, then render them
with `elm_ast::pretty_print` to emit `elm-format`-style source.

## Types and decoders/encoders

```rust
use elm_codegen_builder::{
    build_merged_module, group_by_module,
    DefaultStrategy, MaybeEncoderRef, NameMap, TypeOverrides,
};
use elm_codegen_core::registered_types;

let mut overrides = TypeOverrides::new();
overrides.alias("BigDecimal", elm_codegen_core::ElmTypeRepr::String);

let types: Vec<_> = registered_types()
    .into_iter()
    .map(|t| overrides.apply(t))
    .collect();
let names = NameMap::from_types(&types);
let strategy = DefaultStrategy;
let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

for (module_path, group) in group_by_module(&types) {
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    println!("{}", elm_ast::pretty_print(&module));
}
```

`build_merged_module` produces one Elm module with:

- one `type alias` or `type` per `ElmTypeInfo`,
- optionally a `{name}Decoder` function (gated by
  `BuildStrategy::should_emit_decoder`),
- optionally an `encode{Name}` function (gated by
  `BuildStrategy::should_emit_encoder`),
- deduplicated imports with unioned `exposing` sets,
- any private helpers the generated code needs (see
  "Auto-emitted private helpers" below).

## HTTP request functions

If you've annotated Axum handlers with `#[elm_endpoint(...)]`, the
builder can emit one Elm request function per handler. Feed each
registration through `normalize_endpoint` to produce `EndpointSlots`,
then pass those to a `RequestStyle` impl:

```rust
use elm_codegen_builder::{
    merge_request_into_module, normalize_endpoint, DefaultRequestStyle,
    NameMap, RequestStyle,
};
use elm_codegen_http::registered_endpoints;

let style = DefaultRequestStyle;

for info in registered_endpoints() {
    let endpoint = normalize_endpoint(&info);
    let output = style.build_request_function(
        &endpoint,
        &names,
        endpoint.elm_module_path,
    );
    // Merge into the already-built types module for this path...
    merge_request_into_module(&mut module, output);
    // ...or write `output.declarations` / `output.imports` into its
    // own module if you'd rather separate request functions from types.
}
```

`normalize_endpoint` resolves path slots (named slots match by name;
anonymous slots from `Path<(A, B)>` are zipped positionally),
flattens query/body/headers, and drops `Skip` extractors. It panics
with `handler_name` on cross-check failures (extra/missing slots,
unknown tuple arity); codegen is build-time tooling so panicking is
the correct failure mode.

## Configuration

### `BuildStrategy`

Per-type policy hooks. `DefaultStrategy` emits both a decoder and an
encoder for every type, which isn't always what you want. Read-only
response types don't need encoders, and query filter types don't need
decoders. Implement the trait to skip declarations based on the type
name, module path, or the `tags` set with `#[elm(tags = "...")]`:

```rust
use elm_codegen_builder::BuildStrategy;
use elm_codegen_core::ElmTypeInfo;

struct MyStrategy;

impl BuildStrategy for MyStrategy {
    fn should_emit_encoder(&self, info: &ElmTypeInfo) -> bool {
        // Skip `encodeFoo` for any type tagged "readonly".
        !info.has_tag("readonly")
    }
}
```

Tags are deliberately just strings. Pick whatever vocabulary fits
your codebase (`"response"`, `"input"`, `"filter"`, `"upsert"`, etc.)
and let your strategy decide what they mean.

### `TypeOverrides`

Codebase-wide rewrites applied to `ElmTypeRepr::Custom(_)` references.
Useful for mapping project-specific Rust types (e.g. `BigDecimal`) to
Elm primitives without per-field annotations:

```rust
let mut overrides = TypeOverrides::new();
overrides.alias("BigDecimal", ElmTypeRepr::String);
overrides.alias("CentAmount", ElmTypeRepr::Int);
```

Rewrites recurse through `Maybe` and `List`. Per-field
`#[elm(type = "...")]` overrides take precedence, since they are
applied at derive time before the builder ever sees the
`ElmTypeInfo`.

### `MaybeEncoderRef`

Elm's `Json.Encode` module doesn't ship a built-in helper for encoding
a `Maybe a`. Most projects pull one in — usually
[`Json.Encode.Extra.maybe`](https://package.elm-lang.org/packages/elm-community/json-extra/latest/Json-Encode-Extra#maybe)
from `elm-community/json-extra`, but you may have your own helper in a
project module instead. `MaybeEncoderRef` tells the builder which one
to call, and the matching `import` line is added automatically:

```rust
// Default: Json.Encode.Extra.maybe from elm-community/json-extra.
let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

// Or your own helper at, say, Api.Generated.Encode.encodeMaybe:
let maybe = MaybeEncoderRef::new(
    vec!["Api", "Generated", "Encode"],
    "encodeMaybe",
);
```

### `NameMap`

Resolves `ElmTypeRepr::Custom("PersonApi")` references to their Elm
type name and module path so the builder can emit correct imports.
`NameMap::from_types(&types)` populates it from your registered
types; `NameMap::register(...)` lets you add hand-written modules the
generator wouldn't otherwise see (shared helper modules, types defined
outside the derive set, etc.).

### `RequestStyle`

Controls how one `EndpointSlots` becomes one Elm function declaration.
`DefaultRequestStyle` emits a plain `elm/http` `Http.request` call and
dispatches on `BodyKind` / `ResponseKind` to pick the right
`Http.xBody` and `Http.expectX`. It skips query params and headers and
treats the error body as opaque.

Projects with their own request conventions (auto-injected auth
headers, tagged error envelopes, rate limiting) should implement
`RequestStyle`:

```rust
use elm_codegen_builder::{
    EndpointSlots, NameMap, RequestFunctionOutput, RequestStyle,
};

struct MyStyle;

impl RequestStyle for MyStyle {
    fn build_request_function(
        &self,
        endpoint: &EndpointSlots<'_>,
        names: &NameMap,
        current_module: &[&str],
    ) -> RequestFunctionOutput {
        // ...build an Elm function declaration plus its imports...
        unimplemented!()
    }
}
```

## Auto-emitted private helpers

Some Elm output can't be produced with a single `Encode.object` call.
For internally-tagged enums (`#[serde(tag = "...")]`) whose newtype
variants carry an inner encoded object, serde flattens the inner
object's fields next to the tag on the wire. To match that without
pulling in a JSON library at runtime, the builder emits a small
private helper into the generated module:

```elm
mergeTaggedObject : String -> String -> Value -> Value
mergeTaggedObject tagKey tagValue inner =
    Encode.object
        (( tagKey, Encode.string tagValue )
            :: Result.withDefault []
                (Decode.decodeValue (Decode.keyValuePairs Decode.value) inner)
        )
```

The helper is module-private and only emitted when a type in the
module actually needs it. No action is required from consumers.

## Public API summary

### Types and codecs

- [`build_merged_module`] — one Elm module per module-path group.
- [`group_by_module`] — partition `ElmTypeInfo`s by their target
  module.
- [`build_type_declaration`] / [`build_decoder`] / [`build_encoder`]
  — the per-type building blocks, if you want to compose modules
  yourself.
- [`BuildStrategy`] / [`DefaultStrategy`] — emission policy.
- [`TypeOverrides`] — codebase-wide custom-type rewrites.
- [`MaybeEncoderRef`] — where to find the project's `encodeMaybe`.
- [`NameMap`] / [`NameEntry`] — Rust-name → Elm-name/module
  resolution.

### HTTP endpoints

- [`normalize_endpoint`] — validated `ElmEndpointInfo` →
  `EndpointSlots`.
- [`EndpointSlots`] / [`PathSegment`] — the shape a `RequestStyle`
  consumes.
- [`RequestStyle`] / [`DefaultRequestStyle`] — render one endpoint.
- [`merge_request_into_module`] — splice a request function into an
  existing `ElmModule`, deduplicating imports.

### Helpers for ad-hoc AST construction

[`case_of`], [`import_as`], [`import_as_exposing`], [`import_exposing`],
[`list_multiline`], [`pipeline_chain`], [`record_access`],
[`record_multiline`], [`tqualified`], [`trecord`]. Thin wrappers over
`elm-ast` with the multi-line spans the pretty-printer needs.

## Stability

Pre-1.0. New variants on `ElmTypeRepr`, `ElmVariantPayload`, or the
request-side data model are breaking under pre-1.0 semver and will
bump the minor version.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
