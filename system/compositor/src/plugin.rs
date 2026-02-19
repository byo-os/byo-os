//! BYO compositor plugin — registers resources, events, and systems.

use bevy::prelude::*;

use crate::commands;
use crate::events;
use crate::id_map::IdMap;
use crate::io::{self, ByoBatch};
use crate::style;
use crate::transition;

/// Conversion factor from protocol pixel coordinates to 3D world units (meters).
/// Computed from the assumed logical PPI: `world_scale = 1.0 / (ppi * 39.3701)`.
#[derive(Resource)]
pub struct WorldScale(pub f32);

pub struct ByoPlugin;

impl Plugin for ByoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IdMap>()
            .init_resource::<events::observers::PointerEnterState>()
            .add_message::<ByoBatch>()
            .add_systems(Startup, (io::setup_io, setup_engine).chain())
            .add_systems(
                PreUpdate,
                (io::drain_commands, commands::process_commands).chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    transition::systems::handle_view_transitions,
                    transition::systems::handle_window_transitions,
                    transition::systems::handle_layer_transitions,
                    style::reconcile_views,
                    style::reconcile_view_transforms,
                    style::reconcile_text,
                    style::reconcile_windows,
                    style::reconcile_layers,
                    style::reconcile_view_picking,
                    style::reconcile_text_picking,
                    style::reconcile_layer_picking,
                    style::reconcile_window_picking,
                    style::reorder_children,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    transition::systems::tick_view_transitions,
                    transition::systems::tick_window_transitions,
                    transition::systems::tick_layer_transitions,
                    events::observers::handle_cursor_left,
                ),
            );

        // Register global pointer event observers
        events::observers::register_observers(app);
    }
}

/// Startup system: spawn the propagation engine thread.
fn setup_engine(mut commands: Commands, emitter: Res<io::StdoutEmitter>) {
    let handle = events::engine::spawn_engine(emitter.clone());
    commands.insert_resource(handle);
}
