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

fn render(types: Vec<ElmTypeInfo>) -> String {
    let names = NameMap::from_types(&types);
    let strategy = DefaultStrategy;
    let maybe = MaybeEncoderRef::new(vec!["Json", "Encode", "Extra"], "maybe");
    let groups = group_by_module(&types);
    let (module_path, group) = groups.into_iter().next().expect("one module group");
    let module = build_merged_module(&module_path, &group, &names, &strategy, &maybe);
    elm_ast::pretty_print(&module)
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
fn snapshot_record_module() {
    let rendered = render(vec![SnapPersonApi::elm_type_info()]);
    insta::assert_snapshot!("record_module", rendered);
}

// ── Newtype ─────────────────────────────────────────────────────────

#[derive(ElmType)]
#[elm(module = "Snap.Newtype", name = "UserId")]
#[allow(dead_code)]
pub struct SnapUserIdApi(String);

#[test]
fn snapshot_newtype_module() {
    let rendered = render(vec![SnapUserIdApi::elm_type_info()]);
    insta::assert_snapshot!("newtype_module", rendered);
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
fn snapshot_bare_string_enum_module() {
    let rendered = render(vec![SnapStatusApi::elm_type_info()]);
    insta::assert_snapshot!("bare_string_enum_module", rendered);
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
fn snapshot_internally_tagged_enum_module() {
    let rendered = render(vec![SnapEventApi::elm_type_info()]);
    insta::assert_snapshot!("internally_tagged_enum_module", rendered);
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
fn snapshot_untagged_enum_module() {
    let rendered = render(vec![SnapValueApi::elm_type_info()]);
    insta::assert_snapshot!("untagged_enum_module", rendered);
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
fn snapshot_tuple_fields_module() {
    let rendered = render(vec![SnapCoordinatesApi::elm_type_info()]);
    insta::assert_snapshot!("tuple_fields_module", rendered);
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
fn snapshot_merge_tagged_object_module() {
    let rendered = render(vec![
        SnapTaggedAddressApi::elm_type_info(),
        SnapTaggedActionApi::elm_type_info(),
    ]);
    insta::assert_snapshot!("merge_tagged_object_module", rendered);
}
