//! WindowProps — persistent Bevy component for BYO `window` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;
use crate::transition::config::{EaseFn, TransitionProperty};

/// Persistent component storing all BYO props for a `window` entity.
/// Windows are 3D transform roots that contain layers.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct WindowProps {
    pub class: Option<String>,
    pub width: Option<ByoVal>,
    pub height: Option<ByoVal>,
    pub title: Option<String>,
    pub order: Option<i32>,
    pub order_mode: Option<ByoOrderMode>,
    pub order_scale: Option<f32>,
    // 2D transforms (shared with views)
    pub translate_x: Option<ByoVal>,
    pub translate_y: Option<ByoVal>,
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
    // Input events
    pub events: Option<String>,
    pub pointer_events: Option<ByoPointerEvents>,
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
