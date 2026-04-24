use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Page")]
pub struct Page<T> {
    pub items: Vec<T>,
}

fn main() {}
