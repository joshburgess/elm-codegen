use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Shape")]
#[serde(untagged)]
pub enum Shape {
    Point(i32, i32),
}

fn main() {}
