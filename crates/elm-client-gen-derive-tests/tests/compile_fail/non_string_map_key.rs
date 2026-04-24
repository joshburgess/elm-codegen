use elm_client_gen_core::ElmType;
use std::collections::HashMap;

#[derive(ElmType)]
#[elm(module = "Api.IntMap")]
pub struct IntMap {
    pub by_id: HashMap<i64, String>,
}

fn main() {}
