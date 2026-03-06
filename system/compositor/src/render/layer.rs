//! Layer render pipeline — render texture + Camera2d + 3D textured plane.

use crate::components::LayerPlane;
use crate::props::types::ByoTextureFormat;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureUsages};
use bevy::render::view::Hdr;

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
    /// Current logical width (for resize detection).
    pub width: u32,
    /// Current logical height (for resize detection).
    pub height: u32,
    /// Texture format (needed for resize re-creation).
    pub format: ByoTextureFormat,
    /// Scale factor at creation time (for physical pixel calculation).
    pub scale_factor: f32,
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
    world_scale: f32,
    format: &ByoTextureFormat,
) -> LayerRender {
    // 1. Create render texture at physical pixel resolution for HiDPI.
    let physical_width = (width as f32 * scale_factor) as u32;
    let physical_height = (height as f32 * scale_factor) as u32;
    let size = Extent3d {
        width: physical_width,
        height: physical_height,
        depth_or_array_layers: 1,
    };
    let texture_format = format.to_wgpu();
    let fill = vec![0u8; format.bytes_per_pixel()];
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &fill,
        texture_format,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image_handle = images.add(image);

    // 2. Spawn Camera2d targeting the render texture.
    //    order: -1 ensures layer cameras render before main cameras.
    //    ImageRenderTarget.scale_factor tells Bevy's UI system the DPI scale
    //    so layout happens in logical pixels while rendering at physical resolution.
    let mut camera_cmd = commands.spawn((
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
    ));
    if format.is_hdr() {
        camera_cmd.insert(Hdr);
    }
    let camera = camera_cmd.id();

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
    //    Plane uses world-scaled dimensions — logical pixels converted to meters
    //    via world_scale so Camera3d's perspective projection maps 1:1 at Z=0.
    let half = Vec2::new(width as f32 / 2.0, height as f32 / 2.0) * world_scale;
    let plane_mesh = meshes.add(Plane3d::new(Vec3::Z, half));
    let plane_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        double_sided: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    let mut plane_cmd = commands.spawn((
        LayerPlane,
        Mesh3d(plane_mesh),
        MeshMaterial3d(plane_material),
        Transform::from_xyz(0.0, 0.0, z_offset * world_scale),
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
        width,
        height,
        format: format.clone(),
        scale_factor,
    }
}

/// Resize a layer's render texture and 3D plane mesh in place.
/// Keeps the same asset handles so Camera and Material auto-update.
pub fn resize_layer_render(
    render: &mut LayerRender,
    new_width: u32,
    new_height: u32,
    world_scale: f32,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    mesh_handles: &Query<&Mesh3d, With<LayerPlane>>,
) {
    if render.width == new_width && render.height == new_height {
        return;
    }

    // 1. Resize the render texture (same handle, replace contents).
    let physical_width = (new_width as f32 * render.scale_factor) as u32;
    let physical_height = (new_height as f32 * render.scale_factor) as u32;
    let size = Extent3d {
        width: physical_width,
        height: physical_height,
        depth_or_array_layers: 1,
    };
    let texture_format = render.format.to_wgpu();
    let fill = vec![0u8; render.format.bytes_per_pixel()];
    if let Some(image) = images.get_mut(&render.image) {
        *image = Image::new_fill(
            size,
            TextureDimension::D2,
            &fill,
            texture_format,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::RENDER_ATTACHMENT;
    }

    // 2. Resize the 3D plane mesh (same handle, replace contents).
    let half = Vec2::new(new_width as f32 / 2.0, new_height as f32 / 2.0) * world_scale;
    if let Ok(mesh3d) = mesh_handles.get(render.plane)
        && let Some(mesh) = meshes.get_mut(&mesh3d.0)
    {
        *mesh = Plane3d::new(Vec3::Z, half).mesh().build();
    }

    render.width = new_width;
    render.height = new_height;
}
