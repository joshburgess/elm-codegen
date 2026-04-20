# elm-codegen-derive

Proc-macros for [`elm-codegen`](https://github.com/joshburgess/elm-codegen):

- `#[derive(ElmType)]` — registers a Rust type with the
  [`elm-codegen-core`](https://crates.io/crates/elm-codegen-core)
  registry so downstream tooling can emit the matching Elm type,
  decoder, and encoder.
- `#[elm_endpoint(...)]` — registers an Axum-style handler with the
  [`elm-codegen-http`](https://crates.io/crates/elm-codegen-http)
  registry so downstream tooling can emit the matching Elm request
  function.

You usually don't depend on this crate directly. `elm-codegen-core`
re-exports `#[derive(ElmType)]` behind its default `derive` feature;
`elm-codegen-http` re-exports `#[elm_endpoint(...)]` behind its
default `derive` feature.

## `#[derive(ElmType)]`

```rust
use elm_codegen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person")]
pub struct PersonApi {
    pub id: String,
    pub name: String,
}
```

### Container attributes

| Attribute | Required | Meaning |
| --- | --- | --- |
| `#[elm(module = "Foo.Bar")]` | yes | Elm module path. |
| `#[elm(name = "Person")]` | no (defaults to the Rust ident) | Elm type name. |
| `#[elm(tags = "a,b,c")]` | no | Free-form tags consumers can branch on (e.g. `"entity"`, `"readonly"`, `"upsert_input"`). |

### Field attributes

| Attribute | Meaning |
| --- | --- |
| `#[elm(skip)]` | Exclude this field from the Elm type. |
| `#[elm(type = "Money")]` | Override the inferred Elm type (see "Type overrides" below). |
| `#[elm(name = "customName")]` | Override the camelCased Elm field name. |
| `#[elm(decoder = "customDecoder")]` | Substitute a decoder expression for this field. |
| `#[elm(encoder = "customEncoder")]` | Substitute an encoder function; applied as `customEncoder value.fieldName`. |

### Variant attributes

| Attribute | Meaning |
| --- | --- |
| `#[elm(name = "Alt")]` | Override the Elm constructor name (defaults to the Rust variant ident). |

## Supported shapes

**Record structs** become Elm `type alias X = { ... }` declarations
with one field per struct field.

**Single-field tuple structs** (`struct UserId(String)`) become
transparent Elm `type alias`es over the inner type. The generated
decoder/encoder delegate to the inner type's codec.

**Enums** map to Elm custom types. Three serde representations are
supported:

| Serde representation | Attribute | Allowed variant shapes |
| --- | --- | --- |
| Internally tagged | `#[serde(tag = "type")]` | unit, newtype (1-field tuple), struct |
| Untagged | `#[serde(untagged)]` | newtype (1-field tuple), struct |
| Bare-string | *(default, unit-only enums)* | unit |

Serde's default externally-tagged representation (non-unit enums with
no serde attribute) is rejected. Pick one of the supported
representations by adding `#[serde(tag = "...")]` or
`#[serde(untagged)]`.

Multi-field tuple variants (`Variant(A, B)`) are rejected; wrap the
payload in a struct variant (`Variant { a: A, b: B }`) instead.

For internally-tagged enums whose newtype variants carry another
internally-tagged enum or struct, the generated Elm encoder flattens
the inner JSON object alongside the tag key (matching serde's wire
format). This produces a small private `mergeTaggedObject` helper in
the generated Elm module — see `elm-codegen-builder` for details.

### Rejected inputs

`#[derive(ElmType)]` refuses to produce a codec for shapes with no
canonical JSON form:

- Generic types (`struct Page<T> { ... }`) — the registry
  materializes one declaration per Rust type with no way to
  monomorphize. Define a concrete wrapper per instantiation instead.
- Unit structs (`struct Empty;`) — no JSON shape.
- Multi-field tuple structs (`struct Pair(A, B)`) — no canonical
  JSON shape; use a struct with named fields.
- Unions — JSON has no union concept.
- Tuples of arity 4+ — Elm's tuple support stops at 3.
- Maps with non-`String` keys — JSON object keys are always strings.

## Serde compatibility

Recognized serde attributes:

- `#[serde(rename = "...")]` on containers, variants, and fields.
- `#[serde(rename_all = "...")]` on containers. Conventions: `lowercase`,
  `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`,
  `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`.
- `#[serde(tag = "...")]` — internally tagged enum.
- `#[serde(untagged)]` — untagged enum.

Rename attributes affect the JSON key (what the decoder/encoder use
on the wire). The Elm field name is independently camelCased from the
Rust ident (or set explicitly via `#[elm(name = "...")]`).

## Type mapping

| Rust | Elm |
| --- | --- |
| `String` | `String` |
| `bool` | `Bool` |
| `i8..=i64`, `u8..=u64`, `usize`, `isize` | `Int` |
| `f32`, `f64` | `Float` |
| `Option<T>` | `Maybe T` (emitted as `optional` in decoder) |
| `Vec<T>` | `List T` |
| `HashMap<String, V>` / `BTreeMap<String, V>` | `Dict String V` |
| `(A, B)` / `(A, B, C)` | Elm tuple |
| `chrono::DateTime<_>` | `Time.Posix` |
| `chrono::NaiveDate` | `String` (ISO date) |
| `chrono::NaiveDateTime`, `chrono::NaiveTime` | `String` (bare ISO, no tz) |
| `uuid::Uuid` | `String` |
| `serde_json::Value` | `Json.Encode.Value` |
| `Patch<T>`, `PatchNullable<T>` | `Maybe T` (merge-patch semantics) |
| `Box<T>`, `Arc<T>`, `Rc<T>`, `Cow<_, T>` | same as `T` (transparent) |
| anything else | `ElmTypeRepr::Custom("TypeName")`, resolved via the builder's `NameMap` |

External types (`DateTime`, `NaiveDate`, `NaiveDateTime`, `NaiveTime`,
`Uuid`, `Value`) are matched on the *final* ident of the path, so both
`uuid::Uuid` and a re-exported `crate::Uuid` work. If you have a
differently-shaped type with the same name, override with
`#[elm(type = "...")]` on the field.

### Type overrides

`#[elm(type = "...")]` accepts any Elm type expression the derive
understands. Primitives are recognized by name:

- `"String"`, `"Int"`, `"Float"`, `"Bool"`, `"Time.Posix"`,
  `"Encode.Value"` / `"Decode.Value"`.
- `"Maybe T"`, `"List T"`, `"Dict String T"` (parsed recursively).
- Anything else becomes `Custom("...")` and is resolved via the
  builder's `NameMap`.

## `#[elm_endpoint(...)]`

```rust
use elm_codegen_http::elm_endpoint;

#[elm_endpoint(
    GET, "/api/v1/persons/{person_id}",
    module = "Api.Generated.Person",
    name   = "getPerson",
    tags   = "read",
)]
async fn get_person(
    Path(person_id): Path<String>,
    Query(filters):  Query<PersonFilters>,
    State(app):      State<AppState>,
) -> Result<Json<PersonApi>, HttpError> { /* ... */ }
```

### Syntax

```text
#[elm_endpoint(
    <METHOD>, "<path template>",
    module = "<Elm.Module.Path>",       // required
    name   = "<elmFunctionName>",       // optional; defaults to camelCase(fn_name)
    tags   = "<a,b,c>",                 // optional; comma-separated
)]
```

Methods: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`.

### How it works

The macro doesn't inspect extractor types beyond textually dispatching
to `<T as ElmExtractor>::elm_extractor_info()` and
`<R as ElmResponse>::elm_response_info()`. All interpretation is done
by trait impls. The original function is emitted unchanged.

See `elm-codegen-http` for the traits and the bundled Axum impls.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
