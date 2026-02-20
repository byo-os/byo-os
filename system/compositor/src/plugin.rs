//! BYO compositor plugin — registers resources, events, and systems.

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::commands;
use crate::events;
use crate::id_map::IdMap;
use crate::io::{self, ByoBatch};
use crate::kitty_gfx;
use crate::style;
use crate::transition;
use crate::tty;

/// Conversion factor from protocol pixel coordinates to 3D world units (meters).
/// Computed from the assumed logical PPI: `world_scale = 1.0 / (ppi * 39.3701)`.
#[derive(Resource)]
pub struct WorldScale(pub f32);

pub struct ByoPlugin;

impl Plugin for ByoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IdMap>()
            .init_resource::<events::observers::PointerEnterState>()
            .init_resource::<kitty_gfx::store::KittyGfxImageStore>()
            .add_message::<ByoBatch>()
            .add_systems(
                Startup,
                (io::setup_io, setup_engine, tty::setup_root_tty).chain(),
            )
            .add_systems(
                PreUpdate,
                (io::process_stdin_events, commands::process_commands).chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    transition::systems::handle_view_transitions,
                    transition::systems::handle_window_transitions,
                    transition::systems::handle_layer_transitions,
                    transition::systems::handle_tty_transitions,
                    style::reconcile_views,
                    style::reconcile_view_images,
                    style::reconcile_view_transforms,
                    style::reconcile_text,
                    style::reconcile_tty_style,
                    style::reconcile_windows,
                    style::reconcile_layers,
                    style::reconcile_view_picking,
                    style::reconcile_text_picking,
                    style::reconcile_layer_picking,
                    style::reconcile_window_picking,
                    style::reorder_children,
                    tty::resize_tty,
                    tty::reconcile_tty,
                    kitty_gfx::placement::reconcile_tty_placements,
                    ApplyDeferred,
                )
                    .chain()
                    .before(UiSystems::Prepare),
            )
            .add_systems(
                Update,
                (
                    transition::systems::tick_view_transitions,
                    transition::systems::tick_window_transitions,
                    transition::systems::tick_layer_transitions,
                    transition::systems::tick_tty_transitions,
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
