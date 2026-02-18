//! LayerProps — persistent Bevy component for BYO `layer` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;
use crate::transition::config::{EaseFn, TransitionProperty};

/// Persistent component storing all BYO props for a `layer` entity.
/// Layers own a render texture + Camera2d for 2D UI compositing,
/// projected onto a 3D plane within their parent window.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct LayerProps {
    pub class: Option<String>,
    pub format: Option<ByoTextureFormat>,
    pub width: Option<ByoVal>,
    pub height: Option<ByoVal>,
    pub order: Option<i32>,
    pub order_mode: Option<ByoOrderMode>,
    pub order_scale: Option<f32>,
    // 2D transforms (shared with views/windows)
    pub translate_x: Option<f32>,
    pub translate_y: Option<f32>,
    pub rotate: Option<f32>,
    pub scale: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    // 3D-only transforms
    pub translate_z: Option<f32>,
    pub rotate_x: Option<f32>,
    pub rotate_y: Option<f32>,
    pub rotate_z: Option<f32>,
    pub scale_z: Option<f32>,
    // PBR material props
    pub base_color: Option<ByoColor>,
    pub emissive_color: Option<ByoColor>,
    pub emissive_exposure_weight: Option<f32>,
    pub perceptual_roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub reflectance: Option<f32>,
    pub unlit: Option<bool>,
    pub alpha_mode: Option<ByoAlphaMode>,
    pub double_sided: Option<bool>,
    pub cull_mode: Option<ByoCullMode>,
    pub depth_bias: Option<f32>,
    pub fog_enabled: Option<bool>,
    // Transmission
    pub diffuse_transmission: Option<f32>,
    pub specular_transmission: Option<f32>,
    pub ior: Option<f32>,
    pub thickness: Option<f32>,
    pub attenuation_color: Option<ByoColor>,
    pub attenuation_distance: Option<f32>,
    // Clearcoat
    pub clearcoat: Option<f32>,
    pub clearcoat_perceptual_roughness: Option<f32>,
    // Anisotropy
    pub anisotropy_strength: Option<f32>,
    pub anisotropy_rotation: Option<f32>,
    // Transition
    pub transition: Option<String>,
    #[prop(skip)]
    pub tw_transition_property: Option<TransitionProperty>,
    #[prop(skip)]
    pub tw_transition_duration: Option<f32>,
    #[prop(skip)]
    pub tw_transition_easing: Option<EaseFn>,
    #[prop(skip)]
    pub tw_transition_delay: Option<f32>,
}
