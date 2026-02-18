mod commands;
mod components;
mod id_map;
mod io;
mod plugin;
mod props;
mod render;
mod style;

use bevy::core_pipeline::tonemapping::Tonemapping;
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

    // Camera3d (orthographic) for 3D compositing of window/layer planes.
    // Tonemapping::None is critical — this camera displays pre-rendered UI textures,
    // not HDR scene content. Default tone mapping would desaturate/mute colors.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::from(OrthographicProjection {
            near: -1000.0,
            scaling_mode: bevy::camera::ScalingMode::WindowSize,
            ..OrthographicProjection::default_3d()
        }),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 500.0),
    ));
}
