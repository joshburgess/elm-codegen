use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Pair")]
pub struct Pair(String, i32);

fn main() {}
