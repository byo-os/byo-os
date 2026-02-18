//! LayerProps — persistent Bevy component for BYO `layer` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;

/// Persistent component storing all BYO props for a `layer` entity.
/// Layers own a render texture + Camera2d for 2D UI compositing,
/// projected onto a 3D plane within their parent window.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct LayerProps {
    pub width: Option<ByoVal>,
    pub height: Option<ByoVal>,
    pub order: Option<i32>,
}
