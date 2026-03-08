//! Command processing system — translates BYO commands into ECS operations.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use byo::props::FromProps;

use crate::components::*;
use crate::events::engine::{EngineHandle, EngineInput};
use crate::id_map::IdMap;
use crate::io::ByoBatch;
use crate::measure::MeasureRequest;
use crate::plugin::WorldScale;
use crate::scroll_message::ScrollMessage;

/// Bundled message writers to stay within Bevy's 16-param limit.
#[derive(SystemParam)]
pub(crate) struct CommandOutputs<'w> {
    measure: MessageWriter<'w, MeasureRequest>,
    scroll: MessageWriter<'w, ScrollMessage>,
}
use crate::props::layer::LayerProps;
use crate::props::text::TextProps;
use crate::props::tty::TtyProps;
use crate::props::view::ViewProps;
use crate::props::window::WindowProps;
use crate::render::layer::{LayerRender, spawn_layer_render};
use crate::render::window::WindowRender;
use crate::transition::config::TransitionConfig;
use crate::transition::state::ActiveTransitions;
use crate::tty::TtyState;

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
    mut ttys: Query<&mut TtyProps>,
    layer_renders: Query<&LayerRender>,
    primary_window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    world_scale: Res<WorldScale>,
    mut outputs: CommandOutputs,
    engine: Option<Res<EngineHandle>>,
) {
    let scale_factor = primary_window
        .iter()
        .next()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0);

    // Eagerly track layer→ui_root mappings for layers created in this frame,
    // since deferred Commands haven't applied LayerRender components yet.
    let mut pending_layer_ui_roots: HashMap<Entity, Entity> = HashMap::new();

    for batch in messages.read() {
        debug!("processing batch of {} commands", batch.0.len());
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
                            "tty" => {
                                if let Ok(mut tp) = ttys.get_mut(existing) {
                                    *tp = TtyProps::from_props(props);
                                }
                            }
                            _ => {}
                        }

                        // Reparent if the upsert context implies a different
                        // parent (e.g. after daemon expansion replay).
                        let new_parent = parent_stack.last().copied();
                        if let Some(p) = new_parent {
                            commands.entity(existing).insert(ChildOf(p));
                        } else {
                            commands.entity(existing).remove::<ChildOf>();
                        }

                        existing
                    } else {
                        let parent = parent_stack.last().copied();
                        spawn_entity(
                            &mut commands,
                            &mut images,
                            &mut meshes,
                            &mut materials,
                            &layer_renders,
                            &mut pending_layer_ui_roots,
                            kind_str,
                            parent,
                            props,
                            scale_factor,
                            world_scale.0,
                        )
                    };

                    id_map.insert(id_str.to_string(), entity);
                    info!("upsert {kind_str} {id_str:?} -> {entity:?}");

                    if let Some(order) = extract_order(props) {
                        commands.entity(entity).insert(ByoOrder(order));
                    }

                    last_entity = Some(entity);
                }

                byo::Command::Destroy { id, .. } => {
                    let id_str = id.as_ref();
                    if let Some(entity) = id_map.remove_by_id(id_str) {
                        commands.entity(entity).despawn();
                        // Release any pointer captures targeting this entity
                        if let Some(ref engine) = engine {
                            engine.send(EngineInput::EntityDestroyed {
                                byo_id: id_str.to_string(),
                            });
                        }
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
                            "tty" => {
                                if let Ok(mut tp) = ttys.get_mut(entity) {
                                    tp.apply_props(props);
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

                byo::Command::Push { slot } => {
                    debug_assert!(slot.is_none(), "compositor received slotted push");
                    if let Some(entity) = last_entity {
                        parent_stack.push(entity);
                    }
                    last_entity = None;
                }

                byo::Command::Pop => {
                    last_entity = parent_stack.pop();
                }

                byo::Command::Ack { kind, seq, props } => {
                    if let Some(ref engine) = engine {
                        let handled = props.iter().any(|p| {
                            matches!(p, byo::Prop::Value { key, value } if key.as_ref() == "handled" && value.as_ref() == "true")
                                || matches!(p, byo::Prop::Boolean { key } if key.as_ref() == "handled")
                        });
                        let capture = props.iter().any(|p| {
                            matches!(p, byo::Prop::Value { key, value } if key.as_ref() == "capture" && value.as_ref() == "true")
                                || matches!(p, byo::Prop::Boolean { key } if key.as_ref() == "capture")
                        });
                        engine.send(EngineInput::Ack {
                            kind: kind.clone(),
                            seq: *seq,
                            handled,
                            capture,
                        });
                    }
                }

                byo::Command::Request {
                    kind, seq, targets, ..
                } if kind.as_str() == "measure" => {
                    if let Some(target) = targets.first() {
                        outputs.measure.write(MeasureRequest {
                            target: target.as_ref().to_string(),
                            seq: *seq,
                        });
                    }
                }

                byo::Command::Message {
                    kind: byo::protocol::MessageKind::ScrollTo,
                    target,
                    props,
                    ..
                } => {
                    outputs.scroll.write(ScrollMessage::ScrollTo {
                        target: target.as_ref().to_string(),
                        x: prop_f32(props, "x"),
                        y: prop_f32(props, "y"),
                    });
                }

                byo::Command::Message {
                    kind: byo::protocol::MessageKind::ScrollBy,
                    target,
                    props,
                    ..
                } => {
                    outputs.scroll.write(ScrollMessage::ScrollBy {
                        target: target.as_ref().to_string(),
                        dx: prop_f32(props, "dx").unwrap_or(0.0),
                        dy: prop_f32(props, "dy").unwrap_or(0.0),
                    });
                }

                _ => {}
            }
        }
    }
}

