# elm-codegen-cli

Reference CLI for [`elm-codegen`](https://github.com/joshburgess/elm-codegen).
Walks every type registered via `#[derive(ElmType)]`, groups by Elm
module path, and writes one `.elm` file per module to a directory.

## Install

```sh
cargo install elm-codegen-cli
```

This installs the `elm-codegen` binary.

## Usage

```sh
elm-codegen --output ./src
elm-codegen --output ./src --types Person Order   # filter to specific types
elm-codegen --output ./src --dry-run              # print to stdout instead
```

## Linking your schema crate

If your `#[derive(ElmType)]` types live in a separate crate that the
CLI binary doesn't otherwise reference, rustc will dead-strip them and
the registry will be empty. The reference CLI is intentionally
project-agnostic and *doesn't* import any user crate.

The standard fix is to roll your own thin binary in your workspace:

```rust
// my-codegen/src/main.rs
use my_schema_crate as _;            // force-link the rlib

fn main() -> anyhow::Result<()> {
    elm_codegen_cli::run()           // (or copy the CLI body inline)
}
```

Or copy `crates/elm-codegen-cli/src/main.rs` from this repo and add
your `use` statement at the top. The CLI's entry-point is small on
purpose so it's easy to fork.

## Customization

For project-specific type overrides (e.g. `BigDecimal -> String`),
custom `BuildStrategy` rules, or a different `encodeMaybe` location,
write your own binary that calls the [`elm-codegen-builder`] crate
directly. The reference CLI uses the defaults.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).

[`elm-codegen-builder`]: https://crates.io/crates/elm-codegen-builder
