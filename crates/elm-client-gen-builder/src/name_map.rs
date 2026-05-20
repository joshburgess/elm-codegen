use std::collections::HashMap;

use elm_client_gen_core::ElmTypeInfo;

/// One entry per Elm type known to the codegen, populated from
/// `ElmTypeInfo` and any consumer-supplied externs.
#[derive(Clone)]
pub struct NameEntry {
    pub elm_name: String,
    pub module_path: Vec<String>,
    /// If set, the import emitted for this entry's module exposes
    /// exactly this list instead of the auto-derived
    /// `<elm_name>` / `<elm_name>Decoder` / `encode<elm_name>`. Used for
    /// hand-written helper modules whose codecs don't follow the
    /// derive-based naming convention (e.g. a `Patch` module that
    /// exposes free-standing `patch` and `patchPair` helpers in
    /// addition to the `Patch` type).
    pub exposed_overrides: Option<Vec<String>>,
}

/// Resolves Rust type names referenced via `ElmTypeRepr::Custom(_)` to
/// their Elm names and modules.
///
/// When a struct field is `Vec<BrandAtomApi>`, the proc-macro emits
/// `Custom("BrandAtomApi")`. The builder consults this map to render
/// the correct Elm name (`BrandAtom`) and emit the right import.
pub struct NameMap {
    map: HashMap<String, NameEntry>,
}

impl NameMap {
    pub fn from_types(types: &[ElmTypeInfo]) -> Self {
        let mut map: HashMap<String, NameEntry> = HashMap::new();
        for info in types {
            map.insert(
                info.rust_name.to_string(),
                NameEntry {
                    elm_name: info.type_name.to_string(),
                    module_path: info.module_path.iter().map(|s| s.to_string()).collect(),
                    exposed_overrides: None,
                },
            );
        }
        NameMap { map }
    }

    /// Register a type that wasn't picked up via `ElmType` (e.g. a
    /// hand-written helper module the consumer ships).
    pub fn register(
        &mut self,
        rust_name: impl Into<String>,
        elm_name: impl Into<String>,
        module_path: Vec<String>,
    ) {
        self.map.insert(
            rust_name.into(),
            NameEntry {
                elm_name: elm_name.into(),
                module_path,
                exposed_overrides: None,
            },
        );
    }

    /// Register a hand-written module entry along with the exact list of
    /// names to expose when importing from that module. Use this when
    /// the codec helpers don't follow the auto-derived
    /// `<elm_name>Decoder`/`encode<elm_name>` convention — for example a
    /// `Patch` module whose users invoke `patch` (a pipeline-step
    /// combinator) and `patchPair` (an encoder helper) by name through
    /// the `decoder_step` / `encoder_pairs` field attributes.
    ///
    /// `exposed` is taken as the complete exposing list for imports of
    /// the registered module: it should include the type name, any
    /// helper functions referenced by `decoder_step` /
    /// `encoder_pairs`, and any other names downstream encoders /
    /// decoders refer to. Multiple entries pointing at the same module
    /// are merged into one import and their exposing lists unioned.
    pub fn register_with_exposed(
        &mut self,
        rust_name: impl Into<String>,
        elm_name: impl Into<String>,
        module_path: Vec<String>,
        exposed: Vec<String>,
    ) {
        self.map.insert(
            rust_name.into(),
            NameEntry {
                elm_name: elm_name.into(),
                module_path,
                exposed_overrides: Some(exposed),
            },
        );
    }

    /// Returns the Elm name for a Rust type name, falling back to the
    /// input if no mapping is registered.
    pub fn resolve<'a>(&'a self, rust_name: &'a str) -> &'a str {
        self.map
            .get(rust_name)
            .map(|e| e.elm_name.as_str())
            .unwrap_or(rust_name)
    }

