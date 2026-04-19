//! Core data types and trait for Elm code generation from Rust.
//!
//! Types annotated with `#[derive(ElmType)]` register an `ElmTypeInfo`
//! builder into a global `inventory` collector at link time. Downstream
//! tooling (e.g. `elm-codegen-builder`) iterates the registry to emit
//! Elm modules.
//!
//! # Example
//!
//! ```ignore
//! use elm_codegen_core::ElmType;
//!
//! #[derive(ElmType)]
//! #[elm(module = "Api.Person", name = "Person", tags = "entity")]
//! pub struct PersonApi {
//!     pub id: String,
//!     pub name: String,
//! }
//! ```

#[cfg(feature = "derive")]
pub use elm_codegen_derive::ElmType;

#[doc(hidden)]
pub use inventory;

/// Describes a single field on a Rust struct, post Elm-mapping.
#[derive(Clone, Debug)]
pub struct ElmFieldInfo {
    /// The JSON key (defaults to the raw Rust field name; respects
    /// `#[serde(rename)]` and `#[serde(rename_all)]`).
    pub rust_name: &'static str,
    /// The Elm field name (camelCase by default; overridable via
    /// `#[elm(name = "...")]`).
    pub elm_name: &'static str,
    /// The Elm type the field maps to.
    pub elm_type: ElmTypeRepr,
    /// Whether the field came from `Option<T>`. Emitted as `optional`
    /// rather than `required` in the generated decoder.
    pub is_optional: bool,
    /// Override decoder expression set via `#[elm(decoder = "...")]`.
    pub custom_decoder: Option<&'static str>,
}

/// Simplified Elm type tree.
///
/// Only the primitive shapes that map directly to common Rust types are
/// represented here. Anything else lands in `Custom(name)` and is left to
/// the consumer's name resolution / type override layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElmTypeRepr {
    String,
    Int,
    Float,
    Bool,
    /// `chrono::DateTime<_>`. Typically rendered as `Time.Posix` and
    /// decoded via `Iso8601.decoder`.
    Posix,
    /// `chrono::NaiveDate`. Rendered as `String` (ISO date string).
    IsoDate,
    Maybe(Box<ElmTypeRepr>),
    List(Box<ElmTypeRepr>),
    /// A named type defined elsewhere, usually another struct that also
    /// derives `ElmType`. Consumers may also use this for codebase-specific
    /// types they later remap via the override layer.
    Custom(String),
}

/// Metadata for a single Rust struct that maps to an Elm type.
///
/// `tags` is an open-ended labeling channel. Consumers attach project
/// conventions like `"entity"`, `"response"`, `"upsert_input"` and decide
/// downstream how those affect generation.
#[derive(Clone, Debug)]
pub struct ElmTypeInfo {
    /// The original Rust struct name, e.g. `"PersonApi"`.
    pub rust_name: &'static str,
    /// The Elm module path, e.g. `["Api", "Person"]`.
    pub module_path: Vec<&'static str>,
    /// The Elm type name, e.g. `"Person"`.
    pub type_name: &'static str,
    /// Project-specific tags from `#[elm(tags = "a,b,c")]`.
    pub tags: Vec<&'static str>,
    /// The fields, in declaration order.
    pub fields: Vec<ElmFieldInfo>,
}

impl ElmTypeInfo {
    /// Returns true if this type carries the given tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }
}

/// Trait implemented by `#[derive(ElmType)]`.
pub trait ElmType {
    fn elm_type_info() -> ElmTypeInfo;
}

/// Registration entry stored in the global `inventory` collector.
///
/// A function pointer is used because `inventory::submit!` requires a
/// const expression, while `ElmTypeInfo` owns allocations. Downstream
/// tooling calls `(build)()` to materialize the metadata.
pub struct ElmTypeRegistration {
    pub build: fn() -> ElmTypeInfo,
}

inventory::collect!(ElmTypeRegistration);

/// Materialize every type registered via `#[derive(ElmType)]`.
///
/// Iteration order follows link order. Call `.sort_by(...)` on the
/// result if you need deterministic output.
pub fn registered_types() -> Vec<ElmTypeInfo> {
    inventory::iter::<ElmTypeRegistration>()
        .map(|r| (r.build)())
        .collect()
}
