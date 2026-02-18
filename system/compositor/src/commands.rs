//! Command processing system — translates BYO commands into ECS operations.

use bevy::prelude::*;
use byo::props::FromProps;

use crate::components::*;
use crate::id_map::IdMap;
use crate::io::ByoBatch;
use crate::props::layer::LayerProps;
use crate::props::text::TextProps;
use crate::props::view::ViewProps;
use crate::props::window::WindowProps;
use crate::render::layer::{LayerRender, spawn_layer_render};
use crate::render::window::WindowRender;

/// PreUpdate system: processes BYO batches and applies them to the ECS.
#[allow(clippy::too_many_arguments)]
pub fn process_commands(
    mut messages: MessageReader<ByoBatch>,
    mut commands: Commands,
    mut id_map: ResMut<IdMap>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut views: Query<&mut ViewProps>,
    mut texts: Query<&mut TextProps>,
    mut layers: Query<&mut LayerProps>,
    mut windows: Query<&mut WindowProps>,
    layer_renders: Query<&LayerRender>,
    primary_window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let scale_factor = primary_window
        .iter()
        .next()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0);
    for batch in messages.read() {
        let mut parent_stack: Vec<Entity> = Vec::new();
        let mut last_entity: Option<Entity> = None;

        for cmd in &batch.0 {
            match cmd {
                byo::Command::Upsert {
                    kind, id, props, ..
                } => {
                    let id_str = id.as_ref();
                    let kind_str = kind.as_ref();

                    let entity = if let Some(existing) = id_map.get_entity(id_str) {
                        // Existing entity — full replace props
                        match kind_str {
                            "view" => {
                                if let Ok(mut vp) = views.get_mut(existing) {
                                    *vp = ViewProps::from_props(props);
                                }
                            }
                            "text" => {
                                if let Ok(mut tp) = texts.get_mut(existing) {
                                    *tp = TextProps::from_props(props);
                                }
                            }
                            "layer" => {
                                if let Ok(mut lp) = layers.get_mut(existing) {
                                    *lp = LayerProps::from_props(props);
                                }
                            }
                            "window" => {
                                if let Ok(mut wp) = windows.get_mut(existing) {
                                    *wp = WindowProps::from_props(props);
                                }
                            }
                            _ => {}
                        }
                        existing
                    } else {
                        let parent = parent_stack.last().copied();
                        let entity = spawn_entity(
                            &mut commands,
                            &mut images,
                            &mut meshes,
                            &mut materials,
                            &layer_renders,
                            kind_str,
                            parent,
                            props,
                            scale_factor,
                        );
                        id_map.insert(id_str.to_string(), entity);
                        entity
                    };

                    // Update order component if set
                    if let Some(order) = extract_order(props) {
                        commands.entity(entity).insert(ByoOrder(order));
                    }

                    last_entity = Some(entity);
                }

                byo::Command::Destroy { id, .. } => {
                    let id_str = id.as_ref();
                    if let Some(entity) = id_map.remove_by_id(id_str) {
                        commands.entity(entity).despawn();
                    }
                    last_entity = None;
                }

                byo::Command::Patch {
                    kind, id, props, ..
                } => {
                    let id_str = id.as_ref();
                    let kind_str = kind.as_ref();
                    if let Some(entity) = id_map.get_entity(id_str) {
                        match kind_str {
                            "view" => {
                                if let Ok(mut vp) = views.get_mut(entity) {
                                    vp.apply_props(props);
                                }
                            }
                            "text" => {
                                if let Ok(mut tp) = texts.get_mut(entity) {
                                    tp.apply_props(props);
                                }
                            }
                            "layer" => {
                                if let Ok(mut lp) = layers.get_mut(entity) {
                                    lp.apply_props(props);
                                }
                            }
                            "window" => {
                                if let Ok(mut wp) = windows.get_mut(entity) {
                                    wp.apply_props(props);
                                }
                            }
                            _ => {}
                        }
                        if let Some(order) = extract_order(props) {
                            commands.entity(entity).insert(ByoOrder(order));
                        }
                        last_entity = Some(entity);
                    }
                }

                byo::Command::Push => {
                    if let Some(entity) = last_entity {
                        parent_stack.push(entity);
                    }
                    last_entity = None;
                }

                byo::Command::Pop => {
                    last_entity = parent_stack.pop();
                }

                _ => {}
            }
        }
    }
}

/// Resolve the actual parent entity for a child. If the parent is a layer,
/// redirect to its `ui_root` so UI nodes render via the layer's Camera2d.
fn resolve_parent(parent: Option<Entity>, layer_renders: &Query<&LayerRender>) -> Option<Entity> {
    parent.map(|p| {
        if let Ok(lr) = layer_renders.get(p) {
            lr.ui_root
        } else {
            p
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_entity(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layer_renders: &Query<&LayerRender>,
    kind: &str,
    parent: Option<Entity>,
    props: &[byo::Prop],
    scale_factor: f32,
) -> Entity {
    match kind {
        "view" => {
            let vp = ViewProps::from_props(props);
            let resolved_parent = resolve_parent(parent, layer_renders);
            let mut ec = commands.spawn((
                ByoView,
                vp,
                Node::default(),
                BackgroundColor::default(),
                BorderColor::default(),
            ));
            if let Some(p) = resolved_parent {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        "text" => {
            let tp = TextProps::from_props(props);
            let content = tp.content.clone().unwrap_or_default();
            let resolved_parent = resolve_parent(parent, layer_renders);
            let mut ec = commands.spawn((
                ByoText,
                tp,
                Text::new(content),
                TextFont::default(),
                TextColor::default(),
            ));
            if let Some(p) = resolved_parent {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        "layer" => {
            let lp = LayerProps::from_props(props);
            let logical_width = extract_layer_size(&lp.width, 1280);
            let logical_height = extract_layer_size(&lp.height, 720);

            // Physical texture size = logical * scale_factor (for HiDPI)
            let physical_width = (logical_width as f32 * scale_factor) as u32;
            let physical_height = (logical_height as f32 * scale_factor) as u32;

            // Spawn the layer entity first
            let mut ec = commands.spawn((ByoLayer, lp));
            if let Some(p) = parent {
                ec.insert(ChildOf(p));
            }
            let layer_entity = ec.id();

            // Create the render pipeline (Camera2d + texture + 3D plane)
            let render = spawn_layer_render(
                commands,
                images,
                meshes,
                materials,
                parent, // window entity for 3D plane parenting
                physical_width,
                physical_height,
            );
            commands.entity(layer_entity).insert(render);

            layer_entity
        }
        "window" => {
            let wp = WindowProps::from_props(props);
            let mut ec = commands.spawn((
                ByoWindow,
                wp,
                WindowRender {
                    default_layer: None,
                },
                Transform::IDENTITY,
                Visibility::default(),
            ));
            if let Some(p) = parent {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        _ => {
            let resolved_parent = resolve_parent(parent, layer_renders);
            let mut ec = commands.spawn(Node::default());
            if let Some(p) = resolved_parent {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
    }
}

/// Extract a pixel size from an optional ByoVal prop, defaulting to `fallback`.
fn extract_layer_size(val: &Option<crate::props::types::ByoVal>, fallback: u32) -> u32 {
    val.as_ref().map_or(fallback, |v| match v.0 {
        Val::Px(px) => px as u32,
        _ => fallback,
    })
}

fn extract_order(props: &[byo::Prop]) -> Option<i32> {
    for prop in props {
        if prop.key() == "order"
            && let byo::Prop::Value { value, .. } = prop
        {
            return value.parse().ok();
        }
    }
    None
}
