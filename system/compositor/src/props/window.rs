//! WindowProps — persistent Bevy component for BYO `window` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;

/// Persistent component storing all BYO props for a `window` entity.
/// Windows are 3D transform roots that contain layers.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct WindowProps {
    pub width: Option<ByoVal>,
    pub height: Option<ByoVal>,
    pub title: Option<String>,
    pub order: Option<i32>,
}
