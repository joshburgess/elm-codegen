//! Smoke tests for the `#[elm_endpoint]` attribute macro: verify the
//! expansion compiles, the original handler is preserved, and the
//! endpoint shows up in the inventory registry with the expected
//! shape.

#![cfg(all(
    feature = "derive",
    any(feature = "axum-0-6", feature = "axum-0-7", feature = "axum-0-8"),
))]

#[cfg(feature = "axum-0-6")]
use axum_06 as axum;
#[cfg(feature = "axum-0-7")]
use axum_07 as axum;
#[cfg(feature = "axum-0-8")]
use axum_08 as axum;

use axum::extract::{Path, Query, State};
use axum::Json;
use elm_client_gen_core::ElmType;
use elm_client_gen_http::{
    elm_endpoint, registered_endpoints, BodyKind, ElmTypeRepr, ExtractorInfo, HttpMethod,
};
use test_better::ErrorKind;
use test_better::prelude::*;

fn fail(msg: impl Into<String>) -> TestError {
    TestError::new(ErrorKind::Assertion).with_message(msg.into())
}

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
fn original_handler_remains_callable() -> TestResult {
    // Just checks that the macro expansion preserved the function
    // item itself; if it didn't, this wouldn't compile.
    let _: fn(_, _, _) -> _ = get_person;
    let _: fn(_, _) -> _ = create_person;
    let _: fn(_, _, _) -> _ = patch_person;
    Ok(())
}

#[test]
fn get_person_is_registered_with_expected_metadata() -> TestResult {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "get_person")
        .ok_or_else(|| fail("get_person not registered"))?;

    check!(endpoint.elm_function_name).satisfies(eq("getPerson"))?;
    check!(endpoint.elm_module_path).satisfies(eq(["Api", "Generated", "Person"].as_slice()))?;
    check!(endpoint.method).satisfies(eq(HttpMethod::Get))?;
    check!(endpoint.path_template).satisfies(eq("/api/v1/persons/{person_id}"))?;
    check!(endpoint.tags).satisfies(eq(["read", "person"].as_slice()))?;

    // Three params in source order: State (Skip), Path, Query.
    check!(endpoint.params.len()).satisfies(eq(3))?;
    let p0 = endpoint.params.first().ok_or_else(|| fail("param 0"))?;
    let p1 = endpoint.params.get(1).ok_or_else(|| fail("param 1"))?;
    let p2 = endpoint.params.get(2).ok_or_else(|| fail("param 2"))?;
    check!(matches!(p0, ExtractorInfo::Skip)).satisfies(is_true())?;
    match p1 {
        ExtractorInfo::PathParams(p) => {
            check!(p.len()).satisfies(eq(1))?;
            check!(p.first().ok_or_else(|| fail("first path param"))?.ty.clone()).satisfies(eq(ElmTypeRepr::String))?;
        }
        other => return Err(fail(format!("expected PathParams at index 1, got {:?}", other))),
    }
    match p2 {
        ExtractorInfo::QueryParams(q) => {
            let names: Vec<_> = q.iter().map(|p| p.name).collect();
            check!(names.clone()).satisfies(contains(eq("name")))?;
            check!(names).satisfies(contains(eq("active")))?;
        }
        other => return Err(fail(format!("expected QueryParams at index 2, got {:?}", other))),
    }

    match &endpoint.response.success {
        Some(ElmTypeRepr::Custom(name)) => check!(name.as_str()).satisfies(eq("Person"))?,
        other => return Err(fail(format!("expected Some(Custom(\"Person\")), got {:?}", other))),
    }
    Ok(())
}

#[test]
fn patch_person_registers_with_patch_method_and_body() -> TestResult {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "patch_person")
        .ok_or_else(|| fail("patch_person not registered"))?;

    check!(endpoint.elm_function_name).satisfies(eq("patchPerson"))?;
    check!(endpoint.method).satisfies(eq(HttpMethod::Patch))?;
    check!(endpoint.path_template).satisfies(eq("/api/v1/persons/{person_id}"))?;

    // params[0] State (Skip), params[1] Path, params[2] Json body.
    check!(endpoint.params.len()).satisfies(eq(3))?;
    let p0 = endpoint.params.first().ok_or_else(|| fail("param 0"))?;
    let p1 = endpoint.params.get(1).ok_or_else(|| fail("param 1"))?;
    let p2 = endpoint.params.get(2).ok_or_else(|| fail("param 2"))?;
    check!(matches!(p0, ExtractorInfo::Skip)).satisfies(is_true())?;
    match p1 {
        ExtractorInfo::PathParams(p) => {
            check!(p.len()).satisfies(eq(1))?;
            check!(p.first().ok_or_else(|| fail("first path param"))?.ty.clone()).satisfies(eq(ElmTypeRepr::String))?;
        }
        other => return Err(fail(format!("expected PathParams at index 1, got {:?}", other))),
    }
    match p2 {
        ExtractorInfo::Body {
            kind: BodyKind::Json,
            ty: ElmTypeRepr::Custom(name),
        } => {
            check!(name.as_str()).satisfies(eq("CreatePerson"))?;
        }
        other => return Err(fail(format!("expected Body {{ Json, .. }}, got {:?}", other))),
    }

    match &endpoint.response.success {
        Some(ElmTypeRepr::Custom(name)) => check!(name.as_str()).satisfies(eq("Person"))?,
        other => return Err(fail(format!("expected Some(Custom(\"Person\")), got {:?}", other))),
    }
    Ok(())
}

#[test]
fn create_person_uses_default_name_and_no_tags() -> TestResult {
    let endpoint = registered_endpoints()
        .into_iter()
        .find(|e| e.handler_name == "create_person")
        .ok_or_else(|| fail("create_person not registered"))?;

    check!(endpoint.elm_function_name).satisfies(eq("createPerson"))?;
    check!(endpoint.method).satisfies(eq(HttpMethod::Post))?;
    check!(endpoint.tags.is_empty()).satisfies(is_true())?;

    // params[1] is Json<CreatePerson> -> Body
    let p1 = endpoint.params.get(1).ok_or_else(|| fail("param 1"))?;
    match p1 {
        ExtractorInfo::Body {
            kind: BodyKind::Json,
            ty: ElmTypeRepr::Custom(name),
        } => {
            check!(name.as_str()).satisfies(eq("CreatePerson"))?;
        }
        other => return Err(fail(format!("expected Body {{ Json, .. }}, got {:?}", other))),
    }
    Ok(())
}
