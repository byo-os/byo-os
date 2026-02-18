//! Layer render pipeline — render texture + Camera2d + 3D textured plane.

use bevy::asset::RenderAssetUsages;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

/// Tracks the render pipeline entities for a layer.
#[derive(Component)]
#[allow(dead_code)]
pub struct LayerRender {
    /// Handle to the render texture.
    pub image: Handle<Image>,
    /// The Camera2d entity that renders UI to the texture.
    pub camera: Entity,
    /// The UI root entity (with UiTargetCamera) — views are parented here.
    pub ui_root: Entity,
    /// The 3D plane entity displaying the texture.
    pub plane: Entity,
}

/// Creates the render pipeline for a layer and returns the LayerRender component.
/// Call this from commands.rs when spawning a `+layer`.
///
/// `width`/`height` are logical pixel dimensions. The render texture is created
/// at physical resolution (`width * scale_factor` x `height * scale_factor`) for
/// HiDPI crispness. The 3D plane uses logical dimensions to match the Camera3d's
/// ScalingMode::WindowSize coordinate space.
#[allow(clippy::too_many_arguments)]
pub fn spawn_layer_render(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    window_entity: Option<Entity>,
    width: u32,
    height: u32,
    scale_factor: f32,
    z_offset: f32,
) -> LayerRender {
    // 1. Create render texture at physical pixel resolution for HiDPI.
    let physical_width = (width as f32 * scale_factor) as u32;
    let physical_height = (height as f32 * scale_factor) as u32;
    let size = Extent3d {
        width: physical_width,
        height: physical_height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image_handle = images.add(image);

    // 2. Spawn Camera2d targeting the render texture.
    //    order: -1 ensures layer cameras render before main cameras.
    //    ImageRenderTarget.scale_factor tells Bevy's UI system the DPI scale
    //    so layout happens in logical pixels while rendering at physical resolution.
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: image_handle.clone(),
                scale_factor,
            }),
        ))
        .id();

    // 3. Spawn UI root node targeting this camera
    let ui_root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();

    // 4. Spawn 3D plane with the render texture as material.
    //    Plane uses logical dimensions so it matches Camera3d's ScalingMode::WindowSize.
    let half = Vec2::new(width as f32 / 2.0, height as f32 / 2.0);
    let plane_mesh = meshes.add(Plane3d::new(Vec3::Z, half));
    let plane_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    let mut plane_cmd = commands.spawn((
        Mesh3d(plane_mesh),
        MeshMaterial3d(plane_material),
        Transform::from_xyz(0.0, 0.0, z_offset),
    ));
    if let Some(window) = window_entity {
        plane_cmd.insert(ChildOf(window));
    }
    let plane = plane_cmd.id();

    LayerRender {
        image: image_handle,
        camera,
        ui_root,
        plane,
    }
}
