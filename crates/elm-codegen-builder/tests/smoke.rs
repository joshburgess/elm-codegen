//! End-to-end smoke test: derive a struct, run the builder, render with
//! `elm_ast::pretty_print`, and assert the output contains the expected
//! Elm declarations.

use elm_codegen_builder::{
    build_merged_module, group_by_module, DefaultStrategy, MaybeEncoderRef, NameMap,
};
use elm_codegen_core::{registered_types, ElmType, ElmTypeInfo};

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person", tags = "entity")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PersonApi {
    pub id: String,
    pub full_name: String,
    pub age: i32,
    pub nickname: Option<String>,
    pub tags: Vec<String>,
}

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "PersonFilter", tags = "filter")]
#[allow(dead_code)]
pub struct PersonFilterApi {
    #[elm(skip)]
    pub internal: String,
    pub name_contains: Option<String>,
    #[elm(name = "minAge")]
    pub min_age_years: Option<i32>,
}

#[test]
fn derive_emits_type_info_with_field_metadata() {
    let info = PersonApi::elm_type_info();

    assert_eq!(info.rust_name, "PersonApi");
    assert_eq!(info.module_path, vec!["Api", "Person"]);
    assert_eq!(info.type_name, "Person");
    assert_eq!(info.tags, vec!["entity"]);
    assert!(info.has_tag("entity"));

    assert_eq!(info.fields().len(), 5);
    let full_name = info.fields().get(1).expect("fullName field");
    assert_eq!(full_name.rust_name, "fullName");
    assert_eq!(full_name.elm_name, "fullName");

    let nickname = info.fields().get(3).expect("nickname field");
    assert_eq!(nickname.elm_name, "nickname");
    assert!(nickname.is_optional);
}

#[test]
fn skip_and_rename_attributes_are_honored() {
    let info = PersonFilterApi::elm_type_info();
    let names: Vec<&str> = info.fields().iter().map(|f| f.elm_name).collect();

    assert!(!names.contains(&"internal"));
    assert!(names.contains(&"nameContains"));
    assert!(names.contains(&"minAge"));
}

#[test]
fn registered_types_picks_up_derived_structs() {
    let names: Vec<&str> = registered_types().iter().map(|t| t.rust_name).collect();
    assert!(names.contains(&"PersonApi"));
    assert!(names.contains(&"PersonFilterApi"));
}

#[derive(ElmType)]
#[elm(module = "Api.Treasurer", name = "TreasurerInvoiceState")]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum TreasurerInvoiceStateApi {
    Open,
    Finalized,
    Approved,
    Collected,
    Paid,
    Closed,
    Deleted,
}

#[test]
fn enum_derive_emits_variant_metadata() {
    let info = TreasurerInvoiceStateApi::elm_type_info();

    assert_eq!(info.rust_name, "TreasurerInvoiceStateApi");
    assert_eq!(info.type_name, "TreasurerInvoiceState");
    assert!(info.is_enum());

    let variants = info.variants();
    assert_eq!(variants.len(), 7);

    let by_rust: std::collections::HashMap<_, _> = variants
        .iter()
        .map(|v| (v.rust_name, (v.elm_name, v.json_tag)))
        .collect();

    // Default Elm name = Rust ident; default json_tag = serde rename_all applied.
    assert_eq!(
        by_rust.get("Open").copied().expect("Open variant"),
        ("Open", "open"),
    );
    assert_eq!(
        by_rust
            .get("Finalized")
            .copied()
            .expect("Finalized variant"),
        ("Finalized", "finalized"),
    );
    assert_eq!(
        by_rust.get("Deleted").copied().expect("Deleted variant"),
        ("Deleted", "deleted"),
    );
}

