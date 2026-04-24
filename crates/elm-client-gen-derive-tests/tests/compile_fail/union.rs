use elm_client_gen_core::ElmType;

#[derive(ElmType)]
#[elm(module = "Api.Union")]
pub union Bits {
    as_u32: u32,
    as_bytes: [u8; 4],
}

fn main() {}
