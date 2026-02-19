mod commands;
mod components;
mod events;
mod id_map;
mod io;
mod plugin;
mod props;
mod render;
mod style;
mod transition;

use std::f32::consts::FRAC_PI_6;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::GlobalAmbientLight;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WinitSettings;

/// Vertical field of view for the 3D compositing camera (30°).
/// Narrow enough for near-orthographic behavior at Z≈0, wide enough
/// for visible perspective on layers spread in Z (carousels, tilted cards).
const CAMERA_3D_FOV: f32 = FRAC_PI_6;

/// Assumed logical pixels-per-inch for the display. Combined with
/// 39.3701 inches/meter to derive the world scale factor. Protocol
/// coordinates are in logical pixels; the 3D pipeline converts them
/// to meters using `1.0 / (PPI × 39.3701)`.
/// 96 = Windows/Linux standard; macOS Retina ≈ 127 at default scaling.
const LOGICAL_PPI: f32 = 96.0;

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
        .insert_resource(plugin::WorldScale(1.0 / (LOGICAL_PPI * 39.3701)))
        .add_plugins(plugin::ByoPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, frame_camera_3d)
        .run();
}

fn setup(mut commands: Commands) {
    // Camera2d for direct UI rendering (views without explicit layer)
    commands.spawn(Camera2d);

    // Camera3d (perspective) for 3D compositing of window/layer planes.
    // Positioned so a plane at Z=0 maps 1:1 with logical pixels — objects
    // near Z=0 look orthographic, but layers spread in Z get natural
    // perspective foreshortening.
    // Tonemapping::None preserves exact UI texture colors. Bloom adds
    // glow around HDR emissive layers.
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Bloom::NATURAL,
        Projection::from(PerspectiveProjection {
            fov: CAMERA_3D_FOV,
            near: 0.01,
            far: 100.0,
            ..default()
        }),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 1.0), // updated by frame_camera_3d
    ));

    // Directional light for PBR materials on layers.
    // Hardcoded for now — future: configurable via protocol props.
    commands.spawn((
        DirectionalLight {
            illuminance: 2000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.3, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 100.0,
        ..default()
    });
}

/// Positions Camera3d so a plane at Z=0 maps 1:1 with logical pixels.
/// The window's logical height is converted to meters via [`WorldScale`],
/// then `camera_z = (height_m / 2) / tan(fov / 2)`.
fn frame_camera_3d(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Transform, &Projection), With<Camera3d>>,
    world_scale: Res<plugin::WorldScale>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((mut transform, projection)) = cameras.single_mut() else {
        return;
    };
    let Projection::Perspective(persp) = projection else {
        return;
    };

    let height = window.height() * world_scale.0;
    if height > 0.0 {
        transform.translation.z = (height / 2.0) / (persp.fov / 2.0).tan();
    }
}
