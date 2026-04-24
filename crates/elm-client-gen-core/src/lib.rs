//! Core data types and trait for Elm code generation from Rust.
//!
//! Types annotated with `#[derive(ElmType)]` register an `ElmTypeInfo`
//! builder into a global `inventory` collector at link time. Downstream
//! tooling (e.g. `elm-client-gen-builder`) iterates the registry to emit
//! Elm modules.
//!
//! # Example
//!
//! ```ignore
//! use elm_client_gen_core::ElmType;
//!
//! #[derive(ElmType)]
//! #[elm(module = "Api.Person", name = "Person", tags = "entity")]
//! pub struct PersonApi {
//!     pub id: String,
//!     pub name: String,
//! }
//! ```

#[cfg(feature = "derive")]
pub use elm_client_gen_derive::ElmType;

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
    /// Override encoder expression set via `#[elm(encoder = "...")]`.
    /// Applied as `customEncoder value.fieldName` inside the
    /// `Encode.object` list.
    pub custom_encoder: Option<&'static str>,
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
    /// `(A, B)` or `(A, B, C)`. Renders as an Elm tuple, encodes as a
    /// JSON array, decodes via `Decode.map2`/`Decode.map3` with
    /// `Decode.index`. Tuples larger than 3 are rejected at derive
    /// time because Elm's tuple support stops at 3 (and named records
    /// are clearer at that arity anyway).
    Tuple(Vec<ElmTypeRepr>),
    /// `HashMap<String, T>` / `BTreeMap<String, T>`. Renders as Elm
    /// `Dict String T`. Only string-keyed maps are supported (matching
    /// the JSON object encoding).
    Dict(Box<ElmTypeRepr>),
    /// Raw JSON value. Maps to Elm `Json.Encode.Value` (which doubles
    /// as a decoder via `Json.Decode.Value`). Used for `serde_json::Value`
    /// fields where the wire shape isn't statically typed.
    Value,
    /// A named type defined elsewhere, usually another struct that also
    /// derives `ElmType`. Consumers may also use this for codebase-specific
    /// types they later remap via the override layer.
    Custom(String),
}

/// Shape of an enum variant's associated data.
#[derive(Clone, Debug)]
pub enum ElmVariantPayload {
    /// `Foo` — no associated data.
    Unit,
    /// `Foo(T)` — a single positional value. Produced for untagged
    /// enums (inner value serialized directly) and for internally-tagged
    /// enums where `T` serializes to a JSON object (its fields are
    /// flattened next to the tag key).
    Newtype(ElmTypeRepr),
    /// `Foo { ... }` — named fields.
    Struct(Vec<ElmFieldInfo>),
}

impl ElmVariantPayload {
    pub fn is_unit(&self) -> bool {
        matches!(self, ElmVariantPayload::Unit)
    }

    /// Convenience accessor for callers that only handle struct
    /// variants. Returns `&[]` for `Unit` and `Newtype`.
    pub fn struct_fields(&self) -> &[ElmFieldInfo] {
        match self {
            ElmVariantPayload::Struct(fs) => fs,
            _ => &[],
        }
    }
}

/// Describes one variant of a Rust enum.
#[derive(Clone, Debug)]
pub struct ElmVariantInfo {
    /// The original Rust variant ident, e.g. `"Open"`.
    pub rust_name: &'static str,
    /// The Elm constructor name (defaults to `rust_name`; overridable
    /// via `#[elm(name = "...")]`).
    pub elm_name: &'static str,
    /// The JSON string this variant serializes to (respects
    /// `#[serde(rename)]` and `#[serde(rename_all)]`). Unused for
    /// untagged enums.
    pub json_tag: &'static str,
    /// What this variant carries on the wire.
    pub payload: ElmVariantPayload,
}

