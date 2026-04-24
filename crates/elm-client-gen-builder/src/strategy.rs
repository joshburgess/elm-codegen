use elm_client_gen_core::ElmTypeInfo;

/// Per-type policy hooks for module generation.
///
/// Implementations decide whether each type gets a decoder, an encoder,
/// or both. Defaults emit both for everything; override per-tag rules
/// (e.g. skip encoders for read-only filter types) by providing your
/// own implementation.
pub trait BuildStrategy {
    fn should_emit_decoder(&self, _info: &ElmTypeInfo) -> bool {
        true
    }
    fn should_emit_encoder(&self, _info: &ElmTypeInfo) -> bool {
        true
    }
}

/// Emits both decoder and encoder for every type.
pub struct DefaultStrategy;

impl BuildStrategy for DefaultStrategy {}
