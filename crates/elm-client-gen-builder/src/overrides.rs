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
            // Recurse into the args of a type application; the head is
            // a wrapper name handled separately by the consumer's
            // NameMap so we don't apply alias rewrites to it.
            ElmTypeRepr::App { head, args } => ElmTypeRepr::App {
                head: head.clone(),
                args: args.iter().map(|a| self.rewrite(a)).collect(),
            },
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
    use test_better::prelude::*;
    use test_better::ErrorKind;

    fn overrides() -> TypeOverrides {
        let mut o = TypeOverrides::new();
        o.alias("BigDecimal", ElmTypeRepr::String);
        o
    }

    fn custom(name: &str) -> ElmTypeRepr {
        ElmTypeRepr::Custom(name.to_string())
    }

    #[test]
    fn rewrites_bare_custom() -> TestResult {
        let o = overrides();
        check!(matches!(
            o.rewrite(&custom("BigDecimal")),
            ElmTypeRepr::String
        ))
        .satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn leaves_unknown_custom_untouched() -> TestResult {
        let o = overrides();
        let ElmTypeRepr::Custom(name) = o.rewrite(&custom("UserId")) else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Custom"));
        };
        check!(name.as_str()).satisfies(eq("UserId"))?;
        Ok(())
    }

    #[test]
    fn recurses_through_maybe_list_dict() -> TestResult {
        let o = overrides();
        let nested = ElmTypeRepr::Maybe(Box::new(ElmTypeRepr::List(Box::new(ElmTypeRepr::Dict(
            Box::new(custom("BigDecimal")),
        )))));
        let ElmTypeRepr::Maybe(inner) = o.rewrite(&nested) else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Maybe"));
        };
        let ElmTypeRepr::List(inner) = *inner else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected List"));
        };
        let ElmTypeRepr::Dict(inner) = *inner else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Dict"));
        };
        check!(matches!(*inner, ElmTypeRepr::String)).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn recurses_through_tuple_elements() -> TestResult {
        let o = overrides();
        let t = ElmTypeRepr::Tuple(vec![
            custom("BigDecimal"),
            ElmTypeRepr::Int,
            custom("Unknown"),
        ]);
        let ElmTypeRepr::Tuple(elems) = o.rewrite(&t) else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Tuple"));
        };
        let first = elems.first().or_fail_with("tuple elem 0")?;
        let second = elems.get(1).or_fail_with("tuple elem 1")?;
        let third = elems.get(2).or_fail_with("tuple elem 2")?;
        check!(matches!(first, ElmTypeRepr::String)).satisfies(is_true())?;
        check!(matches!(second, ElmTypeRepr::Int)).satisfies(is_true())?;
        let ElmTypeRepr::Custom(name) = third else {
            return Err(
                TestError::new(ErrorKind::Assertion).with_message("expected Custom(Unknown)")
            );
        };
        check!(name.as_str()).satisfies(eq("Unknown"))?;
        Ok(())
    }

    #[test]
    fn recurses_into_app_args_but_leaves_head_untouched() -> TestResult {
        // The wrapper head (`Patch`) is a user-supplied module name
        // resolved via NameMap, NOT a candidate for the alias rewrite
        // table. Aliases must only fire on the args.
        let mut o = TypeOverrides::new();
        o.alias("BigDecimal", ElmTypeRepr::String);
        // Defensively register an alias that *would* rewrite the head
        // if the implementation ever forgot to skip it.
        o.alias("Patch", ElmTypeRepr::Int);

        let app = ElmTypeRepr::App {
            head: "Patch".into(),
            args: vec![custom("BigDecimal")],
        };
        let ElmTypeRepr::App { head, args } = o.rewrite(&app) else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected App"));
        };
        check!(head.as_str())
            .satisfies(eq("Patch"))
            .context("head must not be rewritten")?;
        check!(args.len()).satisfies(eq(1))?;
        let first = args.first().or_fail_with("app arg 0")?;
        check!(matches!(first, ElmTypeRepr::String)).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn recurses_into_nested_app_args() -> TestResult {
        let o = overrides();
        let nested = ElmTypeRepr::App {
            head: "Patch".into(),
            args: vec![ElmTypeRepr::Maybe(Box::new(custom("BigDecimal")))],
        };
        let ElmTypeRepr::App { args, .. } = o.rewrite(&nested) else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected App"));
        };
        let first = args.first().or_fail_with("app arg 0")?;
        let ElmTypeRepr::Maybe(inner) = first else {
            return Err(
                TestError::new(ErrorKind::Assertion).with_message("expected Maybe inside App")
            );
        };
        check!(matches!(**inner, ElmTypeRepr::String)).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn apply_recurses_into_app_args_on_record_field() -> TestResult {
        let o = overrides();
        let info = ElmTypeInfo {
            rust_name: "Profile",
            module_path: vec!["Api"],
            type_name: "Profile",
            tags: vec![],
            kind: ElmTypeKind::Record {
                fields: vec![ElmFieldInfo {
                    rust_name: "balance",
                    elm_name: "balance",
                    elm_type: ElmTypeRepr::App {
                        head: "Patch".into(),
                        args: vec![custom("BigDecimal")],
                    },
                    is_optional: false,
                    custom_decoder: None,
                    custom_encoder: None,
                    decoder_step: Some("patch"),
                    encoder_pairs: Some("patchPair"),
                }],
            },
        };
        let out = o.apply(info);
        let ElmTypeKind::Record { fields } = out.kind else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Record"));
        };
        let field = fields.first().or_fail_with("record field 0")?;
        let ElmTypeRepr::App { head, args } = &field.elm_type else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected App"));
        };
        check!(head.as_str()).satisfies(eq("Patch"))?;
        let first_arg = args.first().or_fail_with("app arg 0")?;
        check!(matches!(first_arg, ElmTypeRepr::String)).satisfies(is_true())?;
        // Codec hooks pass through untouched.
        check!(field.decoder_step).satisfies(eq(Some("patch")))?;
        check!(field.encoder_pairs).satisfies(eq(Some("patchPair")))?;
        Ok(())
    }

    #[test]
    fn apply_is_idempotent_on_records() -> TestResult {
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
                    decoder_step: None,
                    encoder_pairs: None,
                }],
            },
        };
        let once = o.apply(info.clone());
        let twice = o.apply(once.clone());
        check!(format!("{:?}", once.kind)).satisfies(eq(format!("{:?}", twice.kind)))?;
        Ok(())
    }

    #[test]
    fn apply_rewrites_newtype_inner() -> TestResult {
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
        let ElmTypeKind::Newtype { inner } = o.apply(info).kind else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Newtype"));
        };
        check!(matches!(inner, ElmTypeRepr::String)).satisfies(is_true())?;
        Ok(())
    }

    #[test]
    fn apply_rewrites_enum_variant_payloads() -> TestResult {
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
                            decoder_step: None,
                            encoder_pairs: None,
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
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Enum"));
        };
        let v0 = variants.first().or_fail_with("variant 0")?;
        let v1 = variants.get(1).or_fail_with("variant 1")?;
        let v2 = variants.get(2).or_fail_with("variant 2")?;
        let ElmVariantPayload::Newtype(repr) = &v0.payload else {
            return Err(
                TestError::new(ErrorKind::Assertion).with_message("expected Newtype(String)")
            );
        };
        check!(matches!(repr, ElmTypeRepr::String)).satisfies(is_true())?;
        let ElmVariantPayload::Struct(fields) = &v1.payload else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Struct"));
        };
        let f0 = fields.first().or_fail_with("struct field 0")?;
        let ElmTypeRepr::Maybe(inner) = &f0.elm_type else {
            return Err(TestError::new(ErrorKind::Assertion).with_message("expected Maybe(String)"));
        };
        check!(matches!(**inner, ElmTypeRepr::String)).satisfies(is_true())?;
        check!(matches!(v2.payload, ElmVariantPayload::Unit)).satisfies(is_true())?;
        Ok(())
    }
}