/// Resolve the actual parent entity for a child. If the parent is a layer,
/// redirect to its `ui_root` so UI nodes render via the layer's Camera2d.
/// Checks the pending map first (for layers created this frame), then the ECS query.
fn resolve_parent(
    parent: Option<Entity>,
    layer_renders: &Query<&LayerRender>,
    pending: &HashMap<Entity, Entity>,
) -> Option<Entity> {
    parent.map(|p| {
        if let Some(&ui_root) = pending.get(&p) {
            return ui_root;
        }
        if let Ok(lr) = layer_renders.get(p) {
            return lr.ui_root;
        }
        p
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_entity(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layer_renders: &Query<&LayerRender>,
    pending_layer_ui_roots: &mut HashMap<Entity, Entity>,
    kind: &str,
    parent: Option<Entity>,
    props: &[byo::Prop],
    scale_factor: f32,
    world_scale: f32,
) -> Entity {
    match kind {
        "view" => {
            let vp = ViewProps::from_props(props);
            let resolved = resolve_parent(parent, layer_renders, pending_layer_ui_roots);
            let mut ec = commands.spawn((
                ByoView,
                vp,
                Node::default(),
                BackgroundColor::default(),
                BorderColor::default(),
                BoxShadow::default(),
                TransitionConfig::default(),
                ActiveTransitions::default(),
            ));
            if let Some(p) = resolved {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        "text" => {
            let tp = TextProps::from_props(props);
            let content = tp.content.clone().unwrap_or_default();
            let resolved = resolve_parent(parent, layer_renders, pending_layer_ui_roots);
            let mut ec = commands.spawn((
                ByoText,
                tp,
                Text::new(content),
                TextFont::default(),
                TextColor::default(),
            ));
            if let Some(p) = resolved {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        "layer" => {
            let lp = LayerProps::from_props(props);
            // Parse class once — resolve width, height, format from class with wire overrides
            let mut ts = crate::style::tailwind::TransformStyle::default();
            if let Some(ref class) = lp.class {
                crate::style::tailwind::apply_transform_classes(&mut ts, class);
            }
            let width = extract_layer_size(lp.width.as_ref().or(ts.width.as_ref()), 1280);
            let height = extract_layer_size(lp.height.as_ref().or(ts.height.as_ref()), 720);
            let format = lp.format.clone().or(ts.format).unwrap_or_default();
            let order_scale = lp.order_scale.unwrap_or(0.001);
            let z_offset = match lp.order_mode {
                Some(crate::props::types::ByoOrderMode::TranslateZ) | None => {
                    lp.order.unwrap_or(0) as f32 * order_scale
                }
                _ => 0.0,
            };

            let mut ec = commands.spawn((
                ByoLayer,
                lp,
                TransitionConfig::default(),
                ActiveTransitions::default(),
            ));
            if let Some(p) = parent {
                ec.insert(ChildOf(p));
            }
            let layer_entity = ec.id();

            let render = spawn_layer_render(
                commands,
                images,
                meshes,
                materials,
                parent,
                width,
                height,
                scale_factor,
                z_offset,
                world_scale,
                &format,
            );

            // Record ui_root eagerly so same-batch views can find it
            pending_layer_ui_roots.insert(layer_entity, render.ui_root);
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
                TransitionConfig::default(),
                ActiveTransitions::default(),
            ));
            if let Some(p) = parent {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        "tty" => {
            let tp = TtyProps::from_props(props);
            let resolved_tty = crate::style::resolve_tty_props(&tp);
            let state = TtyState::new(80, 24, resolved_tty.scrollback as usize);
            let resolved = resolve_parent(parent, layer_renders, pending_layer_ui_roots);
            let mut ec = commands.spawn((
                ByoTty,
                tp,
                state,
                crate::kitty_gfx::placement::TtyPlacements::default(),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Start,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::default(),
                BoxShadow::default(),
                TransitionConfig::default(),
                ActiveTransitions::default(),
            ));
            if let Some(p) = resolved {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
        _ => {
            let resolved = resolve_parent(parent, layer_renders, pending_layer_ui_roots);
            let mut ec = commands.spawn(Node::default());
            if let Some(p) = resolved {
                ec.insert(ChildOf(p));
            }
            ec.id()
        }
    }
}

fn extract_layer_size(val: Option<&crate::props::types::ByoVal>, fallback: u32) -> u32 {
    val.map_or(fallback, |v| match v.0 {
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

/// Extract a named f32 prop value from a prop slice.
fn prop_f32(props: &[byo::Prop], key: &str) -> Option<f32> {
    props.iter().find_map(|p| match p {
        byo::Prop::Value { key: k, value } if k.as_ref() == key => value.parse().ok(),
        _ => None,
    })
}
