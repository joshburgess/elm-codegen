# elm-codegen-derive

The `#[derive(ElmType)]` proc-macro for
[`elm-codegen-core`](https://crates.io/crates/elm-codegen-core).

You usually don't depend on this crate directly. Instead, depend on
`elm-codegen-core` with the default `derive` feature enabled and use
the re-exported macro:

```rust
use elm_codegen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person")]
pub struct PersonApi {
    pub id: String,
    pub name: String,
}
```

## Container attributes

| Attribute | Required | Meaning |
| --- | --- | --- |
| `#[elm(module = "Foo.Bar")]` | yes | Elm module path. |
| `#[elm(name = "Person")]` | yes | Elm type name. |
| `#[elm(tags = "a,b,c")]` | no | Free-form tags consumers can branch on (e.g. `"entity"`, `"readonly"`). |

## Field attributes

| Attribute | Meaning |
| --- | --- |
| `#[elm(skip)]` | Exclude this field from the Elm type. |
| `#[elm(type = "Money")]` | Override the inferred Elm type. |
| `#[elm(name = "customName")]` | Override the camelCased Elm field name. |
| `#[elm(decoder = "customDecoder")]` | Substitute a decoder expression. |

## Serde compatibility

`#[serde(rename = "...")]` and `#[serde(rename_all = "...")]` are
honored when computing the JSON key (the string used by the generated
decoder/encoder). The Elm field name is independently camelCased.

## Type mapping

| Rust | Elm |
| --- | --- |
| `String` | `String` |
| `bool` | `Bool` |
| `i8..i64`, `u8..u64`, `usize`, `isize` | `Int` |
| `f32`, `f64` | `Float` |
| `chrono::DateTime<_>` (any path ending in `DateTime`) | `Time.Posix` |
| `chrono::NaiveDate` (any path ending in `NaiveDate`) | `String` (ISO date) |
| `Option<T>` | `Maybe T` (and emitted as `optional` in the decoder) |
| `Vec<T>` | `List T` |
| anything else | `ElmTypeRepr::Custom("TypeName")`, resolvable via the builder's `NameMap` |

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
