//! End-to-end smoke test: derive a struct, run the builder, render with
//! `elm_ast::pretty_print`, and assert the output contains the expected
//! Elm declarations.

use elm_client_gen_builder::{
    build_merged_module, group_by_module, DefaultStrategy, MaybeEncoderRef, NameMap,
};
use elm_client_gen_core::{registered_types, ElmType, ElmTypeInfo};
use test_better::ErrorKind;
use test_better::prelude::*;

fn fail(msg: impl Into<String>) -> TestError {
    TestError::new(ErrorKind::Assertion).with_message(msg.into())
}

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
fn derive_emits_type_info_with_field_metadata() -> TestResult {
    let info = PersonApi::elm_type_info();

    check!(info.rust_name).satisfies(eq("PersonApi"))?;
    check!(info.module_path.as_slice()).satisfies(eq(["Api", "Person"].as_slice()))?;
    check!(info.type_name).satisfies(eq("Person"))?;
    check!(info.tags.as_slice()).satisfies(eq(["entity"].as_slice()))?;
    check!(info.has_tag("entity")).satisfies(is_true())?;

    check!(info.fields().len()).satisfies(eq(5))?;
    let full_name = info.fields().get(1).or_fail_with("fullName field")?;
    check!(full_name.rust_name).satisfies(eq("fullName"))?;
    check!(full_name.elm_name).satisfies(eq("fullName"))?;

    let nickname = info.fields().get(3).or_fail_with("nickname field")?;
    check!(nickname.elm_name).satisfies(eq("nickname"))?;
    check!(nickname.is_optional).satisfies(is_true())?;
    Ok(())
}

#[test]
fn skip_and_rename_attributes_are_honored() -> TestResult {
    let info = PersonFilterApi::elm_type_info();
    let names: Vec<&str> = info.fields().iter().map(|f| f.elm_name).collect();

    check!(names.contains(&"internal")).satisfies(is_false())?;
    check!(names.contains(&"nameContains")).satisfies(is_true())?;
    check!(names.contains(&"minAge")).satisfies(is_true())?;
    Ok(())
}

