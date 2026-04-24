use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Bad")]
#[serde(tag = "kind")]
#[serde(untagged)]
pub enum Bad {
    One,
    Two,
}

fn main() {}
