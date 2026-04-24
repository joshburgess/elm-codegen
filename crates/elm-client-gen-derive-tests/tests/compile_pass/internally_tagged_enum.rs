use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Inner")]
pub struct Inner {
    pub a: i32,
}

#[derive(ElmType)]
#[elm(module = "Api.Event")]
#[serde(tag = "type")]
pub enum Event {
    Ping,
    Tick { at: String },
    Wrapped(Inner),
}

fn main() {
    let _ = <Event as ElmType>::elm_type_info();
}
