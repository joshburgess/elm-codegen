use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Person")]
pub struct Person {
    pub id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub friends: Vec<String>,
}

fn main() {
    let _ = <Person as ElmType>::elm_type_info();
}
