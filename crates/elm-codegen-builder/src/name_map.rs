use std::collections::HashMap;

use elm_codegen_core::ElmTypeInfo;

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
