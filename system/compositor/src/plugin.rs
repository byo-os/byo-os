//! BYO compositor plugin — registers resources, events, and systems.

use bevy::prelude::*;

use crate::commands;
use crate::id_map::IdMap;
use crate::io::{self, ByoBatch};
use crate::style;

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
                    style::reconcile_text,
                    style::reorder_children,
                ),
            );
    }
}