#[test]
fn registered_types_picks_up_derived_structs() -> TestResult {
    let names: Vec<&str> = registered_types().iter().map(|t| t.rust_name).collect();
    check!(names.contains(&"PersonApi")).satisfies(is_true())?;
    check!(names.contains(&"PersonFilterApi")).satisfies(is_true())?;
    Ok(())
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
fn enum_derive_emits_variant_metadata() -> TestResult {
    let info = TreasurerInvoiceStateApi::elm_type_info();

    check!(info.rust_name).satisfies(eq("TreasurerInvoiceStateApi"))?;
    check!(info.type_name).satisfies(eq("TreasurerInvoiceState"))?;
    check!(info.is_enum()).satisfies(is_true())?;

    let variants = info.variants();
    check!(variants.len()).satisfies(eq(7))?;

    let by_rust: std::collections::HashMap<_, _> = variants
        .iter()
        .map(|v| (v.rust_name, (v.elm_name, v.json_tag)))
        .collect();

    // Default Elm name = Rust ident; default json_tag = serde rename_all applied.
    check!(by_rust.get("Open").copied().or_fail_with("Open variant")?).satisfies(eq(("Open", "open")))?;
    check!(by_rust.get("Finalized").copied().or_fail_with("Finalized variant")?).satisfies(eq(("Finalized", "finalized")))?;
    check!(by_rust.get("Deleted").copied().or_fail_with("Deleted variant")?).satisfies(eq(("Deleted", "deleted")))?;
    Ok(())
}

#[test]
fn enum_module_renders_type_decoder_and_encoder() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![TreasurerInvoiceStateApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type declaration uses `type` (not `type alias`) and lists constructors.
    check!(rendered.as_str())
        .satisfies(contains_str("type TreasurerInvoiceState"))
        .context(format!("missing custom type declaration:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Open"))
        .context(format!("missing Open constructor:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Deleted"))
        .context(format!("missing Deleted constructor:\n{rendered}"))?;

    // Exposing list opens constructors so callers can pattern-match.
    check!(rendered.as_str())
        .satisfies(contains_str("TreasurerInvoiceState(..)"))
        .context(format!("expected TreasurerInvoiceState(..) in exposing:\n{rendered}"))?;

    // Decoder dispatches on the json string tags (snake_case).
    check!(rendered.as_str())
        .satisfies(contains_str("treasurerInvoiceStateDecoder"))
        .context(format!("missing decoder:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#""open""#))
        .context(format!("missing snake_case json tag:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#""finalized""#))
        .context(format!("missing snake_case json tag:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.string"))
        .context(format!("decoder should start from Decode.string:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.andThen"))
        .context(format!("decoder should use andThen:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.fail"))
        .context(format!("decoder should fail on unknown tag:\n{rendered}"))?;

    // Encoder uses `case` and Encode.string with the same tags.
    check!(rendered.as_str())
        .satisfies(contains_str("encodeTreasurerInvoiceState"))
        .context(format!("missing encoder:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Encode.string"))
        .context(format!("encoder should call Encode.string:\n{rendered}"))?;
    Ok(())
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
fn tagged_enum_metadata_carries_payload_and_tag_key() -> TestResult {
    let info = UserEmailAddressApi::elm_type_info();
    check!(info.is_enum()).satisfies(is_true())?;
    let variants = info.variants();
    check!(variants.len()).satisfies(eq(3))?;

    let confirmed = variants
        .iter()
        .find(|v| v.rust_name == "Confirmed")
        .or_fail_with("Confirmed variant")?;
    let confirmed_fields = confirmed.payload.struct_fields();
    check!(confirmed_fields.len()).satisfies(eq(2))?;
    let confirmed_at = confirmed_fields.first().or_fail_with("confirmed_at field")?;
    check!(confirmed_at.is_optional).satisfies(is_true())?;
    check!(confirmed_at.elm_name).satisfies(eq("confirmedAt"))?;

    let locked = variants
        .iter()
        .find(|v| v.rust_name == "Locked")
        .or_fail_with("Locked variant")?;
    check!(locked.payload.is_unit()).satisfies(is_true())?;
    Ok(())
}

#[test]
fn tagged_enum_renders_struct_variants_with_anonymous_records() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![UserEmailAddressApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    check!(rendered.as_str())
        .satisfies(contains_str("type UserEmailAddress"))
        .context(format!("missing custom type:\n{rendered}"))?;
    // Struct variants render with an anonymous record arg.
    check!(rendered.as_str())
        .satisfies(contains_str("Confirmed {"))
        .context(format!("expected `Confirmed {{` in rendered output:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Unconfirmed {"))
        .context(format!("expected `Unconfirmed {{` in rendered output:\n{rendered}"))?;
    // Unit variants stay bare.
    check!(rendered.as_str())
        .satisfies(contains_str("| Locked"))
        .context(format!("expected `| Locked` in rendered output:\n{rendered}"))?;

    // Decoder reads the discriminator field and dispatches.
    check!(rendered.as_str())
        .satisfies(contains_str(r#"Decode.field "type""#))
        .context(format!("decoder should read the tag field:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.andThen"))
        .context(format!("decoder should use andThen:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#""Confirmed""#))
        .context(format!("decoder should branch on Confirmed:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#"required "email_address""#))
        .context(format!("Confirmed branch should require email_address:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#"optional "confirmed_at""#))
        .context(format!("Confirmed branch should make confirmed_at optional:\n{rendered}"))?;

    // Encoder pattern-matches on the constructor with a payload binding
    // and emits the tag alongside the payload fields.
    check!(rendered.as_str())
        .satisfies(contains_str("Confirmed payload"))
        .context(format!("encoder should bind payload in Confirmed branch:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("payload.emailAddress"))
        .context(format!("encoder should access payload.emailAddress:\n{rendered}"))?;
    check!(
        rendered.contains(r#"( "type", Encode.string "Confirmed" )"#)
            || rendered.contains(r#"("type", Encode.string "Confirmed")"#)
    )
    .satisfies(is_true())
    .context(format!("encoder should emit the tag pair:\n{rendered}"))?;

    // Time/Iso8601 imports kick in because of the Posix payload field.
    check!(rendered.as_str())
        .satisfies(contains_str("import Time"))
        .context(format!("expected Time import:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("import Iso8601"))
        .context(format!("expected Iso8601 import:\n{rendered}"))?;
    Ok(())
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
fn untagged_enum_metadata_carries_variant_payloads() -> TestResult {
    use elm_client_gen_core::{ElmTypeKind, ElmTypeRepr, ElmVariantPayload, EnumRepresentation};

    let info = SearchHitApi::elm_type_info();
    let ElmTypeKind::Enum {
        variants,
        representation,
    } = &info.kind
    else {
        return Err(fail("expected enum kind"));
    };
    check!(matches!(representation, EnumRepresentation::Untagged)).satisfies(is_true())?;
    check!(variants.len()).satisfies(eq(3))?;

    let term = variants
        .iter()
        .find(|v| v.rust_name == "Term")
        .or_fail_with("Term variant")?;
    match &term.payload {
        ElmVariantPayload::Newtype(ElmTypeRepr::String) => {}
        other => return Err(fail(format!("expected Newtype(String) for Term, got {other:?}"))),
    }

    let range = variants
        .iter()
        .find(|v| v.rust_name == "Range")
        .or_fail_with("Range variant")?;
    let range_fields = range.payload.struct_fields();
    check!(range_fields.len()).satisfies(eq(2))?;
    check!(range_fields.first().or_fail_with("first range field")?.elm_name).satisfies(eq("from"))?;

    let empty = variants
        .iter()
        .find(|v| v.rust_name == "Empty")
        .or_fail_with("Empty variant")?;
    check!(empty.payload.is_unit()).satisfies(is_true())?;
    Ok(())
}

#[test]
fn untagged_enum_renders_oneof_decoder_and_tag_free_encoder() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![SearchHitApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type renders newtype variant with positional arg, struct variant
    // with anonymous record, unit variant bare.
    check!(rendered.as_str())
        .satisfies(contains_str("Term String"))
        .context(format!("missing Term String constructor:\n{rendered}"))?;
    check!(
        rendered.contains("Range") && rendered.contains("{ from : Int")
    )
    .satisfies(is_true())
    .context(format!("missing Range struct constructor with anonymous record:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("| Empty"))
        .context(format!("missing Empty unit constructor:\n{rendered}"))?;

    // Decoder uses Decode.oneOf, NOT Decode.field "type" / andThen.
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.oneOf"))
        .context(format!("untagged decoder must use Decode.oneOf:\n{rendered}"))?;
    check!(rendered.contains(r#"Decode.field "type""#))
        .satisfies(is_false())
        .context(format!("untagged decoder must not read a tag field:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.map Term"))
        .context(format!("missing Decode.map Term:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.null Empty"))
        .context(format!("missing Decode.null Empty for unit variant:\n{rendered}"))?;

    // Encoder is tag-free: each branch emits the inner shape directly.
    check!(rendered.as_str())
        .satisfies(contains_str("encodeSearchHit"))
        .context(format!("missing encoder:\n{rendered}"))?;
    check!(rendered.contains(r#"( "type", Encode.string"#))
        .satisfies(is_false())
        .context(format!("untagged encoder must not emit a tag pair:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Term inner"))
        .context(format!("encoder should bind newtype payload as `inner`:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Encode.string inner"))
        .context(format!("newtype Term should encode inner directly:\n{rendered}"))?;
    check!(
        rendered.contains("Empty ->\n            Encode.null")
            || rendered.contains("Empty ->\n        Encode.null")
            || rendered.contains("Empty ->") && rendered.contains("Encode.null")
    )
    .satisfies(is_true())
    .context(format!("Empty branch should encode as null:\n{rendered}"))?;
    Ok(())
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
fn newtype_struct_emits_type_alias() -> TestResult {
    use elm_client_gen_core::{ElmTypeKind, ElmTypeRepr};

    let info = UserIdApi::elm_type_info();
    match &info.kind {
        ElmTypeKind::Newtype {
            inner: ElmTypeRepr::String,
        } => {}
        other => return Err(fail(format!("expected Newtype(String), got {other:?}"))),
    }
    check!(info.is_newtype()).satisfies(is_true())?;
    Ok(())
}

#[test]
fn newtype_struct_renders_alias_and_delegating_codec() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![UserIdApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Renders as a type alias, not a custom type with a constructor.
    check!(rendered.as_str())
        .satisfies(contains_str("type alias UserId"))
        .context(format!("expected `type alias UserId ...`:\n{rendered}"))?;
    let alias_idx = rendered
        .find("type alias UserId")
        .or_fail_with("rendered output should contain the type alias")?;
    let after = &rendered[alias_idx..];
    check!(after.contains("String"))
        .satisfies(is_true())
        .context(format!("alias should resolve to String:\n{rendered}"))?;

    // Decoder/encoder delegate to String's codec via type-alias transparency.
    check!(rendered.as_str())
        .satisfies(contains_str("userIdDecoder"))
        .context(format!("missing decoder:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.string"))
        .context(format!("decoder should delegate to Decode.string:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("encodeUserId"))
        .context(format!("missing encoder:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Encode.string value"))
        .context(format!("encoder should delegate to Encode.string:\n{rendered}"))?;
    Ok(())
}

#[test]
fn box_and_arc_passthrough_to_inner_type() -> TestResult {
    let info = WrappedApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();
    use elm_client_gen_core::ElmTypeRepr;
    check!(matches!(
        by_name.get("boxedName").or_fail_with("boxedName field")?,
        ElmTypeRepr::String
    ))
    .satisfies(is_true())?;
    check!(matches!(
        by_name.get("arcCount").or_fail_with("arcCount field")?,
        ElmTypeRepr::Int
    ))
    .satisfies(is_true())?;
    Ok(())
}

#[test]
fn custom_encoder_attribute_substitutes_field_encoder() -> TestResult {
    let info = WrappedApi::elm_type_info();
    let price = info
        .fields()
        .iter()
        .find(|f| f.elm_name == "price")
        .or_fail_with("price field")?;
    check!(price.custom_encoder).satisfies(eq(Some("Money.encode")))?;

    let types: Vec<ElmTypeInfo> = vec![WrappedApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Custom encoder substituted: should call Money.encode, NOT Encode.int.
    check!(rendered.as_str())
        .satisfies(contains_str("Money.encode value.price"))
        .context(format!("encoder should delegate to Money.encode:\n{rendered}"))?;
    check!(rendered.contains("Encode.int value.price"))
        .satisfies(is_false())
        .context(format!("encoder should not use the type-driven encoder for `price`:\n{rendered}"))?;
    Ok(())
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
fn tuple_field_repr_carries_inner_types() -> TestResult {
    use elm_client_gen_core::ElmTypeRepr;

    let info = CoordinatesApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();

    match by_name.get("latLon").or_fail_with("latLon field")? {
        ElmTypeRepr::Tuple(elems) => {
            check!(elems.len()).satisfies(eq(2))?;
            check!(matches!(
                elems.first().or_fail_with("first tuple element")?,
                ElmTypeRepr::Float
            ))
            .satisfies(is_true())?;
            check!(matches!(
                elems.get(1).or_fail_with("second tuple element")?,
                ElmTypeRepr::Float
            ))
            .satisfies(is_true())?;
        }
        other => return Err(fail(format!("expected Tuple for latLon, got {other:?}"))),
    }
    match by_name.get("boundingBox").or_fail_with("boundingBox field")? {
        ElmTypeRepr::Tuple(elems) => check!(elems.len()).satisfies(eq(3))?,
        other => return Err(fail(format!("expected Tuple for boundingBox, got {other:?}"))),
    }
    Ok(())
}

#[test]
fn tuple_renders_type_decoder_and_encoder() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![CoordinatesApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Type annotation renders as a tuple.
    check!(
        rendered.contains("( Float, Float )") || rendered.contains("(Float, Float)")
    )
    .satisfies(is_true())
    .context(format!("expected 2-tuple type annotation:\n{rendered}"))?;

    // Decoder uses Decode.map2/map3 with positional Decode.index.
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.map2"))
        .context(format!("decoder should use Decode.map2 for 2-tuple:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.map3"))
        .context(format!("decoder should use Decode.map3 for 3-tuple:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.index 0"))
        .context(format!("decoder should index 0:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.index 1"))
        .context(format!("decoder should index 1:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.index 2"))
        .context(format!("decoder should index 2 for 3-tuple:\n{rendered}"))?;

    // Encoder destructures tuple via lambda pattern and emits Encode.list with identity.
    check!(rendered.as_str())
        .satisfies(contains_str("Encode.list"))
        .context(format!("encoder should use Encode.list for tuple:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("identity"))
        .context(format!("encoder should pass identity as the per-element encoder:\n{rendered}"))?;
    check!(
        rendered.contains("\\( a, b )") || rendered.contains("\\(a, b)")
    )
    .satisfies(is_true())
    .context(format!("encoder should destructure 2-tuple in lambda:\n{rendered}"))?;
    Ok(())
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
fn chrono_naive_types_map_to_iso_date_and_string() -> TestResult {
    use elm_client_gen_core::ElmTypeRepr;
    let info = WhenApi::elm_type_info();
    let by_name: std::collections::HashMap<_, _> = info
        .fields()
        .iter()
        .map(|f| (f.elm_name, &f.elm_type))
        .collect();
    check!(matches!(
        by_name.get("day").or_fail_with("day field")?,
        ElmTypeRepr::IsoDate
    ))
    .satisfies(is_true())?;
    check!(matches!(
        by_name.get("stamp").or_fail_with("stamp field")?,
        ElmTypeRepr::String
    ))
    .satisfies(is_true())?;
    check!(matches!(
        by_name.get("clock").or_fail_with("clock field")?,
        ElmTypeRepr::String
    ))
    .satisfies(is_true())?;
    Ok(())
}

#[test]
fn build_merged_module_renders_expected_elm() -> TestResult {
    let types: Vec<ElmTypeInfo> =
        vec![PersonApi::elm_type_info(), PersonFilterApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;

    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    check!(rendered.as_str()).satisfies(contains_str("module Api.Person exposing"))?;
    check!(rendered.as_str()).satisfies(contains_str("type alias Person ="))?;
    check!(rendered.as_str()).satisfies(contains_str("type alias PersonFilter ="))?;
    check!(rendered.as_str()).satisfies(contains_str("personDecoder"))?;
    check!(rendered.as_str()).satisfies(contains_str("encodePerson"))?;
    check!(rendered.as_str()).satisfies(contains_str("import Json.Decode as Decode"))?;
    check!(rendered.as_str()).satisfies(contains_str("import Json.Encode.Extra exposing (maybe)"))?;
    check!(rendered.as_str()).satisfies(contains_str("nickname : Maybe String"))?;
    Ok(())
}

// ── mergeTaggedObject helper emission ───────────────────────────────
//
// An internally-tagged enum whose newtype variant carries another
// object-shaped type (record struct or another internally-tagged enum)
// must flatten the inner object's fields alongside the outer tag.
// That's done via a private `mergeTaggedObject` helper the builder
// emits into the module, and which the variant encoder invokes.

#[derive(ElmType)]
#[elm(module = "Api.Tagged", name = "AddressApi")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct MergeAddressApi {
    pub line1: String,
    pub city: String,
}

#[derive(ElmType)]
#[elm(module = "Api.Tagged", name = "Action")]
#[serde(tag = "action")]
#[allow(dead_code)]
pub enum MergeActionApi {
    UpdateAddress(MergeAddressApi),
    Noop,
}

#[test]
fn merge_tagged_object_helper_emitted_for_internally_tagged_newtype_struct() -> TestResult {
    let types: Vec<ElmTypeInfo> = vec![
        MergeAddressApi::elm_type_info(),
        MergeActionApi::elm_type_info(),
    ];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    // Helper is declared in the module.
    check!(rendered.as_str())
        .satisfies(contains_str("mergeTaggedObject :"))
        .context(format!("expected mergeTaggedObject type annotation in module:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("mergeTaggedObject tagKey tagValue inner"))
        .context(format!("expected mergeTaggedObject implementation:\n{rendered}"))?;
    // Helper body uses Decode.keyValuePairs and Encode.object to flatten.
    check!(rendered.as_str())
        .satisfies(contains_str("Decode.keyValuePairs"))
        .context(format!("mergeTaggedObject should decode inner as keyValuePairs:\n{rendered}"))?;
    check!(rendered.as_str())
        .satisfies(contains_str("Encode.object"))
        .context(format!("mergeTaggedObject should rebuild via Encode.object:\n{rendered}"))?;

    // Variant encoder for UpdateAddress binds the inner payload and calls
    // the helper with the tag key, variant name, and the inner encoded value.
    check!(rendered.as_str())
        .satisfies(contains_str("UpdateAddress inner"))
        .context(format!("UpdateAddress branch should bind inner:\n{rendered}"))?;
    check!(
        rendered.contains("mergeTaggedObject \"action\" \"UpdateAddress\"")
            || rendered.contains("mergeTaggedObject \"action\" \"UpdateAddress\" (")
    )
    .satisfies(is_true())
    .context(format!("UpdateAddress branch should call mergeTaggedObject with action/UpdateAddress:\n{rendered}"))?;
    check!(
        rendered.contains("encodeAddressApi inner") || rendered.contains("encodeAddressApi")
    )
    .satisfies(is_true())
    .context(format!("UpdateAddress branch should invoke the inner type's encoder:\n{rendered}"))?;
    Ok(())
}

#[test]
fn merge_tagged_object_helper_not_emitted_when_unused() -> TestResult {
    // Internally-tagged enum with only unit and struct variants (no
    // newtype variant) should NOT pull in the mergeTaggedObject helper.
    let types: Vec<ElmTypeInfo> = vec![UserEmailAddressApi::elm_type_info()];
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");

    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    let rendered = elm_ast::pretty_print(&module);

    check!(rendered.contains("mergeTaggedObject"))
        .satisfies(is_false())
        .context(format!("helper should not be emitted when no variant needs it:\n{rendered}"))?;
    Ok(())
}

// ── App type + decoder_step / encoder_pairs (0.3.0) ─────────────────
//
// `Patch<T>` stands in for any consumer-supplied wrapper that needs
// hand-written codec helpers: a pipeline-step combinator (`patch`) and
// a pairs-emitting encoder helper (`patchPair`). The `#[elm(type = ...)]`
// override turns the field's Elm type into `App { head: "Patch", args: [..] }`,
// and the consumer registers the wrapper module via
// `NameMap::register_with_exposed` so the import emits the exact
// helpers the rendered decoder/encoder reference.

#[allow(dead_code)]
pub struct Patch<T>(std::marker::PhantomData<T>);

#[derive(ElmType)]
#[elm(module = "Api.Profile", name = "ProfilePatch")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ProfilePatchApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub display_name: Patch<String>,
    pub version: i32,
}

fn render_with_patch_module(types: Vec<ElmTypeInfo>) -> TestResult<String> {
    let mut names = NameMap::from_types(&types);
    names.register_with_exposed(
        "Patch",
        "Patch",
        vec!["Api".into(), "Patch".into()],
        vec![
            "Patch".into(),
            "patch".into(),
            "patchPair".into(),
            "patchDecoder".into(),
            "encodePatch".into(),
        ],
    );
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&types);
    let (module_path, group) = groups
        .into_iter()
        .next()
        .or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    Ok(elm_ast::pretty_print(&module))
}

#[test]
fn app_type_repr_carries_head_and_args() -> TestResult {
    use elm_client_gen_core::ElmTypeRepr;
    let info = ProfilePatchApi::elm_type_info();
    let display = info
        .fields()
        .iter()
        .find(|f| f.elm_name == "displayName")
        .or_fail_with("displayName field")?;
    match &display.elm_type {
        ElmTypeRepr::App { head, args } => {
            check!(head.as_str()).satisfies(eq("Patch"))?;
            check!(args.len()).satisfies(eq(1))?;
            let first = args.first().or_fail_with("App should have one arg")?;
            check!(matches!(first, ElmTypeRepr::String)).satisfies(is_true())?;
        }
        other => return Err(fail(format!("expected App {{ head: Patch, args: [String] }}, got {other:?}"))),
    }
    check!(display.decoder_step).satisfies(eq(Some("patch")))?;
    check!(display.encoder_pairs).satisfies(eq(Some("patchPair")))?;
    Ok(())
}

#[test]
fn app_type_renders_head_with_arg_in_field_annotation() -> TestResult {
    let rendered = render_with_patch_module(vec![ProfilePatchApi::elm_type_info()])?;
    // The displayName field is annotated as `Patch String`, not as a
    // bare `Patch` ident or as `String`.
    check!(rendered.as_str())
        .satisfies(contains_str("displayName : Patch String"))
        .context(format!("expected `displayName : Patch String` annotation:\n{rendered}"))?;
    Ok(())
}

#[test]
fn decoder_step_emits_pipeline_step_combinator() -> TestResult {
    let rendered = render_with_patch_module(vec![ProfilePatchApi::elm_type_info()])?;
    // `decoder_step = "patch"` overrides the default `required` /
    // `optional` step. The rust_name is the JSON key (camelCase here
    // because of `serde(rename_all = "camelCase")`) and the inner
    // decoder is built from the App's first arg (`String`).
    check!(rendered.as_str())
        .satisfies(contains_str(r#"|> patch "displayName" Decode.string"#))
        .context(format!("expected pipeline step `|> patch \"displayName\" Decode.string`:\n{rendered}"))?;
    // It must NOT fall back to `required "displayName" patchDecoder`.
    check!(rendered.contains(r#"|> required "displayName""#))
        .satisfies(is_false())
        .context(format!("decoder_step should preempt `required`:\n{rendered}"))?;
    Ok(())
}

#[test]
fn encoder_pairs_wraps_body_in_list_concat() -> TestResult {
    let rendered = render_with_patch_module(vec![ProfilePatchApi::elm_type_info()])?;
    // Any field with `encoder_pairs` flips the whole record encoder
    // body to `Encode.object (List.concat [ ... ])`.
    check!(rendered.contains("Encode.object") && rendered.contains("List.concat"))
        .satisfies(is_true())
        .context(format!("expected `Encode.object (List.concat [...])` body:\n{rendered}"))?;
    // The pairs field uses the helper directly with no list wrapper.
    check!(rendered.as_str())
        .satisfies(contains_str(r#"patchPair "displayName" Encode.string value.displayName"#))
        .context(format!("expected pairs helper call:\n{rendered}"))?;
    // Plain fields are wrapped in a singleton list inside the concat.
    check!(
        rendered.contains(r#"[ ( "version", Encode.int value.version ) ]"#)
            || rendered.contains(r#"[("version", Encode.int value.version)]"#)
    )
    .satisfies(is_true())
    .context(format!("expected plain field wrapped as singleton pair list:\n{rendered}"))?;
    Ok(())
}

#[test]
fn app_type_imports_head_via_register_with_exposed() -> TestResult {
    let rendered = render_with_patch_module(vec![ProfilePatchApi::elm_type_info()])?;
    // The Patch wrapper is registered with an explicit exposing list,
    // so the import line uses it verbatim instead of the auto-derived
    // `<elm_name> / <elm_name>Decoder / encode<elm_name>` triple.
    let import = rendered
        .lines()
        .find(|l| l.contains("import Api.Patch"))
        .ok_or_else(|| fail(format!("expected `import Api.Patch ...` line:\n{rendered}")))?;
    check!(import.contains("Patch"))
        .satisfies(is_true())
        .context(format!("import should expose Patch type:\n{import}"))?;
    check!(import.contains("patch"))
        .satisfies(is_true())
        .context(format!("import should expose patch combinator:\n{import}"))?;
    check!(import.contains("patchPair"))
        .satisfies(is_true())
        .context(format!("import should expose patchPair helper:\n{import}"))?;
    Ok(())
}

// ── Precedence: step/pairs win over decoder/encoder ─────────────────
//
// The derive currently lets a user set both `decoder = "..."` and
// `decoder_step = "..."` on the same field; the contract is that the
// step combinator wins. Same for encoder vs encoder_pairs. Building
// `ElmFieldInfo` by hand exercises the precedence logic without
// piling up overlapping derive attributes.

fn patch_field(
    rust_name: &'static str,
    custom_decoder: Option<&'static str>,
    decoder_step: Option<&'static str>,
    custom_encoder: Option<&'static str>,
    encoder_pairs: Option<&'static str>,
) -> elm_client_gen_core::ElmFieldInfo {
    elm_client_gen_core::ElmFieldInfo {
        rust_name,
        elm_name: rust_name,
        elm_type: elm_client_gen_core::ElmTypeRepr::App {
            head: "Patch".into(),
            args: vec![elm_client_gen_core::ElmTypeRepr::String],
        },
        is_optional: false,
        custom_decoder,
        custom_encoder,
        decoder_step,
        encoder_pairs,
    }
}

fn render_record_with_fields(fields: Vec<elm_client_gen_core::ElmFieldInfo>) -> TestResult<String> {
    use elm_client_gen_core::{ElmTypeInfo, ElmTypeKind};
    let info = ElmTypeInfo {
        rust_name: "Hand",
        module_path: vec!["Api", "Hand"],
        type_name: "Hand",
        tags: vec![],
        kind: ElmTypeKind::Record { fields },
    };
    render_with_patch_module(vec![info])
}

#[test]
fn decoder_step_wins_over_custom_decoder_when_both_set() -> TestResult {
    let rendered = render_record_with_fields(vec![patch_field(
        "name",
        Some("loserDecoder"),
        Some("patch"),
        None,
        None,
    )])?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#"|> patch "name" Decode.string"#))
        .context(format!("decoder_step should be emitted:\n{rendered}"))?;
    check!(rendered.contains("loserDecoder"))
        .satisfies(is_false())
        .context(format!("custom_decoder should be dropped when decoder_step is set:\n{rendered}"))?;
    Ok(())
}

#[test]
fn encoder_pairs_wins_over_custom_encoder_when_both_set() -> TestResult {
    let rendered = render_record_with_fields(vec![patch_field(
        "name",
        None,
        None,
        Some("loserEncoder"),
        Some("patchPair"),
    )])?;
    check!(rendered.as_str())
        .satisfies(contains_str(r#"patchPair "name" Encode.string value.name"#))
        .context(format!("encoder_pairs should be emitted:\n{rendered}"))?;
    check!(rendered.contains("loserEncoder"))
        .satisfies(is_false())
        .context(format!("custom_encoder should be dropped when encoder_pairs is set:\n{rendered}"))?;
    Ok(())
}

// ── Multiple types collapse into a single import per Elm module ─────

#[derive(ElmType)]
#[elm(module = "Api.MultiPatch", name = "Left")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct MultiPatchLeftApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub left: Patch<String>,
}

#[derive(ElmType)]
#[elm(module = "Api.MultiPatch", name = "Right")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct MultiPatchRightApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub right: Patch<String>,
}

#[test]
fn multiple_app_fields_emit_a_single_import_for_the_wrapper_module() -> TestResult {
    let rendered = render_with_patch_module(vec![
        MultiPatchLeftApi::elm_type_info(),
        MultiPatchRightApi::elm_type_info(),
    ])?;
    let count = rendered.matches("import Api.Patch").count();
    check!(count)
        .satisfies(eq(1))
        .context(format!("expected exactly one `import Api.Patch ...` line, got {count}:\n{rendered}"))?;
    Ok(())
}
