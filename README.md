# elm-client-gen

Generate Elm types, decoders, encoders, and (optionally) typed `Http`
request functions from your Rust types and Axum handlers.

Annotate a Rust struct with `#[derive(ElmType)]`, point a small CLI (or
your own binary) at the resulting registry, and get well-formatted
`.elm` modules out the other end. Built on top of
[`elm-ast`](https://crates.io/crates/elm-ast) for both AST
construction and layout-aware pretty-printing.

```rust
use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person", tags = "entity")]
pub struct PersonApi {
    pub id: String,
    pub full_name: String,
    pub nickname: Option<String>,
}
```

Run the CLI and you get `Api/Person.elm` containing:

```elm
module Api.Person exposing (Person, personDecoder, encodePerson)

import Json.Decode as Decode exposing (Decoder)
import Json.Decode.Pipeline exposing (required, optional)
import Json.Encode as Encode exposing (Value)
import Json.Encode.Extra exposing (maybe)


type alias Person =
    { id : String
    , fullName : String
    , nickname : Maybe String
    }


personDecoder : Decoder Person
personDecoder =
    Decode.succeed Person
        |> required "id" Decode.string
        |> required "fullName" Decode.string
        |> optional "nickname" (Decode.maybe Decode.string) Nothing


encodePerson : Person -> Value
encodePerson value =
    Encode.object
        [ ( "id", Encode.string value.id )
        , ( "fullName", Encode.string value.fullName )
        , ( "nickname", maybe Encode.string value.nickname )
        ]
```

## Crates

| Crate | Purpose |
| --- | --- |
| [`elm-client-gen-core`](crates/elm-client-gen-core) | Core types (`ElmTypeInfo`, `ElmTypeRepr`, `ElmType` trait) and the `inventory`-backed registry. Re-exports `#[derive(ElmType)]` behind the default `derive` feature. |
| [`elm-client-gen-derive`](crates/elm-client-gen-derive) | The `#[derive(ElmType)]` proc-macro plus the `#[elm_endpoint(...)]` attribute macro. Pulled in transitively via the `derive` feature on `elm-client-gen-core` and `elm-client-gen-http`. |
| [`elm-client-gen-http`](crates/elm-client-gen-http) | HTTP endpoint metadata: the `#[elm_endpoint(...)]` registry plus the `ElmExtractor` / `ElmResponse` traits. Optional Axum integration behind the `axum` feature. |
| [`elm-client-gen-builder`](crates/elm-client-gen-builder) | Turns `ElmTypeInfo` and `ElmEndpointInfo` into `elm-ast` modules. Configurable via `BuildStrategy`, `TypeOverrides`, `MaybeEncoderRef`, and a pluggable `RequestStyle`. |
| [`elm-client-gen-cli`](crates/elm-client-gen-cli) | Reference binary. Walks the registry, applies overrides, writes one `.elm` file per Elm module path. |

## Quick start

### 1. Annotate your schema types

In whichever crate defines your API DTOs, add `elm-client-gen-core` and
annotate the types you want exported:

```toml
# my-schema/Cargo.toml
[dependencies]
elm-client-gen-core = "0.2"
```

```rust
use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person")]
pub struct PersonApi { /* ... */ }
```

The `derive` feature is on by default, so `#[derive(ElmType)]` is
re-exported from `elm-client-gen-core` directly (you don't need to depend
on `elm-client-gen-derive`).

### 2. Run codegen

You have two options.

**Option A: Roll your own binary in your workspace.** This is the
recommended path if you need any customization (codebase-wide type
overrides, per-tag emission rules, a custom `encodeMaybe` location, or
Elm request functions for HTTP endpoints):

```toml
# my-codegen/Cargo.toml
[dependencies]
elm-client-gen-core = "0.2"
elm-client-gen-builder = "0.2"
my-schema = { path = "../my-schema" }
```

```rust
// my-codegen/src/main.rs
use std::fs;
use std::path::PathBuf;

use my_schema as _;   // force-link the schema crate (see "Linking note" below)

use elm_client_gen_builder::{
    build_merged_module, group_by_module,
    DefaultStrategy, MaybeEncoderRef, NameMap, TypeOverrides,
};
use elm_client_gen_core::registered_types;

fn main() -> std::io::Result<()> {
    let out_dir = PathBuf::from("./elm/src");

    let overrides = TypeOverrides::new();
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let mut types: Vec<_> = registered_types()
        .into_iter()
        .map(|t| overrides.apply(t))
        .collect();
    // `inventory` yields types in link order, which shifts whenever
    // dependencies change. Sort by (module_path, type_name) so the
    // emitted output stays stable across unrelated rebuilds.
    types.sort_by(|a, b| {
        a.module_path
            .cmp(&b.module_path)
            .then_with(|| a.type_name.cmp(b.type_name))
    });
    let names = NameMap::from_types(&types);

    // `group_by_module` returns a `BTreeMap`, so modules are already
    // iterated in sorted module-path order. For very large schemas,
    // swap this loop for `rayon::prelude::*` + `.into_par_iter()` on
    // the grouped map to write files in parallel instead. Each module
    // writes to a distinct file, so the output is unchanged.
    for (module_path, group) in group_by_module(&types) {
        let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);

        // Map ["Api", "Person"] -> ./elm/src/Api/Person.elm.
        let mut file_path = out_dir.clone();
        for segment in &module_path {
            file_path.push(segment);
        }
        file_path.set_extension("elm");

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, elm_ast::pretty_print(&module))?;
    }
    Ok(())
}
```

**Option B: Use the reference CLI for the simplest cases.** Install
with `cargo install elm-client-gen-cli`, then run `elm-client-gen --output
./elm/src`. Note that the published `elm-client-gen` binary doesn't
import any user crate, so it can only see types defined in crates it
links to. In practice this means you'll still want a thin wrapper
binary in your workspace (Option A) unless your schema lives in the
same crate as your codegen entry point.

## Linking note

Because `#[derive(ElmType)]` registers types via `inventory`'s
link-time collector, any crate that *only* contributes derived types
must be referenced from the binary that runs codegen. The idiomatic
pattern is `use my_schema_crate as _;` at the top of `main.rs`. Without
this, rustc dead-strips the rlib and you get an empty registry.

The same caveat applies to `#[elm_endpoint(...)]` on Axum handlers.

## Supported Rust shapes

**Record structs** map to Elm type aliases with named fields, one
decoder, and one encoder.

**Single-field tuple structs** (`struct UserId(String)`) become
transparent Elm `type alias`es over the inner type with delegating
codecs, so you can reference the named type without the per-field
`#[elm(type = "...")]` ceremony.

**Enums** map to Elm custom types. Three serde representations are
supported:

| Serde representation | Attribute | Allowed variant shapes |
| --- | --- | --- |
| Internally tagged | `#[serde(tag = "type")]` | unit, newtype, struct |
| Untagged | `#[serde(untagged)]` | unit, newtype, struct |
| Bare-string | *(default, unit-only)* | unit |

Serde's default externally-tagged shape (non-unit enums without any
serde attribute) isn't currently supported; add `#[serde(tag = "...")]`
or `#[serde(untagged)]` to pick a representation.

