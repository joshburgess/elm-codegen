use std::collections::HashMap;

use elm_client_gen_core::{ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantPayload};

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

    /// Apply the rewrites to every referenced type in an `ElmTypeInfo`.
    ///
    /// Record struct fields, newtype-kind inner reprs, and enum variant
    /// payloads (both newtype and struct variants) all have their
    /// `ElmTypeRepr`s rewritten, recursing through `Maybe`/`List`/
    /// `Dict`/`Tuple` so aliases nested inside compound types are
    /// substituted too.
    pub fn apply(&self, mut info: ElmTypeInfo) -> ElmTypeInfo {
        match &mut info.kind {
            ElmTypeKind::Record { fields } => {
                for field in fields {
                    field.elm_type = self.rewrite(&field.elm_type);
                }
            }
            ElmTypeKind::Newtype { inner } => {
                *inner = self.rewrite(inner);
            }
            ElmTypeKind::Enum { variants, .. } => {
                for variant in variants {
                    match &mut variant.payload {
                        ElmVariantPayload::Unit => {}
                        ElmVariantPayload::Newtype(inner) => {
                            *inner = self.rewrite(inner);
                        }
                        ElmVariantPayload::Struct(fields) => {
                            for field in fields {
                                field.elm_type = self.rewrite(&field.elm_type);
                            }
                        }
                    }
                }
            }
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
            ElmTypeRepr::Dict(inner) => ElmTypeRepr::Dict(Box::new(self.rewrite(inner))),
            ElmTypeRepr::Tuple(elems) => {
                ElmTypeRepr::Tuple(elems.iter().map(|e| self.rewrite(e)).collect())
            }
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elm_client_gen_core::{
        ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
        EnumRepresentation,
    };

    fn overrides() -> TypeOverrides {
        let mut o = TypeOverrides::new();
        o.alias("BigDecimal", ElmTypeRepr::String);
        o
    }

    fn custom(name: &str) -> ElmTypeRepr {
        ElmTypeRepr::Custom(name.to_string())
    }

    #[test]
    fn rewrites_bare_custom() {
        let o = overrides();
        assert!(matches!(
            o.rewrite(&custom("BigDecimal")),
            ElmTypeRepr::String
        ));
    }

    #[test]
    fn leaves_unknown_custom_untouched() {
        let o = overrides();
        match o.rewrite(&custom("UserId")) {
            ElmTypeRepr::Custom(name) => assert_eq!(name, "UserId"),
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn recurses_through_maybe_list_dict() {
        let o = overrides();
        let nested = ElmTypeRepr::Maybe(Box::new(ElmTypeRepr::List(Box::new(ElmTypeRepr::Dict(
            Box::new(custom("BigDecimal")),
        )))));
        let rewritten = o.rewrite(&nested);
        match rewritten {
            ElmTypeRepr::Maybe(inner) => match *inner {
                ElmTypeRepr::List(inner) => match *inner {
                    ElmTypeRepr::Dict(inner) => assert!(matches!(*inner, ElmTypeRepr::String)),
                    other => panic!("expected Dict, got {other:?}"),
                },
                other => panic!("expected List, got {other:?}"),
            },
            other => panic!("expected Maybe, got {other:?}"),
        }
    }

    #[test]
    fn recurses_through_tuple_elements() {
        let o = overrides();
        let t = ElmTypeRepr::Tuple(vec![
            custom("BigDecimal"),
            ElmTypeRepr::Int,
            custom("Unknown"),
        ]);
        match o.rewrite(&t) {
            ElmTypeRepr::Tuple(elems) => {
                assert!(matches!(elems[0], ElmTypeRepr::String));
                assert!(matches!(elems[1], ElmTypeRepr::Int));
                match &elems[2] {
                    ElmTypeRepr::Custom(name) => assert_eq!(name, "Unknown"),
                    other => panic!("expected Custom(Unknown), got {other:?}"),
                }
            }
            other => panic!("expected Tuple, got {other:?}"),
        }
    }

    #[test]
    fn apply_is_idempotent_on_records() {
        let o = overrides();
        let info = ElmTypeInfo {
            rust_name: "Order",
            module_path: vec!["Api", "Order"],
            type_name: "Order",
            tags: vec![],
            kind: ElmTypeKind::Record {
                fields: vec![ElmFieldInfo {
                    rust_name: "total",
                    elm_name: "total",
                    elm_type: custom("BigDecimal"),
                    is_optional: false,
                    custom_decoder: None,
                    custom_encoder: None,
                }],
            },
        };
        let once = o.apply(info.clone());
        let twice = o.apply(once.clone());
        assert_eq!(format!("{:?}", once.kind), format!("{:?}", twice.kind));
    }

    #[test]
    fn apply_rewrites_newtype_inner() {
        let o = overrides();
        let info = ElmTypeInfo {
            rust_name: "Money",
            module_path: vec!["Api"],
            type_name: "Money",
            tags: vec![],
            kind: ElmTypeKind::Newtype {
                inner: custom("BigDecimal"),
            },
        };
        match o.apply(info).kind {
            ElmTypeKind::Newtype { inner } => assert!(matches!(inner, ElmTypeRepr::String)),
            other => panic!("expected Newtype, got {other:?}"),
        }
    }

    #[test]
    fn apply_rewrites_enum_variant_payloads() {
        let o = overrides();
        let info = ElmTypeInfo {
            rust_name: "Event",
            module_path: vec!["Api"],
            type_name: "Event",
            tags: vec![],
            kind: ElmTypeKind::Enum {
                representation: EnumRepresentation::Untagged,
                variants: vec![
                    ElmVariantInfo {
                        rust_name: "Amount",
                        elm_name: "Amount",
                        json_tag: "Amount",
                        payload: ElmVariantPayload::Newtype(custom("BigDecimal")),
                    },
                    ElmVariantInfo {
                        rust_name: "Line",
                        elm_name: "Line",
                        json_tag: "Line",
                        payload: ElmVariantPayload::Struct(vec![ElmFieldInfo {
                            rust_name: "price",
                            elm_name: "price",
                            elm_type: ElmTypeRepr::Maybe(Box::new(custom("BigDecimal"))),
                            is_optional: true,
                            custom_decoder: None,
                            custom_encoder: None,
                        }]),
                    },
                    ElmVariantInfo {
                        rust_name: "None",
                        elm_name: "None",
                        json_tag: "None",
                        payload: ElmVariantPayload::Unit,
                    },
                ],
            },
        };
        let out = o.apply(info);
        let ElmTypeKind::Enum { variants, .. } = out.kind else {
            panic!("expected Enum");
        };
        match &variants[0].payload {
            ElmVariantPayload::Newtype(repr) => assert!(matches!(repr, ElmTypeRepr::String)),
            other => panic!("expected Newtype(String), got {other:?}"),
        }
        match &variants[1].payload {
            ElmVariantPayload::Struct(fields) => match &fields[0].elm_type {
                ElmTypeRepr::Maybe(inner) => assert!(matches!(**inner, ElmTypeRepr::String)),
                other => panic!("expected Maybe(String), got {other:?}"),
            },
            other => panic!("expected Struct, got {other:?}"),
        }
        assert!(matches!(variants[2].payload, ElmVariantPayload::Unit));
    }
}
