# elm-codegen-core

Core types and trait for generating Elm code from Rust types. Part of
the [`elm-codegen`](https://github.com/joshburgess/elm-codegen)
workspace.

This crate defines the data model that every other layer operates on:
an `ElmType` trait, a link-time registry populated by
`#[derive(ElmType)]`, and the `ElmTypeInfo` / `ElmTypeRepr` structures
that describe one Rust type's mapping to Elm.

## Usage

```rust
use elm_codegen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person", tags = "entity")]
pub struct PersonApi {
    pub id: String,
    pub name: String,
    pub nickname: Option<String>,
}

fn main() {
    for info in elm_codegen_core::registered_types() {
        println!("{}.{}", info.module_path.join("."), info.type_name);
    }
}
```

## The `ElmType` trait

```rust
pub trait ElmType {
    const IS_UNIT: bool = false;
    const IS_WRAPPER: bool = false;
    fn elm_type_info() -> ElmTypeInfo;
    fn elm_type_repr() -> ElmTypeRepr { /* default: Custom(rust_name) */ }
}
```

- **`elm_type_info()`** returns the full declaration-level metadata
  (module path, type name, fields or variants, tags).
- **`elm_type_repr()`** returns the reference-level shape. Default is
  `Custom(rust_name)`. Wrapper impls override it: `Vec<T>` returns
  `List(Box::new(T::elm_type_repr()))` so `Json<Vec<PersonApi>>` can
  compose into `List Person` without a named wrapper struct.
- **`IS_UNIT`** is `true` only for `()`. Used by response-type wrappers
  (e.g. `ElmResponse for Json<T>`) to short-circuit to an empty-body
  response without a conflicting concrete impl on `Json<()>`.
- **`IS_WRAPPER`** is `true` for types like `Vec<T>` that don't have
  their own Elm declaration. When set, `elm_type_info()` is a sentinel
  and consumers must use `elm_type_repr()` instead.

Built-in blanket impls:

- `impl ElmType for ()` — unit, sentinel `ElmTypeInfo`.
- `impl<T: ElmType> ElmType for Vec<T>` — wrapper, composes `List T`.

## Data model

### `ElmTypeInfo`

```rust
pub struct ElmTypeInfo {
    pub rust_name:   &'static str,
    pub module_path: Vec<&'static str>,
    pub type_name:   &'static str,
    pub tags:        Vec<&'static str>,
    pub kind:        ElmTypeKind,
}
```

Convenience accessors: `has_tag`, `fields`, `variants`, `is_enum`,
`is_newtype`.

### `ElmTypeKind`

The three shapes a Rust type can map to on the Elm side:

| Kind | Rust shape | Elm shape |
| --- | --- | --- |
| `Record { fields }` | Struct with named fields | `type alias X = { ... }` |
| `Enum { variants, representation }` | `enum` | `type X = ... \| ...` |
| `Newtype { inner }` | Single-field tuple struct | `type alias X = <inner>` |

### `ElmTypeRepr`

The simplified Elm type tree used everywhere a type is *referenced*
(inside a field, as a `List`/`Maybe` payload, as a response body):

| Variant | Meaning |
| --- | --- |
| `String`, `Int`, `Float`, `Bool` | Elm primitives |
| `Posix` | `Time.Posix` (used for `chrono::DateTime<_>`) |
| `IsoDate` | ISO date string (used for `chrono::NaiveDate`) |
| `Maybe(Box<ElmTypeRepr>)` | `Maybe T`; decoded with `optional` |
| `List(Box<ElmTypeRepr>)` | `List T` |
| `Tuple(Vec<ElmTypeRepr>)` | Elm 2- or 3-tuple (JSON array) |
| `Dict(Box<ElmTypeRepr>)` | `Dict String T` (string-keyed only) |
| `Value` | `Json.Encode.Value` (raw JSON blob) |
| `Custom(String)` | A named type; resolved via the builder's `NameMap` |

### `EnumRepresentation`

```rust
pub enum EnumRepresentation {
    BareString,
    InternallyTagged { tag_key: &'static str },
    Untagged,
}
```

- **`BareString`** — unit-only enum. Each variant serializes as a
  plain string.
- **`InternallyTagged { tag_key }`** — `#[serde(tag = "...")]`.
  Variants serialize as objects with the tag key set to the variant
  name; unit variants produce `{ "<tag>": "..." }`; struct and
  newtype payloads have their fields flattened alongside the tag.
- **`Untagged`** — `#[serde(untagged)]`. Each variant's inner shape
  is encoded directly with no discriminator. Decoding tries variants
  in declaration order.

### `ElmVariantPayload`

```rust
pub enum ElmVariantPayload {
    Unit,                       // Foo
    Newtype(ElmTypeRepr),       // Foo(T)
    Struct(Vec<ElmFieldInfo>),  // Foo { ... }
}
```

Produced for both untagged and internally-tagged enums. Multi-field
tuple variants (`Foo(A, B)`) are rejected at derive time.

### `ElmFieldInfo`

One field on a record or struct variant:

```rust
pub struct ElmFieldInfo {
    pub rust_name:      &'static str,  // JSON key (respects serde rename)
    pub elm_name:       &'static str,  // Elm field name (camelCase by default)
    pub elm_type:       ElmTypeRepr,
    pub is_optional:    bool,          // Option<T> => emitted as `optional`
    pub custom_decoder: Option<&'static str>,
    pub custom_encoder: Option<&'static str>,
}
```

## The registry

```rust
pub struct ElmTypeRegistration {
    pub build: fn() -> ElmTypeInfo,
}

pub fn registered_types() -> Vec<ElmTypeInfo>;
```

Each `#[derive(ElmType)]` expands to an `inventory::submit!` that
registers a builder function. `registered_types()` materializes every
registered entry. Iteration order follows link order; sort the result
if you need deterministic output.

## Features

- **`derive`** *(default)*: re-exports `#[derive(ElmType)]` from
  `elm-codegen-derive`. Disable (`default-features = false`) if you
  want to implement `ElmType` by hand without pulling in the
  proc-macro crate.

## Linking note

`#[derive(ElmType)]` emits a static initializer that runs at link
time. If a crate's only contribution to your binary is derived types,
you must reference it with `use my_crate as _;` to keep rustc from
dead-stripping the rlib. Without this, `registered_types()` returns
empty.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
