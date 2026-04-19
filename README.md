# elm-codegen

Generate Elm types, decoders, and encoders from your Rust types.

Annotate a Rust struct with `#[derive(ElmType)]`, point a small CLI (or
your own binary) at the resulting registry, and get well-formatted
`.elm` modules out the other end. Built on top of
[`elm-ast`](https://crates.io/crates/elm-ast) for both AST
construction and layout-aware pretty-printing.

```rust
use elm_codegen_core::ElmType;

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
| [`elm-codegen-core`](crates/elm-codegen-core) | Core types (`ElmTypeInfo`, `ElmTypeRepr`, `ElmType` trait) and the `inventory`-backed registry. Re-exports `#[derive(ElmType)]` behind the default `derive` feature. |
| [`elm-codegen-derive`](crates/elm-codegen-derive) | The `#[derive(ElmType)]` proc-macro. Pulled in transitively via the `derive` feature on `elm-codegen-core`; you rarely depend on it directly. |
| [`elm-codegen-builder`](crates/elm-codegen-builder) | Turns `ElmTypeInfo` values into `elm-ast` modules. Configurable via `BuildStrategy`, `TypeOverrides`, and `MaybeEncoderRef`. |
| [`elm-codegen-cli`](crates/elm-codegen-cli) | Reference binary. Walks the registry, applies overrides, writes one `.elm` file per Elm module path. |

## Quick start

```toml
[dependencies]
elm-codegen-core = "0.1"

[build-dependencies]
elm-codegen-builder = "0.1"
```

Annotate types throughout your crate with `#[derive(ElmType)]`, then
either:

1. **Use the reference CLI**: install with
   `cargo install elm-codegen-cli`, then write a thin binary in your
   workspace that adds `use my_schema_crate as _;` to force-link your
   types and re-uses the CLI.
2. **Roll your own binary**: call
   `elm_codegen_core::registered_types()`, apply your own
   `TypeOverrides` and `BuildStrategy`, then iterate
   `elm_codegen_builder::group_by_module(&types)` and write each
   module to disk.

## Linking note

Because `#[derive(ElmType)]` registers types via `inventory`'s
link-time collector, any crate that *only* contributes derived types
must be referenced from the binary that runs codegen. The idiomatic
pattern is `use my_schema_crate as _;` at the top of `main.rs`. Without
this, rustc dead-strips the rlib and you get an empty registry.

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
encoding a `Maybe a` (you'd have to write
`case x of Just v -> encodeA v; Nothing -> Encode.null` every time).
Most projects pull one in, usually
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
and adds the matching `import` line, so the generated module compiles
without any extra wiring on the Elm side.

## License

MIT