/// How a Rust enum is laid out on the wire.
#[derive(Clone, Debug)]
pub enum EnumRepresentation {
    /// All-unit enum. Each variant serializes as a bare string. The
    /// default for unit-only Rust enums without a serde tag attribute.
    BareString,
    /// `#[serde(tag = "<key>")]`. Variants serialize as objects with
    /// the tag key set to the variant name and any payload fields
    /// flattened alongside.
    InternallyTagged { tag_key: &'static str },
    /// `#[serde(untagged)]`. Each variant's inner shape is encoded
    /// directly with no discriminator. Decoding tries variants in
    /// declaration order and the first that matches wins.
    Untagged,
}

/// What shape of Elm declaration this Rust type maps to.
#[derive(Clone, Debug)]
pub enum ElmTypeKind {
    Record {
        fields: Vec<ElmFieldInfo>,
    },
    Enum {
        variants: Vec<ElmVariantInfo>,
        representation: EnumRepresentation,
    },
    /// A single-field tuple struct (`struct UserId(String);`). Renders
    /// as a transparent Elm `type alias` over the inner type, with
    /// decoder/encoder that delegate to the inner type's codec. Lets
    /// downstream code reference the named type without the per-field
    /// `#[elm(type = "...")]` ceremony.
    Newtype {
        inner: ElmTypeRepr,
    },
}

/// Metadata for a single Rust type that maps to an Elm declaration.
///
/// `tags` is an open-ended labeling channel. Consumers attach project
/// conventions like `"entity"`, `"response"`, `"upsert_input"` and decide
/// downstream how those affect generation.
#[derive(Clone, Debug)]
pub struct ElmTypeInfo {
    /// The original Rust type name, e.g. `"PersonApi"`.
    pub rust_name: &'static str,
    /// The Elm module path, e.g. `["Api", "Person"]`.
    pub module_path: Vec<&'static str>,
    /// The Elm type name, e.g. `"Person"`.
    pub type_name: &'static str,
    /// Project-specific tags from `#[elm(tags = "a,b,c")]`.
    pub tags: Vec<&'static str>,
    /// What kind of Elm declaration this maps to.
    pub kind: ElmTypeKind,
}

impl ElmTypeInfo {
    /// Returns true if this type carries the given tag.
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }

    /// Returns the record fields, or an empty slice for any other kind.
    /// Convenience for code that only handles records.
    pub fn fields(&self) -> &[ElmFieldInfo] {
        match &self.kind {
            ElmTypeKind::Record { fields } => fields,
            _ => &[],
        }
    }

    /// Returns the enum variants, or an empty slice for any other kind.
    pub fn variants(&self) -> &[ElmVariantInfo] {
        match &self.kind {
            ElmTypeKind::Enum { variants, .. } => variants,
            _ => &[],
        }
    }

    pub fn is_enum(&self) -> bool {
        matches!(self.kind, ElmTypeKind::Enum { .. })
    }

    pub fn is_newtype(&self) -> bool {
        matches!(self.kind, ElmTypeKind::Newtype { .. })
    }
}

/// Trait implemented by `#[derive(ElmType)]`.
pub trait ElmType {
    /// True iff `Self` is the unit type `()`. Default `false`. The unit
    /// blanket impl below sets it to `true`, letting wrapper traits
    /// (e.g. `ElmResponse for Json<T>`) short-circuit to an empty body
    /// without special-casing `()` at every call site.
    const IS_UNIT: bool = false;
    /// True iff this type doesn't own a standalone Elm declaration
    /// (e.g. `Vec<T>` is a wrapper that only makes sense composed at
    /// use sites). When `true`, `elm_type_info()` is a sentinel and
    /// `elm_type_repr()` is the only safe reference.
    const IS_WRAPPER: bool = false;
    fn elm_type_info() -> ElmTypeInfo;
    /// The Elm-level type representation at a reference site. Default
    /// is `Custom(rust_name)` for named types; wrapper impls like
    /// `Vec<T>` override this to compose the inner repr (e.g.
    /// `List(T::elm_type_repr())`).
    fn elm_type_repr() -> ElmTypeRepr {
        ElmTypeRepr::Custom(Self::elm_type_info().rust_name.to_string())
    }
}

/// `Json<()>` is the conventional Axum way to say "200 OK with no
/// payload" while keeping the success arm of a `Result<Json<_>, _>`
/// well-typed. Implementing `ElmType` for `()` here lets that flow
/// pick up the standard `Json<T: ElmType>` response impl without a
/// conflicting concrete impl on `Json<()>`. The returned `ElmTypeInfo`
/// is a sentinel: consumers should branch on `IS_UNIT` instead of
/// rendering the info.
impl ElmType for () {
    const IS_UNIT: bool = true;
    fn elm_type_info() -> ElmTypeInfo {
        ElmTypeInfo {
            rust_name: "()",
            module_path: vec![],
            type_name: "()",
            tags: vec![],
            kind: ElmTypeKind::Record { fields: vec![] },
        }
    }
}

/// `Vec<T>` is a wrapper: it doesn't have its own Elm declaration, it
/// just composes into `List T` wherever it's referenced. The blanket
/// here lets top-level response/body positions accept `Vec<T>` (e.g.
/// `Json<Vec<PersonApi>>`) without forcing callers to wrap it in a
/// named struct. Consumers must use `elm_type_repr()` to reference
/// this type; `elm_type_info()` panics.
impl<T: ElmType> ElmType for Vec<T> {
    const IS_WRAPPER: bool = true;
    fn elm_type_info() -> ElmTypeInfo {
        panic!(
            "ElmType::elm_type_info() is not meaningful for Vec<T>; \
             use ElmType::elm_type_repr() for wrapper types"
        );
    }
    fn elm_type_repr() -> ElmTypeRepr {
        ElmTypeRepr::List(Box::new(T::elm_type_repr()))
    }
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
