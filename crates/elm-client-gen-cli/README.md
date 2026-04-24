# elm-client-gen-cli

Reference CLI for [`elm-client-gen`](https://github.com/joshburgess/elm-client-gen).
Walks every type registered via `#[derive(ElmType)]`, groups by Elm
module path, and writes one `.elm` file per module to a directory.

This is a thin wrapper around `elm-client-gen-builder` with the default
configuration. Most real projects outgrow it quickly and ship their
own codegen binary (see below).

## Install

```sh
cargo install elm-client-gen-cli
```

This installs the `elm-client-gen` binary.

## Usage

```sh
elm-client-gen --output ./src                        # write every registered type to ./src
elm-client-gen --output ./src --types Person Order   # filter to specific type names
elm-client-gen --output ./src --dry-run              # print modules to stdout instead of writing
```

Options:

| Flag | Meaning |
| --- | --- |
| `-o, --output <DIR>` | Output directory for generated `.elm` files. Required. |
| `-t, --types <NAMES>...` | Filter to specific Elm type names. Omit for all registered types. |
| `--dry-run` | Print to stdout instead of writing files. |
| `-h, --help` | Print help. |

The CLI uses the default `BuildStrategy` (emit decoder + encoder for
every type), the default `MaybeEncoderRef`
(`Json.Encode.Extra.maybe`), an empty `TypeOverrides`, and no HTTP
endpoint codegen.

## Linking your schema crate

Because `#[derive(ElmType)]` registers types via `inventory`'s
link-time collector, any crate that *only* contributes derived types
must be referenced from the binary that runs codegen. The reference
CLI is intentionally project-agnostic and *doesn't* import any user
crate.

If your schema lives in the same crate as your codegen entry point
this isn't an issue, and you can use the published `elm-client-gen`
binary directly. Otherwise, rustc will dead-strip the schema rlib and
the registry will be empty.

The standard fix is to fork: copy the body of
`crates/elm-client-gen-cli/src/main.rs` from this repo into your own
binary and add a `use my_schema_crate as _;` at the top. The CLI's
entry point is small on purpose so this is cheap.

## Customization

The reference CLI has no flags for:

- project-specific type overrides (e.g. `BigDecimal` -> `String`),
- custom `BuildStrategy` rules (e.g. skip encoders for read-only
  types tagged `"response"`),
- custom `encodeMaybe` location,
- emitting Elm request functions from `#[elm_endpoint(...)]`
  handlers,
- running a drift check against a committed tree.

For any of those, write your own binary that calls
[`elm-client-gen-builder`] directly. See the workspace
[README](../../README.md#2-run-codegen) for a minimal template.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).

[`elm-client-gen-builder`]: https://crates.io/crates/elm-client-gen-builder
