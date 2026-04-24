//! Property-based invariants for the builder's transformation
//! passes. These complement the example-based unit tests by
//! generating random inputs and asserting laws the code must hold
//! for every well-formed input.

use elm_client_gen_builder::{group_by_module, TypeOverrides};
use elm_client_gen_core::{
    ElmFieldInfo, ElmTypeInfo, ElmTypeKind, ElmTypeRepr, ElmVariantInfo, ElmVariantPayload,
    EnumRepresentation,
};
use proptest::prelude::*;

// ── Repr generators ─────────────────────────────────────────────────

fn custom_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("BigDecimal".to_string()),
        Just("Untouched".to_string()),
        Just("UserId".to_string()),
        Just("Money".to_string()),
    ]
}

fn repr_strategy() -> impl Strategy<Value = ElmTypeRepr> {
    let leaf = prop_oneof![
        Just(ElmTypeRepr::String),
        Just(ElmTypeRepr::Int),
        Just(ElmTypeRepr::Float),
        Just(ElmTypeRepr::Bool),
        Just(ElmTypeRepr::Posix),
        Just(ElmTypeRepr::IsoDate),
        custom_name().prop_map(ElmTypeRepr::Custom),
    ];
    leaf.prop_recursive(4, 16, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(|r| ElmTypeRepr::Maybe(Box::new(r))),
            inner.clone().prop_map(|r| ElmTypeRepr::List(Box::new(r))),
            inner.clone().prop_map(|r| ElmTypeRepr::Dict(Box::new(r))),
            prop::collection::vec(inner, 2..4).prop_map(ElmTypeRepr::Tuple),
        ]
    })
}

// ── TypeOverrides idempotency ───────────────────────────────────────

fn wrap_in_record(repr: ElmTypeRepr) -> ElmTypeInfo {
    ElmTypeInfo {
        rust_name: "FuzzWrap",
        module_path: vec!["Fuzz"],
        type_name: "FuzzWrap",
        tags: vec![],
        kind: ElmTypeKind::Record {
            fields: vec![ElmFieldInfo {
                rust_name: "value",
                elm_name: "value",
                elm_type: repr,
                is_optional: false,
                custom_decoder: None,
                custom_encoder: None,
            }],
        },
    }
}

fn wrap_in_enum_newtype(repr: ElmTypeRepr) -> ElmTypeInfo {
    ElmTypeInfo {
        rust_name: "FuzzEnum",
        module_path: vec!["Fuzz"],
        type_name: "FuzzEnum",
        tags: vec![],
        kind: ElmTypeKind::Enum {
            representation: EnumRepresentation::Untagged,
            variants: vec![ElmVariantInfo {
                rust_name: "Only",
                elm_name: "Only",
                json_tag: "Only",
                payload: ElmVariantPayload::Newtype(repr),
            }],
        },
    }
}

fn overrides_fixture() -> TypeOverrides {
    let mut o = TypeOverrides::new();
    o.alias("BigDecimal", ElmTypeRepr::String);
    o.alias("Money", ElmTypeRepr::Float);
    o
}

proptest! {
    #[test]
    fn type_overrides_apply_is_idempotent_on_record(repr in repr_strategy()) {
        let o = overrides_fixture();
        let info = wrap_in_record(repr);
        let once = o.apply(info);
        let twice = o.apply(once.clone());
        prop_assert_eq!(format!("{:?}", once.kind), format!("{:?}", twice.kind));
    }

    #[test]
    fn type_overrides_apply_is_idempotent_on_enum_newtype(repr in repr_strategy()) {
        let o = overrides_fixture();
        let info = wrap_in_enum_newtype(repr);
        let once = o.apply(info);
        let twice = o.apply(once.clone());
        prop_assert_eq!(format!("{:?}", once.kind), format!("{:?}", twice.kind));
    }
}

// ── group_by_module partition law ───────────────────────────────────

fn module_path_strategy() -> impl Strategy<Value = Vec<&'static str>> {
    // Pick from a small fixed pool of &'static str so we actually get
    // repeats and can exercise the grouping logic.
    prop_oneof![
        Just(vec!["Api", "Person"]),
        Just(vec!["Api", "Order"]),
        Just(vec!["Api", "Person"]),
        Just(vec!["Domain"]),
        Just(vec!["Domain", "Inventory"]),
    ]
}

fn typeinfo_strategy() -> impl Strategy<Value = ElmTypeInfo> {
    module_path_strategy().prop_map(|module_path| ElmTypeInfo {
        rust_name: "Fuzz",
        module_path,
        type_name: "Fuzz",
        tags: vec![],
        kind: ElmTypeKind::Record { fields: vec![] },
    })
}

proptest! {
    #[test]
    fn group_by_module_preserves_count_and_partitions_by_path(
        types in prop::collection::vec(typeinfo_strategy(), 0..20),
    ) {
        let groups = group_by_module(&types);
        let total: usize = groups.values().map(|v| v.len()).sum();
        prop_assert_eq!(total, types.len());
        for (key, members) in &groups {
            for m in members {
                let member_path: Vec<&str> = m.module_path.to_vec();
                prop_assert_eq!(&member_path, key);
            }
        }
    }
}
