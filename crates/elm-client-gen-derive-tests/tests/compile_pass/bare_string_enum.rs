use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Status")]
pub enum Status {
    Active,
    Archived,
    Banned,
}

fn main() {
    let _ = <Status as ElmType>::elm_type_info();
}
