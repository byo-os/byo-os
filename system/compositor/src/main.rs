mod commands;
mod components;
mod id_map;
mod io;
mod plugin;
mod props;
mod render;
mod style;

use bevy::prelude::*;
use bevy::winit::WinitSettings;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "BYO/OS".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(WinitSettings::desktop_app())
        .add_plugins(plugin::ByoPlugin)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    // Camera2d for direct UI rendering (views without explicit layer)
    commands.spawn(Camera2d);

    // Camera3d (orthographic) for 3D compositing of window/layer planes
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::WindowSize,
            ..OrthographicProjection::default_3d()
        }),
        // Render after 2D UI (higher order)
        Camera {
            order: 1,
            ..default()
        },
    ));
}
