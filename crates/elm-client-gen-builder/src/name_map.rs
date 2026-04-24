use std::collections::HashMap;

use elm_client_gen_core::ElmTypeInfo;

/// One entry per Elm type known to the codegen, populated from
/// `ElmTypeInfo` and any consumer-supplied externs.
#[derive(Clone)]
pub struct NameEntry {
    pub elm_name: String,
    pub module_path: Vec<String>,
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

    #[test]
    fn from_types_builds_entry_per_type() {
        let types = vec![
            record("PersonApi", "Person", vec!["Api", "Person"]),
            record("OrderApi", "Order", vec!["Api", "Order"]),
        ];
        let map = NameMap::from_types(&types);
        let p = map.lookup("PersonApi").expect("Person entry");
        assert_eq!(p.elm_name, "Person");
        assert_eq!(p.module_path, vec!["Api", "Person"]);
        assert_eq!(map.resolve("OrderApi"), "Order");
    }

    #[test]
    fn resolve_falls_back_to_input_on_miss() {
        let map = NameMap::from_types(&[]);
        assert_eq!(map.resolve("Unknown"), "Unknown");
        assert!(map.lookup("Unknown").is_none());
    }

    #[test]
    fn register_adds_hand_written_entries() {
        let mut map = NameMap::from_types(&[]);
        map.register("Money", "Money", vec!["Api".into(), "Money".into()]);
        let e = map.lookup("Money").expect("registered Money entry");
        assert_eq!(e.elm_name, "Money");
        assert_eq!(e.module_path, vec!["Api", "Money"]);
    }

    #[test]
    fn register_overwrites_existing_entry() {
        let types = vec![record("Overlap", "FirstName", vec!["Api", "First"])];
        let mut map = NameMap::from_types(&types);
        map.register("Overlap", "SecondName", vec!["Api".into(), "Second".into()]);
        let e = map.lookup("Overlap").expect("overwrite entry");
        assert_eq!(e.elm_name, "SecondName");
        assert_eq!(e.module_path, vec!["Api", "Second"]);
    }
}
