//! Snapshot tests for the full Elm module rendered by the builder.
//!
//! The pre-existing substring assertions in `smoke.rs` tell us the
//! output contains specific markers. These snapshots pin the *exact*
//! output so formatting/whitespace/pretty-printer regressions surface
//! as a reviewable diff rather than passing silently.
//!
//! Run `cargo insta review` (or `INSTA_UPDATE=auto cargo test`) to
//! accept intentional changes.

use elm_client_gen_builder::{
    build_merged_module, group_by_module, DefaultStrategy, MaybeEncoderRef, NameMap,
};
use elm_client_gen_core::{ElmType, ElmTypeInfo};
use test_better::prelude::*;

fn render(types: Vec<ElmTypeInfo>) -> TestResult<String> {
    let names = NameMap::from_types(&types);
    render_with_names(types, names)
}

fn render_with_names(types: Vec<ElmTypeInfo>, names: NameMap) -> TestResult<String> {
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().or_fail_with("one module group")?;
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    Ok(elm_ast::pretty_print(&module))
}

// ── Record ──────────────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Record", name = "Person")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapPersonApi {
    pub id: String,
    pub full_name: String,
    pub age: i32,
    pub nickname: Option<String>,
    pub tags: Vec<String>,
}

#[test]
fn snapshot_record_module() -> TestResult {
    let rendered = render(vec![SnapPersonApi::elm_type_info()])?;
    insta::assert_snapshot!("record_module", rendered);
    Ok(())
}

// ── Newtype ─────────────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Newtype", name = "UserId")]
#[allow(dead_code)]
pub struct SnapUserIdApi(String);

#[test]
fn snapshot_newtype_module() -> TestResult {
    let rendered = render(vec![SnapUserIdApi::elm_type_info()])?;
    insta::assert_snapshot!("newtype_module", rendered);
    Ok(())
}

// ── Bare-string enum ────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Status", name = "Status")]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum SnapStatusApi {
    Active,
    Archived,
    Banned,
}

#[test]
fn snapshot_bare_string_enum_module() -> TestResult {
    let rendered = render(vec![SnapStatusApi::elm_type_info()])?;
    insta::assert_snapshot!("bare_string_enum_module", rendered);
    Ok(())
}

// ── Internally-tagged enum with struct + unit variants ──────────────

#[derive(ElmType)]
#[elm(module = "Snap.Event", name = "Event")]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum SnapEventApi {
    Ping,
    Tick { at: String, jitter_ms: Option<i32> },
}

#[test]
fn snapshot_internally_tagged_enum_module() -> TestResult {
    let rendered = render(vec![SnapEventApi::elm_type_info()])?;
    insta::assert_snapshot!("internally_tagged_enum_module", rendered);
    Ok(())
}

// ── Untagged enum ───────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Value", name = "Value")]
#[serde(untagged)]
#[allow(dead_code)]
pub enum SnapValueApi {
    Text(String),
    Count(i64),
}

#[test]
fn snapshot_untagged_enum_module() -> TestResult {
    let rendered = render(vec![SnapValueApi::elm_type_info()])?;
    insta::assert_snapshot!("untagged_enum_module", rendered);
    Ok(())
}

// ── Tuple field ─────────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Geo", name = "Coordinates")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapCoordinatesApi {
    pub lat_lon: (f64, f64),
    pub bbox: (f64, f64, f64),
}

#[test]
fn snapshot_tuple_fields_module() -> TestResult {
    let rendered = render(vec![SnapCoordinatesApi::elm_type_info()])?;
    insta::assert_snapshot!("tuple_fields_module", rendered);
    Ok(())
}

// ── Internally-tagged newtype-of-struct (mergeTaggedObject helper) ──

#[derive(ElmType)]
#[elm(module = "Snap.Tagged", name = "Address")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapTaggedAddressApi {
    pub line1: String,
    pub city: String,
}

#[derive(ElmType)]
#[elm(module = "Snap.Tagged", name = "Action")]
#[serde(tag = "action")]
#[allow(dead_code)]
pub enum SnapTaggedActionApi {
    UpdateAddress(SnapTaggedAddressApi),
    Noop,
}

#[test]
fn snapshot_merge_tagged_object_module() -> TestResult {
    let rendered = render(vec![
        SnapTaggedAddressApi::elm_type_info(),
        SnapTaggedActionApi::elm_type_info(),
    ])?;
    insta::assert_snapshot!("merge_tagged_object_module", rendered);
    Ok(())
}

// ── App + decoder_step + encoder_pairs (0.3.0 wrapper hooks) ────────

#[allow(dead_code)]
pub struct SnapPatch<T>(std::marker::PhantomData<T>);

#[derive(ElmType)]
#[elm(module = "Snap.Profile", name = "ProfilePatch")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapProfilePatchApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub display_name: SnapPatch<String>,
    pub version: i32,
}

fn names_with_patch_module(types: &[ElmTypeInfo]) -> NameMap {
    let mut names = NameMap::from_types(types);
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
    names
}

#[test]
fn snapshot_patch_field_module() -> TestResult {
    let types = vec![SnapProfilePatchApi::elm_type_info()];
    let names = names_with_patch_module(&types);
    let rendered = render_with_names(types, names)?;
    insta::assert_snapshot!("patch_field_module", rendered);
    Ok(())
}

// ── Multiple types share one Elm module → one collapsed import ──────

#[derive(ElmType)]
#[elm(module = "Snap.MultiPatch", name = "Left")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapMultiPatchLeftApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub left: SnapPatch<String>,
}

#[derive(ElmType)]
#[elm(module = "Snap.MultiPatch", name = "Right")]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SnapMultiPatchRightApi {
    #[elm(
        type = "Patch String",
        decoder_step = "patch",
        encoder_pairs = "patchPair"
    )]
    pub right: SnapPatch<String>,
}

#[test]
fn snapshot_imports_collapse_for_same_target_module() -> TestResult {
    let types = vec![
        SnapMultiPatchLeftApi::elm_type_info(),
        SnapMultiPatchRightApi::elm_type_info(),
    ];
    let names = names_with_patch_module(&types);
    let rendered = render_with_names(types, names)?;
    insta::assert_snapshot!("imports_collapse_for_same_target_module", rendered);
    Ok(())
}
