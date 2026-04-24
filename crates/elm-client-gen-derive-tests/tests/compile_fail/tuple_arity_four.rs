use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Quad")]
pub struct Quad {
    pub payload: (i32, i32, i32, i32),
}

fn main() {}
