# elm-codegen-builder

Build [`elm-ast`](https://crates.io/crates/elm-ast) modules (records,
decoders, encoders, imports) from `elm-codegen-core` type info, then
render them with `elm_ast::pretty_print` to emit `elm-format`-style
source.

## Usage

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

## What you can configure

### `BuildStrategy`

Per-type policy hooks. `DefaultStrategy` emits both a decoder and an
encoder for every type, which isn't always what you want. Read-only
response types don't need encoders, and query filter types don't need
decoders. Implement the trait to skip declarations based on the type
name, module path, or the `tags` set with `#[elm(tags = "...")]`:

```rust
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
Elm primitives without per-field annotations.

Per-field `#[elm(type = "...")]` overrides take precedence, since
they are applied at derive time, before the builder ever sees the
`ElmTypeInfo`.

### `MaybeEncoderRef`

Elm's `Json.Encode` module doesn't ship a built-in helper for encoding
a `Maybe a`. Most projects pull one in, usually
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
`NameMap::from_types(&types)` populates it from your registered types;
`NameMap::register(...)` lets you add hand-written modules the
generator wouldn't otherwise see.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
