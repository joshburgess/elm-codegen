use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Ids")]
pub struct UserId(pub String);

fn main() {
    let _ = <UserId as ElmType>::elm_type_info();
}