#[test]
fn enum_module_renders_type_decoder_and_encoder() {
    let types: Vec<ElmTypeInfo> = vec![TreasurerInvoiceStateApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type declaration uses `type` (not `type alias`) and lists constructors.
    assert!(
        rendered.contains("type TreasurerInvoiceState"),
        "missing custom type declaration:\n{rendered}",
    );
    assert!(
        rendered.contains("Open"),
        "missing Open constructor:\n{rendered}"
    );
    assert!(
        rendered.contains("Deleted"),
        "missing Deleted constructor:\n{rendered}"
    );

    // Exposing list opens constructors so callers can pattern-match.
    assert!(
        rendered.contains("TreasurerInvoiceState(..)"),
        "expected TreasurerInvoiceState(..) in exposing:\n{rendered}",
    );

    // Decoder dispatches on the json string tags (snake_case).
    assert!(
        rendered.contains("treasurerInvoiceStateDecoder"),
        "missing decoder:\n{rendered}",
    );
    assert!(
        rendered.contains(r#""open""#),
        "missing snake_case json tag:\n{rendered}"
    );
    assert!(
        rendered.contains(r#""finalized""#),
        "missing snake_case json tag:\n{rendered}"
    );
    assert!(
        rendered.contains("Decode.string"),
        "decoder should start from Decode.string:\n{rendered}"
    );
    assert!(
        rendered.contains("Decode.andThen"),
        "decoder should use andThen:\n{rendered}"
    );
    assert!(
        rendered.contains("Decode.fail"),
        "decoder should fail on unknown tag:\n{rendered}"
    );

    // Encoder uses `case` and Encode.string with the same tags.
    assert!(
        rendered.contains("encodeTreasurerInvoiceState"),
        "missing encoder:\n{rendered}"
    );
    assert!(
        rendered.contains("Encode.string"),
        "encoder should call Encode.string:\n{rendered}",
    );
}

// The derive macro recognises types by the last path segment ident
// (`DateTime` -> `Posix`), so a stub type with the right name is
// enough to exercise the codegen without pulling in chrono.
#[allow(dead_code)]
pub struct DateTime<T>(std::marker::PhantomData<T>);
#[allow(dead_code)]
pub struct Utc;

#[derive(ElmType)]
#[elm(module = "Api.UserEmail", name = "UserEmailAddress")]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum UserEmailAddressApi {
    Confirmed {
        confirmed_at: Option<DateTime<Utc>>,
        email_address: String,
    },
    Unconfirmed {
        email_address: Option<String>,
    },
    Locked,
}

#[test]
fn tagged_enum_metadata_carries_payload_and_tag_key() {
    let info = UserEmailAddressApi::elm_type_info();
    assert!(info.is_enum());
    let variants = info.variants();
    assert_eq!(variants.len(), 3);

    let confirmed = variants
        .iter()
        .find(|v| v.rust_name == "Confirmed")
        .expect("Confirmed variant");
    let confirmed_fields = confirmed.payload.struct_fields();
    assert_eq!(confirmed_fields.len(), 2);
    let confirmed_at = confirmed_fields.first().expect("confirmed_at field");
    assert!(confirmed_at.is_optional);
    assert_eq!(confirmed_at.elm_name, "confirmedAt");

    let locked = variants
        .iter()
        .find(|v| v.rust_name == "Locked")
        .expect("Locked variant");
    assert!(locked.payload.is_unit());
}

#[test]
fn tagged_enum_renders_struct_variants_with_anonymous_records() {
    let types: Vec<ElmTypeInfo> = vec![UserEmailAddressApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    assert!(
        rendered.contains("type UserEmailAddress"),
        "missing custom type:\n{rendered}",
    );
    // Struct variants render with an anonymous record arg.
    assert!(
        rendered.contains("Confirmed {"),
        "expected `Confirmed {{` in rendered output:\n{rendered}",
    );
    assert!(
        rendered.contains("Unconfirmed {"),
        "expected `Unconfirmed {{` in rendered output:\n{rendered}",
    );
    // Unit variants stay bare.
    assert!(
        rendered.contains("| Locked"),
        "expected `| Locked` in rendered output:\n{rendered}",
    );

    // Decoder reads the discriminator field and dispatches.
    assert!(
        rendered.contains(r#"Decode.field "type""#),
        "decoder should read the tag field:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.andThen"),
        "decoder should use andThen:\n{rendered}",
    );
    assert!(
        rendered.contains(r#""Confirmed""#),
        "decoder should branch on Confirmed:\n{rendered}",
    );
    assert!(
        rendered.contains(r#"required "email_address""#),
        "Confirmed branch should require email_address:\n{rendered}",
    );
    assert!(
        rendered.contains(r#"optional "confirmed_at""#),
        "Confirmed branch should make confirmed_at optional:\n{rendered}",
    );

    // Encoder pattern-matches on the constructor with a payload binding
    // and emits the tag alongside the payload fields.
    assert!(
        rendered.contains("Confirmed payload"),
        "encoder should bind payload in Confirmed branch:\n{rendered}",
    );
    assert!(
        rendered.contains("payload.emailAddress"),
        "encoder should access payload.emailAddress:\n{rendered}",
    );
    assert!(
        rendered.contains(r#"( "type", Encode.string "Confirmed" )"#)
            || rendered.contains(r#"("type", Encode.string "Confirmed")"#),
        "encoder should emit the tag pair:\n{rendered}",
    );

    // Time/Iso8601 imports kick in because of the Posix payload field.
    assert!(
        rendered.contains("import Time"),
        "expected Time import:\n{rendered}"
    );
    assert!(
        rendered.contains("import Iso8601"),
        "expected Iso8601 import:\n{rendered}"
    );
}

// ── Untagged enum coverage ──────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Api.Search", name = "SearchHit")]
#[serde(untagged)]
#[allow(dead_code)]
pub enum SearchHitApi {
    /// Newtype variant: encodes as a bare string on the wire.
    Term(String),
    /// Struct variant: encodes as a JSON object with named fields.
    Range { from: i32, to: i32 },
    /// Unit variant: encodes as JSON `null`.
    Empty,
}

#[test]
fn untagged_enum_metadata_carries_variant_payloads() {
    use elm_codegen_core::{ElmTypeKind, ElmTypeRepr, ElmVariantPayload, EnumRepresentation};

    let info = SearchHitApi::elm_type_info();
    let ElmTypeKind::Enum {
        variants,
        representation,
    } = &info.kind
    else {
        panic!("expected enum kind");
    };
    assert!(matches!(representation, EnumRepresentation::Untagged));
    assert_eq!(variants.len(), 3);

    let term = variants
        .iter()
        .find(|v| v.rust_name == "Term")
        .expect("Term variant");
    match &term.payload {
        ElmVariantPayload::Newtype(ElmTypeRepr::String) => {}
        other => panic!("expected Newtype(String) for Term, got {other:?}"),
    }

    let range = variants
        .iter()
        .find(|v| v.rust_name == "Range")
        .expect("Range variant");
    let range_fields = range.payload.struct_fields();
    assert_eq!(range_fields.len(), 2);
    assert_eq!(
        range_fields.first().expect("first range field").elm_name,
        "from"
    );

    let empty = variants
        .iter()
        .find(|v| v.rust_name == "Empty")
        .expect("Empty variant");
    assert!(empty.payload.is_unit());
}

#[test]
fn untagged_enum_renders_oneof_decoder_and_tag_free_encoder() {
    let types: Vec<ElmTypeInfo> = vec![SearchHitApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type renders newtype variant with positional arg, struct variant
    // with anonymous record, unit variant bare.
    assert!(
        rendered.contains("Term String"),
        "missing Term String constructor:\n{rendered}"
    );
    assert!(
        rendered.contains("Range") && rendered.contains("{ from : Int"),
        "missing Range struct constructor with anonymous record:\n{rendered}",
    );
    assert!(
        rendered.contains("| Empty"),
        "missing Empty unit constructor:\n{rendered}"
    );

    // Decoder uses Decode.oneOf, NOT Decode.field "type" / andThen.
    assert!(
        rendered.contains("Decode.oneOf"),
        "untagged decoder must use Decode.oneOf:\n{rendered}",
    );
    assert!(
        !rendered.contains(r#"Decode.field "type""#),
        "untagged decoder must not read a tag field:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.map Term"),
        "missing Decode.map Term:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.null Empty"),
        "missing Decode.null Empty for unit variant:\n{rendered}",
    );

    // Encoder is tag-free: each branch emits the inner shape directly.
    assert!(
        rendered.contains("encodeSearchHit"),
        "missing encoder:\n{rendered}",
    );
    assert!(
        !rendered.contains(r#"( "type", Encode.string"#),
        "untagged encoder must not emit a tag pair:\n{rendered}",
    );
    assert!(
        rendered.contains("Term inner"),
        "encoder should bind newtype payload as `inner`:\n{rendered}",
    );
    assert!(
        rendered.contains("Encode.string inner"),
        "newtype Term should encode inner directly:\n{rendered}",
    );
    assert!(
        rendered.contains("Empty ->\n            Encode.null")
            || rendered.contains("Empty ->\n        Encode.null")
            || rendered.contains("Empty ->") && rendered.contains("Encode.null"),
        "Empty branch should encode as null:\n{rendered}",
    );
}

// ── Newtype struct + wrapper passthrough + custom encoder ───────────

#[derive(ElmType)]
#[elm(module = "Api.Ids", name = "UserId")]
#[allow(dead_code)]
pub struct UserIdApi(String);

#[derive(ElmType)]
#[elm(module = "Api.Wrapped", name = "Wrapped")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WrappedApi {
    pub boxed_name: Box<String>,
    pub arc_count: std::sync::Arc<i32>,
    #[elm(encoder = "Money.encode")]
    pub price: i32,
}

#[test]
fn newtype_struct_emits_type_alias() {
    use elm_codegen_core::{ElmTypeKind, ElmTypeRepr};

    let info = UserIdApi::elm_type_info();
    match &info.kind {
        ElmTypeKind::Newtype {
            inner: ElmTypeRepr::String,
        } => {}
        other => panic!("expected Newtype(String), got {other:?}"),
    }
    assert!(info.is_newtype());
}

#[test]
fn newtype_struct_renders_alias_and_delegating_codec() {
    let types: Vec<ElmTypeInfo> = vec![UserIdApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Renders as a type alias, not a custom type with a constructor.
    assert!(
        rendered.contains("type alias UserId"),
        "expected `type alias UserId ...`:\n{rendered}",
    );
    let alias_idx = rendered
        .find("type alias UserId")
        .expect("rendered output should contain the type alias");
    let after = &rendered[alias_idx..];
    assert!(
        after.contains("String"),
        "alias should resolve to String:\n{rendered}",
    );

    // Decoder/encoder delegate to String's codec via type-alias transparency.
    assert!(
        rendered.contains("userIdDecoder"),
        "missing decoder:\n{rendered}"
    );
    assert!(
        rendered.contains("Decode.string"),
        "decoder should delegate to Decode.string:\n{rendered}"
    );
    assert!(
        rendered.contains("encodeUserId"),
        "missing encoder:\n{rendered}"
    );
    assert!(
        rendered.contains("Encode.string value"),
        "encoder should delegate to Encode.string:\n{rendered}"
    );
}

#[test]
fn box_and_arc_passthrough_to_inner_type() {
    let info = WrappedApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();
    use elm_codegen_core::ElmTypeRepr;
    assert!(matches!(
        by_name.get("boxedName").expect("boxedName field"),
        ElmTypeRepr::String
    ));
    assert!(matches!(
        by_name.get("arcCount").expect("arcCount field"),
        ElmTypeRepr::Int
    ));
}

#[test]
fn custom_encoder_attribute_substitutes_field_encoder() {
    let info = WrappedApi::elm_type_info();
    let price = info
        .fields()
        .iter()
        .find(|f| f.elm_name == "price")
        .expect("price field");
    assert_eq!(price.custom_encoder, Some("Money.encode"));

    let types: Vec<ElmTypeInfo> = vec![WrappedApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Custom encoder substituted: should call Money.encode, NOT Encode.int.
    assert!(
        rendered.contains("Money.encode value.price"),
        "encoder should delegate to Money.encode:\n{rendered}",
    );
    assert!(
        !rendered.contains("Encode.int value.price"),
        "encoder should not use the type-driven encoder for `price`:\n{rendered}",
    );
}

// ── Tuple support ───────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Api.Geo", name = "Coordinates")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct CoordinatesApi {
    pub lat_lon: (f64, f64),
    pub bounding_box: (f64, f64, f64),
}

#[test]
fn tuple_field_repr_carries_inner_types() {
    use elm_codegen_core::ElmTypeRepr;

    let info = CoordinatesApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();

    match by_name.get("latLon").expect("latLon field") {
        ElmTypeRepr::Tuple(elems) => {
            assert_eq!(elems.len(), 2);
            assert!(matches!(
                elems.first().expect("first tuple element"),
                ElmTypeRepr::Float
            ));
            assert!(matches!(
                elems.get(1).expect("second tuple element"),
                ElmTypeRepr::Float
            ));
        }
        other => panic!("expected Tuple for latLon, got {other:?}"),
    }
    match by_name.get("boundingBox").expect("boundingBox field") {
        ElmTypeRepr::Tuple(elems) => assert_eq!(elems.len(), 3),
        other => panic!("expected Tuple for boundingBox, got {other:?}"),
    }
}

#[test]
fn tuple_renders_type_decoder_and_encoder() {
    let types: Vec<ElmTypeInfo> = vec![CoordinatesApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type annotation renders as a tuple.
    assert!(
        rendered.contains("( Float, Float )") || rendered.contains("(Float, Float)"),
        "expected 2-tuple type annotation:\n{rendered}",
    );

    // Decoder uses Decode.map2/map3 with positional Decode.index.
    assert!(
        rendered.contains("Decode.map2"),
        "decoder should use Decode.map2 for 2-tuple:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.map3"),
        "decoder should use Decode.map3 for 3-tuple:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.index 0"),
        "decoder should index 0:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.index 1"),
        "decoder should index 1:\n{rendered}",
    );
    assert!(
        rendered.contains("Decode.index 2"),
        "decoder should index 2 for 3-tuple:\n{rendered}",
    );

    // Encoder destructures tuple via lambda pattern and emits Encode.list with identity.
    assert!(
        rendered.contains("Encode.list"),
        "encoder should use Encode.list for tuple:\n{rendered}",
    );
    assert!(
        rendered.contains("identity"),
        "encoder should pass identity as the per-element encoder:\n{rendered}",
    );
    assert!(
        rendered.contains("\\( a, b )") || rendered.contains("\\(a, b)"),
        "encoder should destructure 2-tuple in lambda:\n{rendered}",
    );
}

// ── Chrono naive types ──────────────────────────────────────────────

#[allow(dead_code)]
pub struct NaiveDate;
#[allow(dead_code)]
pub struct NaiveDateTime;
#[allow(dead_code)]
pub struct NaiveTime;

#[derive(ElmType)]
#[elm(module = "Api.When", name = "When")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WhenApi {
    pub day: NaiveDate,
    pub stamp: NaiveDateTime,
    pub clock: NaiveTime,
}

#[test]
fn chrono_naive_types_map_to_iso_date_and_string() {
    use elm_codegen_core::ElmTypeRepr;
    let info = WhenApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();
    assert!(matches!(
        by_name.get("day").expect("day field"),
        ElmTypeRepr::IsoDate
    ));
    assert!(matches!(
        by_name.get("stamp").expect("stamp field"),
        ElmTypeRepr::String
    ));
    assert!(matches!(
        by_name.get("clock").expect("clock field"),
        ElmTypeRepr::String
    ));
}

#[test]
fn build_merged_module_renders_expected_elm() {
    let types: Vec<ElmTypeInfo> =
        vec![PersonApi::elm_type_info(), PersonFilterApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");

    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    assert!(rendered.contains("module Api.Person exposing"));
    assert!(rendered.contains("type alias Person ="));
    assert!(rendered.contains("type alias PersonFilter ="));
    assert!(rendered.contains("personDecoder"));
    assert!(rendered.contains("encodePerson"));
    assert!(rendered.contains("import Json.Decode as Decode"));
    assert!(rendered.contains("import Json.Encode.Extra exposing (maybe)"));
    assert!(rendered.contains("nickname : Maybe String"));
}
