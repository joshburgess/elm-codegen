# elm-codegen-http

HTTP endpoint metadata for [`elm-codegen`](https://github.com/joshburgess/elm-codegen).

`#[derive(ElmType)]` (from `elm-codegen-core`) describes Rust types
that should appear as Elm types. This crate adds the matching layer
for HTTP endpoints: a `#[elm_endpoint]` attribute macro on handlers
plus an `ElmExtractor` trait that teaches the codegen how each
extractor (`Path<T>`, `Query<T>`, `Json<T>`, custom wrappers) maps to
an Elm function parameter.

Pair with `elm-codegen-builder`'s `RequestStyle` trait to render the
collected endpoints as Elm request functions in whatever shape your
project uses.

## Status

Pre-1.0. The data model and trait surface are still moving.

## License

Dual licensed under [Apache 2.0](../../LICENSE-APACHE) or [MIT](../../LICENSE-MIT).
