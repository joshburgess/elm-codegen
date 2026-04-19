# elm-codegen-core

Core types and trait for generating Elm code from Rust types. Part of
the [`elm-codegen`](https://github.com/joshburgess/elm-codegen)
workspace.

This crate defines:

- `ElmTypeInfo` / `ElmFieldInfo` / `ElmTypeRepr`: the data the rest
  of the pipeline operates on.
- The `ElmType` trait, implemented by `#[derive(ElmType)]`.
- `ElmTypeRegistration` and `registered_types()`: an
  [`inventory`](https://crates.io/crates/inventory)-backed registry
  populated automatically by every `#[derive(ElmType)]`.

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

## Features

- `derive` (default): re-exports `#[derive(ElmType)]` from
  `elm-codegen-derive`. Disable if you want to implement `ElmType`
  by hand.

## Linking

`#[derive(ElmType)]` emits a static initializer that runs at link
time. If a crate's only contribution to your binary is derived types,
you must reference it with `use my_crate as _;` to keep its rlib from
being dead-stripped.

## License

MIT
