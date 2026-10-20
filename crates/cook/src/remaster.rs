//! Placeholder hook for the future "remaster" profile: texture upscaling and PBR derivation
//! (roughness/metalness/AO from the diffuse + specular + normal maps). Called by the texture
//! cooker with the source image and the cooked payload when `Profile::remaster_hook` is set.
//! Currently a no-op so the remaster profile cooks identically to `desktop-high`; replace the
//! body to substitute upscaled or derived data before it is written.
use gothic_formats::ximg::Ximg;

pub fn on_texture(_source: &Ximg<'_>, _payload: &mut Vec<u8>) {}
