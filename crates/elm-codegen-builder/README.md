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

Per-type policy hooks. Defaults emit both decoder and encoder for
every type. Override per-tag rules by implementing the trait
yourself:

```rust
struct MyStrategy;

impl BuildStrategy for MyStrategy {
    fn should_emit_encoder(&self, info: &ElmTypeInfo) -> bool {
        !info.has_tag("readonly")
    }
}
```

### `TypeOverrides`

Codebase-wide rewrites applied to `ElmTypeRepr::Custom(_)` references.
Useful for mapping project-specific Rust types (e.g. `BigDecimal`) to
Elm primitives without per-field annotations.

Per-field `#[elm(type = "...")]` overrides take precedence — they are
applied at derive time, before the builder ever sees the
`ElmTypeInfo`.

### `MaybeEncoderRef`

Where to find your `encodeMaybe` (or equivalent) helper in your Elm
codebase. The builder emits an unqualified call to
`function_name` and adds the matching import.

```rust
let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
```

### `NameMap`

Resolves `ElmTypeRepr::Custom("PersonApi")` references to their Elm
type name and module path so the builder can emit correct imports.
`NameMap::from_types(&types)` populates it from your registered types;
`NameMap::register(...)` lets you add hand-written modules the
generator wouldn't otherwise see.

## License

MIT