Internally-tagged **newtype variants** (e.g. `Ready(DocumentRef)` where
`DocumentRef` itself is internally tagged) work: the builder emits a
small private `mergeTaggedObject` helper that flattens the inner
object's fields next to the tag key, matching serde's wire format.

**Multi-field tuple variants** (`Variant(A, B)`) are rejected because
they have no canonical JSON shape. Use a struct variant instead.

**Generic types** (`struct Page<T>`) are rejected because the registry
materializes one Elm declaration per Rust type, with no way to
monomorphize. Define a concrete wrapper per instantiation (e.g.
`pub struct PageOfPerson(Page<PersonApi>)`) or hand-write the Elm
types for that case.

## Built-in type mapping

| Rust | Elm |
| --- | --- |
| `String` | `String` |
| `bool` | `Bool` |
| `i8..=i64`, `u8..=u64`, `usize`, `isize` | `Int` |
| `f32`, `f64` | `Float` |
| `Option<T>` | `Maybe T` (emitted as `optional` in the decoder) |
| `Vec<T>` | `List T` |
| `HashMap<String, V>` / `BTreeMap<String, V>` | `Dict String V` |
| `(A, B)` / `(A, B, C)` | `(A, B)` / `(A, B, C)` Elm tuple |
| `chrono::DateTime<_>` (matched by final ident) | `Time.Posix` |
| `chrono::NaiveDate` (matched by final ident) | `String` (ISO date) |
| `chrono::NaiveDateTime`, `chrono::NaiveTime` | `String` (bare ISO, no tz) |
| `uuid::Uuid` (matched by final ident) | `String` |
| `serde_json::Value` (matched by final ident) | `Json.Encode.Value` |
| `Box<T>`, `Arc<T>`, `Rc<T>`, `Cow<_, T>` | same as `T` (transparent) |
| anything else | `ElmTypeRepr::Custom("TypeName")` (resolved via the builder's `NameMap`) |

Non-`String` map keys are rejected at derive time because JSON object
keys are always strings; use `#[elm(type = "Encode.Value")]` if you
need an opaque blob there.

## HTTP endpoints

Annotate Axum handlers with `#[elm_endpoint(...)]` to register them
alongside your types. The builder's `RequestStyle` trait turns each
registration into one Elm request function per handler, plus the
imports needed:

```rust
#[elm_client_gen_http::elm_endpoint(
    GET, "/api/v1/persons/{person_id}",
    module = "Api.Generated.Person",
    name   = "getPerson",
    tags   = "read",
)]
async fn get_person(
    Path(person_id): Path<String>,
    Query(filters): Query<PersonFilters>,
    State(app): State<AppState>,
) -> Result<Json<PersonApi>, HttpError> { /* ... */ }
```

Extractors are inspected via `ElmExtractor` trait impls: `Path<T>` and
`Query<T>` expand into path and query parameters, `Json<T>` / `Form<T>`
/ `String` / `Bytes` become typed request bodies, and `State<T>` (and
anything else you tag with `ExtractorInfo::Skip`) is ignored.

See `elm-client-gen-http` for the full trait surface and bundled Axum
impls.

## Customization

### Per-field type overrides

```rust
#[derive(ElmType)]
#[elm(module = "Api.Order", name = "Order")]
pub struct OrderApi {
    #[elm(type = "Money")]
    pub total: BigDecimal,
}
```

### Codebase-wide rewrites

```rust
let mut overrides = TypeOverrides::new();
overrides.alias("BigDecimal", ElmTypeRepr::String);

let types: Vec<_> = registered_types()
    .into_iter()
    .map(|t| overrides.apply(t))
    .collect();
```

### Per-type emission policy

By default every registered type gets both a decoder and an encoder.
That isn't always what you want: response/read-only types never need
to be serialized back into JSON, and query filter types never need to
be parsed *out* of JSON. Implement `BuildStrategy` to skip the
declarations you don't need.

The hooks receive the full `ElmTypeInfo`, so you can branch on the
type name, the module path, or the free-form `tags` you set with
`#[elm(tags = "...")]` on the struct:

```rust
#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person", tags = "readonly")]
pub struct PersonApi { /* ... */ }

struct MyStrategy;

impl BuildStrategy for MyStrategy {
    fn should_emit_encoder(&self, info: &ElmTypeInfo) -> bool {
        // Skip `encodePerson` for any type tagged "readonly".
        !info.has_tag("readonly")
    }
}
```

`tags` are deliberately just strings. Pick whatever vocabulary fits
your codebase (`"response"`, `"input"`, `"filter"`, `"upsert"`, etc.)
and let your strategy decide what they mean.

### Choosing where `encodeMaybe` lives

Elm's `Json.Encode` module doesn't ship a built-in helper for
encoding a `Maybe a`. Most projects pull one in, usually
[`Json.Encode.Extra.maybe`](https://package.elm-lang.org/packages/elm-community/json-extra/latest/Json-Encode-Extra#maybe)
from `elm-community/json-extra`, but you may have your own helper in a
project module instead. `MaybeEncoderRef` tells the builder which one
to call.

```rust
// Default: Json.Encode.Extra.maybe from elm-community/json-extra.
let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

// Or your own helper at, say, Api.Generated.Encode.encodeMaybe:
let maybe = MaybeEncoderRef::new(
    vec!["Api", "Generated", "Encode"],
    "encodeMaybe",
);
```

The builder both emits the call (`maybe Encode.string value.nickname`)
and adds the matching `import` line.

### Custom request styles

If your project wraps `elm/http` in a project-specific helper (e.g.
one that auto-injects auth headers, decodes a tagged error envelope,
or rate-limits), implement `RequestStyle` in your codegen binary. The
default impl (`DefaultRequestStyle`) is a vanilla `Http.request` that
treats the error body as opaque.

## Stability

Pre-1.0. The data model is additive-but-breaking: adding a new variant
to an enum like `ElmTypeRepr` bumps the minor version. Pin to a
specific minor range (`"0.2"`) in your `Cargo.toml` to avoid surprises.

## Testing

The workspace ships with layered test coverage:

- **Unit tests** across every crate cover the derive macro's attribute
  parser, the builder's name-map and type-override passes, the endpoint
  normalization pass, and the core data model.
- **`trybuild` compile-fail / compile-pass cases**
  (`crates/elm-client-gen-derive-tests`) pin the errors the derive macro
  emits for unsupported shapes (generics, multi-field tuple structs,
  unions, arity-4+ tuples, non-`String` map keys, externally-tagged
  enums, etc.) and verify that accepted shapes still compile.
- **`insta` snapshot tests**
  (`crates/elm-client-gen-builder/tests/snapshot.rs`) pin the full
  pretty-printed Elm output for representative record, newtype,
  bare-string enum, internally-tagged enum, untagged enum, tuple-field,
  and `mergeTaggedObject` module shapes. Formatting or pretty-printer
  regressions surface as reviewable diffs rather than passing silently.
- **CLI smoke tests** (`crates/elm-client-gen-cli/tests`) exercise both
  the library entry point and the compiled binary: dry-run banner
  output, file-per-module write layout, name filtering, empty-registry
  errors, `--help` output, and exit codes.
- **Property tests** (`proptest`) cover the path-template parser's
  round-trip invariant, `TypeOverrides::apply` idempotency across
  nested `Maybe`/`List`/`Dict`/`Tuple`/`Custom` trees, and
  `group_by_module`'s count-preservation and partition-by-module-path
  laws.

Run the full suite with `cargo test --workspace`. To regenerate
snapshots after an intentional formatting change, run
`INSTA_UPDATE=always cargo test -p elm-client-gen-builder --test snapshot`
(or use `cargo insta review` for an interactive diff). Trybuild
`.stderr` files are regenerated with `TRYBUILD=overwrite cargo test -p
elm-client-gen-derive-tests`.

## License

Dual licensed under [Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT).