    /// Look up the full entry (used when emitting imports).
    pub fn lookup(&self, rust_name: &str) -> Option<&NameEntry> {
        self.map.get(rust_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elm_client_gen_core::{ElmTypeInfo, ElmTypeKind};
    use test_better::prelude::*;

    fn record(
        rust_name: &'static str,
        type_name: &'static str,
        module: Vec<&'static str>,
    ) -> ElmTypeInfo {
        ElmTypeInfo {
            rust_name,
            module_path: module,
            type_name,
            tags: vec![],
            kind: ElmTypeKind::Record { fields: vec![] },
        }
    }

    fn module_path_strs(path: &[String]) -> Vec<&str> {
        path.iter().map(String::as_str).collect()
    }

    #[test]
    fn from_types_builds_entry_per_type() -> TestResult {
        let types = vec![
            record("PersonApi", "Person", vec!["Api", "Person"]),
            record("OrderApi", "Order", vec!["Api", "Order"]),
        ];
        let map = NameMap::from_types(&types);
        let p = map.lookup("PersonApi").or_fail_with("Person entry")?;
        check!(p.elm_name.as_str()).satisfies(eq("Person"))?;
        check!(module_path_strs(&p.module_path).as_slice())
            .satisfies(eq(["Api", "Person"].as_slice()))?;
        check!(map.resolve("OrderApi")).satisfies(eq("Order"))?;
        Ok(())
    }

    #[test]
    fn resolve_falls_back_to_input_on_miss() -> TestResult {
        let map = NameMap::from_types(&[]);
        check!(map.resolve("Unknown")).satisfies(eq("Unknown"))?;
        check!(map.lookup("Unknown").is_none()).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn register_adds_hand_written_entries() -> TestResult {
        let mut map = NameMap::from_types(&[]);
        map.register("Money", "Money", vec!["Api".into(), "Money".into()]);
        let e = map.lookup("Money").or_fail_with("registered Money entry")?;
        check!(e.elm_name.as_str()).satisfies(eq("Money"))?;
        check!(module_path_strs(&e.module_path).as_slice())
            .satisfies(eq(["Api", "Money"].as_slice()))?;
        Ok(())
    }

    #[test]
    fn register_overwrites_existing_entry() -> TestResult {
        let types = vec![record("Overlap", "FirstName", vec!["Api", "First"])];
        let mut map = NameMap::from_types(&types);
        map.register("Overlap", "SecondName", vec!["Api".into(), "Second".into()]);
        let e = map.lookup("Overlap").or_fail_with("overwrite entry")?;
        check!(e.elm_name.as_str()).satisfies(eq("SecondName"))?;
        check!(module_path_strs(&e.module_path).as_slice())
            .satisfies(eq(["Api", "Second"].as_slice()))?;
        Ok(())
    }

    #[test]
    fn register_with_exposed_records_overrides_verbatim() -> TestResult {
        let mut map = NameMap::from_types(&[]);
        map.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "Patch".into()],
            vec!["Patch".into(), "patch".into(), "patchPair".into()],
        );
        let e = map.lookup("Patch").or_fail_with("Patch entry")?;
        check!(e.elm_name.as_str()).satisfies(eq("Patch"))?;
        check!(module_path_strs(&e.module_path).as_slice())
            .satisfies(eq(["Api", "Patch"].as_slice()))?;
        let exposed = e
            .exposed_overrides
            .as_deref()
            .or_fail_with("exposed_overrides should be set")?;
        check!(module_path_strs(exposed).as_slice()).satisfies(eq([
            "Patch",
            "patch",
            "patchPair",
        ]
        .as_slice()))?;
        Ok(())
    }

    #[test]
    fn register_leaves_exposed_overrides_unset() -> TestResult {
        let mut map = NameMap::from_types(&[]);
        map.register("Money", "Money", vec!["Api".into(), "Money".into()]);
        let e = map.lookup("Money").or_fail_with("Money entry")?;
        check!(e.exposed_overrides.is_none())
            .satisfies(is_true())
            .context("register() must not set exposed_overrides — it's reserved for the with_exposed variant")?;
        Ok(())
    }

    #[test]
    fn register_with_exposed_overwrites_prior_register_entry() -> TestResult {
        let mut map = NameMap::from_types(&[]);
        map.register("Patch", "Patch", vec!["Old".into()]);
        map.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "Patch".into()],
            vec!["Patch".into(), "patch".into()],
        );
        let e = map.lookup("Patch").or_fail_with("Patch entry")?;
        check!(module_path_strs(&e.module_path).as_slice())
            .satisfies(eq(["Api", "Patch"].as_slice()))?;
        check!(e.exposed_overrides.is_some()).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn register_overwrites_prior_register_with_exposed_entry() -> TestResult {
        let mut map = NameMap::from_types(&[]);
        map.register_with_exposed(
            "Patch",
            "Patch",
            vec!["Api".into(), "Patch".into()],
            vec!["Patch".into(), "patch".into()],
        );
        map.register("Patch", "Patch", vec!["Other".into()]);
        let e = map.lookup("Patch").or_fail_with("Patch entry")?;
        check!(module_path_strs(&e.module_path).as_slice()).satisfies(eq(["Other"].as_slice()))?;
        check!(e.exposed_overrides.is_none())
            .satisfies(is_true())
            .context("plain register() should clear any prior exposed_overrides")?;
        Ok(())
    }
}
