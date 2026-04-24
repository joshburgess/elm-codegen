use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Value")]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Count(i64),
}

fn main() {
    let _ = <Value as ElmType>::elm_type_info();
}
