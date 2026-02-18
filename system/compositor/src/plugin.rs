//! BYO compositor plugin — registers resources, events, and systems.

use bevy::prelude::*;

use crate::commands;
use crate::id_map::IdMap;
use crate::io::{self, ByoBatch};
use crate::style;

/// Conversion factor from protocol pixel coordinates to 3D world units (meters).
/// Computed from the assumed logical PPI: `world_scale = 1.0 / (ppi * 39.3701)`.
#[derive(Resource)]
pub struct WorldScale(pub f32);

pub struct ByoPlugin;

impl Plugin for ByoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IdMap>()
            .add_message::<ByoBatch>()
            .add_systems(Startup, io::setup_io)
            .add_systems(
                PreUpdate,
                (io::drain_commands, commands::process_commands).chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    style::reconcile_views,
                    style::reconcile_view_transforms,
                    style::reconcile_text,
                    style::reconcile_windows,
                    style::reconcile_layers,
                    style::reorder_children,
                ),
            );
    }
}
