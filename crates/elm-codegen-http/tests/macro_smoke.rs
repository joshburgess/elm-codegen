//! Smoke tests for the `#[elm_endpoint]` attribute macro: verify the
//! expansion compiles, the original handler is preserved, and the
//! endpoint shows up in the inventory registry with the expected
//! shape.

#![cfg(all(feature = "derive", feature = "axum"))]

use axum::extract::{Path, Query, State};
use axum::Json;
use elm_codegen_core::ElmType;
use elm_codegen_http::{
    elm_endpoint, registered_endpoints, BodyKind, ElmTypeRepr, ExtractorInfo, HttpMethod,
};

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "Person")]
#[allow(dead_code)]
struct Person {
    id: String,
    name: String,
}

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "PersonFilters")]
#[allow(dead_code)]
struct PersonFilters {
    name: Option<String>,
    active: bool,
}

#[derive(ElmType)]
#[elm(module = "Api.Person", name = "CreatePerson")]
#[allow(dead_code)]
struct CreatePerson {
    name: String,
}

struct AppCtx;

#[elm_endpoint(
    GET,
    "/api/v1/persons/{person_id}",
    module = "Api.Generated.Person",
    name = "getPerson",
    tags = "read,person"
)]
#[allow(dead_code, unused_variables)]
async fn get_person(
    State(_ctx): State<AppCtx>,
    Path(person_id): Path<String>,
    Query(filters): Query<PersonFilters>,
) -> Result<Json<Person>, ()> {
    unreachable!()
}

#[elm_endpoint(POST, "/api/v1/persons", module = "Api.Generated.Person")]
#[allow(dead_code, unused_variables)]
async fn create_person(
    State(_ctx): State<AppCtx>,
    Json(body): Json<CreatePerson>,
) -> Result<Json<Person>, ()> {
    unreachable!()
}

#[elm_endpoint(PATCH, "/api/v1/persons/{person_id}", module = "Api.Generated.Person")]
#[allow(dead_code, unused_variables)]
async fn patch_person(
    State(_ctx): State<AppCtx>,
    Path(person_id): Path<String>,
    Json(body): Json<CreatePerson>,
) -> Result<Json<Person>, ()> {
    unreachable!()
}

#[test]
fn original_handler_remains_callable() {
    // Just checks that the macro expansion preserved the function
    // item itself; if it didn't, this wouldn't compile.
    let _: fn(_, _, _) -> _ = get_person;
    let _: fn(_, _) -> _ = create_person;
    let _: fn(_, _, _) -> _ = patch_person;
}

#[test]
fn get_person_is_registered_with_expected_metadata() {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "get_person")
        .expect("get_person not registered");

    assert_eq!(endpoint.elm_function_name, "getPerson");
    assert_eq!(endpoint.elm_module_path, &["Api", "Generated", "Person"]);
    assert_eq!(endpoint.method, HttpMethod::Get);
    assert_eq!(endpoint.path_template, "/api/v1/persons/{person_id}");
    assert_eq!(endpoint.tags, &["read", "person"]);

    // Three params in source order: State (Skip), Path, Query.
    assert_eq!(endpoint.params.len(), 3);
    let p0 = endpoint.params.first().expect("param 0");
    let p1 = endpoint.params.get(1).expect("param 1");
    let p2 = endpoint.params.get(2).expect("param 2");
    assert!(matches!(p0, ExtractorInfo::Skip));
    match p1 {
        ExtractorInfo::PathParams(p) => {
            assert_eq!(p.len(), 1);
            assert_eq!(p.first().expect("first path param").ty, ElmTypeRepr::String);
        }
        other => panic!("expected PathParams at index 1, got {:?}", other),
    }
    match p2 {
        ExtractorInfo::QueryParams(q) => {
            let names: Vec<_> = q.iter().map(|p| p.name).collect();
            assert!(names.contains(&"name"));
            assert!(names.contains(&"active"));
        }
        other => panic!("expected QueryParams at index 2, got {:?}", other),
    }

    match &endpoint.response.success {
        Some(ElmTypeRepr::Custom(name)) => assert_eq!(name, "Person"),
        other => panic!("expected Some(Custom(\"Person\")), got {:?}", other),
    }
}

#[test]
fn patch_person_registers_with_patch_method_and_body() {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "patch_person")
        .expect("patch_person not registered");

    assert_eq!(endpoint.elm_function_name, "patchPerson");
    assert_eq!(endpoint.method, HttpMethod::Patch);
    assert_eq!(endpoint.path_template, "/api/v1/persons/{person_id}");

    // params[0] State (Skip), params[1] Path, params[2] Json body.
    assert_eq!(endpoint.params.len(), 3);
    let p0 = endpoint.params.first().expect("param 0");
    let p1 = endpoint.params.get(1).expect("param 1");
    let p2 = endpoint.params.get(2).expect("param 2");
    assert!(matches!(p0, ExtractorInfo::Skip));
    match p1 {
        ExtractorInfo::PathParams(p) => {
            assert_eq!(p.len(), 1);
            assert_eq!(p.first().expect("first path param").ty, ElmTypeRepr::String);
        }
        other => panic!("expected PathParams at index 1, got {:?}", other),
    }
    match p2 {
        ExtractorInfo::Body {
            kind: BodyKind::Json,
            ty: ElmTypeRepr::Custom(name),
        } => {
            assert_eq!(name, "CreatePerson");
        }
        other => panic!("expected Body {{ Json, .. }}, got {:?}", other),
    }

    match &endpoint.response.success {
        Some(ElmTypeRepr::Custom(name)) => assert_eq!(name, "Person"),
        other => panic!("expected Some(Custom(\"Person\")), got {:?}", other),
    }
}

#[test]
fn create_person_uses_default_name_and_no_tags() {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "create_person")
        .expect("create_person not registered");

    assert_eq!(endpoint.elm_function_name, "createPerson");
    assert_eq!(endpoint.method, HttpMethod::Post);
    assert!(endpoint.tags.is_empty());

    // params[1] is Json<CreatePerson> -> Body
    let p1 = endpoint.params.get(1).expect("param 1");
    match p1 {
        ExtractorInfo::Body {
            kind: BodyKind::Json,
            ty: ElmTypeRepr::Custom(name),
        } => {
            assert_eq!(name, "CreatePerson");
        }
        other => panic!("expected Body {{ Json, .. }}, got {:?}", other),
    }
}
