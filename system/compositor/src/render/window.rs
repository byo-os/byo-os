//! Window render management — 3D transform entity for window compositing.

use bevy::prelude::*;

/// Tracks the default layer for a window.
#[derive(Component)]
#[allow(dead_code)]
pub struct WindowRender {
    /// The default layer entity for this window (auto-created).
    pub default_layer: Option<Entity>,
}
