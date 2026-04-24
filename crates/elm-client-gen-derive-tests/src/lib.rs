//! Test-only crate that hosts the trybuild harness for
//! `elm-client-gen-derive`. Kept separate so the derive's test build
//! doesn't try to depend on `elm-client-gen-core` with the `derive`
//! feature active (which would cycle through the derive crate itself).
