use std::collections::HashMap;

use elm_codegen_core::{ElmTypeInfo, ElmTypeRepr};

/// Codebase-wide rewrites applied to `ElmTypeRepr::Custom(_)` references.
///
/// Use this to map project-specific Rust types to Elm primitives without
/// per-field annotations. For example, if `bigdecimal::BigDecimal`
/// serializes as a JSON string in your API:
///
/// ```ignore
/// let mut overrides = TypeOverrides::new();
/// overrides.alias("BigDecimal", ElmTypeRepr::String);
/// let types: Vec<_> = registered_types().into_iter()
///     .map(|t| overrides.apply(t))
///     .collect();
/// ```
#[derive(Default, Clone)]
pub struct TypeOverrides {
    aliases: HashMap<String, ElmTypeRepr>,
}

impl TypeOverrides {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an override mapping for a given Rust type name.
    ///
    /// Per-field `#[elm(type = "...")]` overrides take precedence over
    /// these, since they are applied at derive time, before this layer ever
    /// sees the `ElmTypeInfo`.
    pub fn alias(&mut self, rust_name: impl Into<String>, repr: ElmTypeRepr) -> &mut Self {
        self.aliases.insert(rust_name.into(), repr);
        self
    }

    /// Apply the rewrites to every field of an `ElmTypeInfo`.
    pub fn apply(&self, mut info: ElmTypeInfo) -> ElmTypeInfo {
        for field in &mut info.fields {
            field.elm_type = self.rewrite(&field.elm_type);
        }
        info
    }

    fn rewrite(&self, repr: &ElmTypeRepr) -> ElmTypeRepr {
        match repr {
            ElmTypeRepr::Custom(name) => self
                .aliases
                .get(name)
                .cloned()
                .unwrap_or_else(|| repr.clone()),
            ElmTypeRepr::Maybe(inner) => ElmTypeRepr::Maybe(Box::new(self.rewrite(inner))),
            ElmTypeRepr::List(inner) => ElmTypeRepr::List(Box::new(self.rewrite(inner))),
            other => other.clone(),
        }
    }
}

