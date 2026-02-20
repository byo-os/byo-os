//! Bevy observer integration — intercepts picking events and submits them
//! to the propagation engine with pre-built spines.

use std::collections::{HashMap, HashSet};

use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::picking::pointer::{Location, PointerId};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::{CursorLeft, CursorMoved};

use crate::components::{ByoLayer, ByoText, ByoView, ByoWindow};
use crate::events::config::EventSubscriptions;
use crate::id_map::IdMap;

use super::engine::{
    Button, CaptureState, EngineHandle, EngineInput, Modifiers, PointerData, PointerType, SpineNode,
};

use byo::protocol::EventKind;

/// Tracks which entities each pointer is currently "inside" for proper
/// W3C pointerenter/pointerleave semantics (subtree-aware, non-bubbling).
///
/// Unlike pointerover/pointerout (which fire when crossing any element boundary,
/// even parent↔child), pointerenter/pointerleave only fire when the pointer
/// truly enters or exits the element's subtree.
#[derive(Resource, Default)]
pub struct PointerEnterState {
    /// Per pointer ID, the ordered chain of BYO entities the pointer is inside (root → leaf).
    chains: HashMap<i64, Vec<Entity>>,
}

/// Register all global pointer event observers.
pub fn register_observers(app: &mut App) {
    app.add_observer(on_pointer_down)
        .add_observer(on_pointer_up)
        .add_observer(on_pointer_move)
        .add_observer(on_pointer_over)
        .add_observer(on_pointer_out);
}

/// Build the propagation spine from a hit entity up to the root.
/// Returns spine ordered root → leaf, filtering only BYO entities
/// that have event subscriptions for the given event kind.
fn build_spine(
    entity: Entity,
    kind: &EventKind,
    pointer: &PointerData,
    id_map: &IdMap,
    parent_query: &Query<&ChildOf>,
    subs_query: &Query<&EventSubscriptions>,
    byo_entities: &Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) -> Vec<SpineNode> {
    let mut spine = Vec::new();
    let mut current = Some(entity);

    while let Some(e) = current {
        // Only include BYO entities that are in the IdMap
        if byo_entities.get(e).is_ok() {
            if let Some(byo_id) = id_map.get_id(e) {
                // Compute local coordinates and dimensions relative to this element
                let (local_x, local_y, w, h) = compute_local_coords(
                    e,
                    pointer.client_x,
                    pointer.client_y,
                    node_query,
                    ui_transforms,
                );

                if let Ok(subs) = subs_query.get(e) {
                    if let Some(sub) = subs.get(kind) {
                        spine.push(SpineNode {
                            byo_id: byo_id.to_string(),
                            phase: sub.phase,
                            passive: sub.passive,
                            verbose: sub.verbose,
                            local_x,
                            local_y,
                            width: w,
                            height: h,
                        });
                    }
                }
                // Even if no subscription, we still walk up the tree
            }
        }

        // Walk up to parent
        current = parent_query.get(e).ok().map(|c| c.parent());
    }

    // Reverse so it's root → leaf
    spine.reverse();
    spine
}

/// Build the full chain of BYO entities from `entity` up to root.
/// Returns entities in root → leaf order. Includes ALL BYO entities
/// in the ancestry, not filtered by event subscription.
fn build_ancestor_chain(
    entity: Entity,
    id_map: &IdMap,
    parent_query: &Query<&ChildOf>,
    byo_entities: &Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
) -> Vec<Entity> {
    let mut chain = Vec::new();
    let mut current = Some(entity);
    while let Some(e) = current {
        if byo_entities.get(e).is_ok() && id_map.get_id(e).is_some() {
            chain.push(e);
        }
        current = parent_query.get(e).ok().map(|c| c.parent());
    }
    chain.reverse(); // root → leaf
    chain
}

/// Build a SpineNode for a single entity, if it has a subscription for the given event kind.
fn build_spine_node(
    entity: Entity,
    kind: &EventKind,
    pointer: &PointerData,
    id_map: &IdMap,
    subs_query: &Query<&EventSubscriptions>,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) -> Option<SpineNode> {
    let byo_id = id_map.get_id(entity)?;
    let subs = subs_query.get(entity).ok()?;
    let sub = subs.get(kind)?;
    let (local_x, local_y, w, h) = compute_local_coords(
        entity,
        pointer.client_x,
        pointer.client_y,
        node_query,
        ui_transforms,
    );
    Some(SpineNode {
        byo_id: byo_id.to_string(),
        phase: sub.phase,
        passive: sub.passive,
        verbose: sub.verbose,
        local_x,
        local_y,
        width: w,
        height: h,
    })
}

/// Compute coordinates relative to a UI node's top-left corner and its dimensions.
/// Returns (local_x, local_y, width, height) in logical pixels.
///
/// Uses Bevy's `ComputedNode::normalize_point()` with `UiGlobalTransform` for correct
/// coordinate mapping. `normalize_point` returns center-based [-0.5, 0.5] coordinates;
/// we convert to top-left-based [0, size] coordinates in logical pixels.
fn compute_local_coords(
    entity: Entity,
    client_x: f64,
    client_y: f64,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) -> (f64, f64, f64, f64) {
    if let Ok(computed) = node_query.get(entity)
        && let Ok(&transform) = ui_transforms.get(entity)
    {
        let isf = computed.inverse_scale_factor as f64;
        let phys_size = computed.size();
        let w = phys_size.x as f64 * isf;
        let h = phys_size.y as f64 * isf;

        // client_x/client_y are logical pixels (from Bevy picking Location.position),
        // but normalize_point expects physical pixels (ComputedNode is in physical space).
        let scale_factor = 1.0 / computed.inverse_scale_factor;
        if let Some(normalized) = computed.normalize_point(
            transform,
            Vec2::new(
                client_x as f32 * scale_factor,
                client_y as f32 * scale_factor,
            ),
        ) {
            // normalize_point returns center-based: (0,0)=center, ±0.5=edges
            // Convert to top-left-based local coordinates
            let local_x = (normalized.x as f64 + 0.5) * w;
            let local_y = (normalized.y as f64 + 0.5) * h;
            (local_x, local_y, w, h)
        } else {
            (client_x, client_y, w, h)
        }
    } else {
        (client_x, client_y, 0.0, 0.0)
    }
}

/// Resolve the BYO ID for an entity, walking up the tree if needed.
fn resolve_target_id(entity: Entity, id_map: &IdMap, parent_query: &Query<&ChildOf>) -> String {
    let mut current = Some(entity);
    while let Some(e) = current {
        if let Some(id) = id_map.get_id(e) {
            return id.to_string();
        }
        current = parent_query.get(e).ok().map(|c| c.parent());
    }
    String::new()
}

/// Compute element dimensions for a given entity in logical pixels.
fn compute_element_size(entity: Entity, node_query: &Query<&ComputedNode>) -> (f64, f64) {
    if let Ok(computed) = node_query.get(entity) {
        let isf = computed.inverse_scale_factor as f64;
        let size = computed.size();
        (size.x as f64 * isf, size.y as f64 * isf)
    } else {
        (0.0, 0.0)
    }
}

/// Convert Bevy PointerId to our i64 pointer ID.
fn pointer_id_to_i64(id: &PointerId) -> i64 {
    match id {
        PointerId::Mouse => 1,
        PointerId::Touch(tid) => *tid as i64 + 100, // offset to avoid collision with mouse
        PointerId::Custom(uuid) => uuid.as_u128() as i64,
    }
}

/// Convert Bevy PointerId to our PointerType.
fn pointer_id_to_type(id: &PointerId) -> PointerType {
    match id {
        PointerId::Mouse => PointerType::Mouse,
        PointerId::Touch(_) => PointerType::Touch,
        PointerId::Custom(_) => PointerType::Mouse,
    }
}

/// Convert Bevy PointerButton to our Button.
fn bevy_button(b: PointerButton) -> Button {
    match b {
        PointerButton::Primary => Button::Primary,
        PointerButton::Secondary => Button::Secondary,
        PointerButton::Middle => Button::Middle,
    }
}

/// Read current modifier key state.
fn read_modifiers(keys: &ButtonInput<KeyCode>) -> Modifiers {
    Modifiers {
        shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ctrl: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        meta: keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]),
    }
}

/// Create PointerData from a Bevy pointer event.
fn make_pointer_data(
    pointer_id: &PointerId,
    location: &Location,
    button: i8,
    buttons: u16,
    pressure: f32,
    primary: bool,
    modifiers: Modifiers,
    target: String,
    target_width: f64,
    target_height: f64,
) -> PointerData {
    let mut data = PointerData::mouse(
        location.position.x as f64,
        location.position.y as f64,
        primary,
    );
    data.pointer_id = pointer_id_to_i64(pointer_id);
    data.pointer_type = pointer_id_to_type(pointer_id);
    data.button = button;
    data.buttons = buttons;
    data.pressure = pressure;
    data.modifiers = modifiers;
    data.target = target;
    data.target_width = target_width;
    data.target_height = target_height;
    data
}

// ---------------------------------------------------------------------------
// Global observers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn on_pointer_down(
    event: On<Pointer<Press>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    let btn = bevy_button(event.button);
    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        btn.wire_value(),
        btn.bitmask(),
        0.5, // mouse default when pressed
        true,
        modifiers,
        target,
        tw,
        th,
    );

    let spine = build_spine(
        event.entity,
        &EventKind::PointerDown,
        &pointer,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );

    if !spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerDown,
            pointer,
            spine,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_up(
    event: On<Pointer<Release>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    capture_state: Res<CaptureState>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // During active capture, handle_captured_pointer synthesizes PointerUp
    let pid = pointer_id_to_i64(&event.pointer_id);
    if capture_state.get(pid).is_some() {
        return;
    }

    let btn = bevy_button(event.button);
    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        btn.wire_value(),
        0, // buttons released
        0.0,
        true,
        modifiers,
        target,
        tw,
        th,
    );

    let spine = build_spine(
        event.entity,
        &EventKind::PointerUp,
        &pointer,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );

    if !spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerUp,
            pointer,
            spine,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_move(
    event: On<Pointer<Move>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    capture_state: Res<CaptureState>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // During active capture, handle_captured_pointer synthesizes PointerMove
    let pid = pointer_id_to_i64(&event.pointer_id);
    if capture_state.get(pid).is_some() {
        return;
    }

    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        true,
        modifiers,
        target,
        tw,
        th,
    );

    let spine = build_spine(
        event.entity,
        &EventKind::PointerMove,
        &pointer,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );

    if !spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerMove,
            pointer,
            spine,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_over(
    mut event: On<Pointer<Over>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    mut enter_state: ResMut<PointerEnterState>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // Stop Bevy's built-in bubbling — we handle propagation ourselves via the
    // engine's spine-based dispatch. Without this, the observer fires once per
    // ancestor in the hierarchy, each time with a shorter chain, undoing the
    // enter state set by the previous invocation.
    event.propagate(false);

    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        true,
        modifiers,
        target,
        tw,
        th,
    );

    // --- Subtree-aware enter/leave tracking ---
    // Build the full BYO ancestor chain for the new hover target.
    let ancestor_chain = build_ancestor_chain(event.entity, &id_map, &parent_query, &byo_entities);
    let ancestor_set: HashSet<Entity> = ancestor_chain.iter().copied().collect();

    let pointer_id = pointer_id_to_i64(&event.pointer_id);
    let prev_chain = enter_state.chains.entry(pointer_id).or_default();
    let prev_set: HashSet<Entity> = prev_chain.iter().copied().collect();

    // Entities that left: in previous chain but not in new (leaf→root for pointerleave)
    let mut newly_left: Vec<Entity> = prev_chain
        .iter()
        .filter(|e| !ancestor_set.contains(e))
        .copied()
        .collect();
    newly_left.reverse(); // root→leaf collected, reverse to leaf→root

    // Entities that entered: in new chain but not in previous (root→leaf for pointerenter)
    let newly_entered: Vec<Entity> = ancestor_chain
        .iter()
        .filter(|e| !prev_set.contains(e))
        .copied()
        .collect();

    // Update state
    *prev_chain = ancestor_chain;

    // W3C event order: pointerout(old) → pointerleave(old) → pointerover(new) → pointerenter(new)
    // pointerout is handled by on_pointer_out, so here we fire: leave → over → enter

    // 1. pointerleave for entities that left the subtree (leaf→root, non-bubbling)
    let leave_spine: Vec<SpineNode> = newly_left
        .iter()
        .filter_map(|&e| {
            build_spine_node(
                e,
                &EventKind::PointerLeave,
                &pointer,
                &id_map,
                &subs_query,
                &node_query,
                &ui_transforms,
            )
        })
        .collect();
    if !leave_spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerLeave,
            pointer: pointer.clone(),
            spine: leave_spine,
        });
    }

    // 2. pointerover (bubbles through the spine)
    let over_spine = build_spine(
        event.entity,
        &EventKind::PointerOver,
        &pointer,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
    if !over_spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerOver,
            pointer: pointer.clone(),
            spine: over_spine,
        });
    }

    // 3. pointerenter for entities newly entered (root→leaf, non-bubbling)
    let enter_spine: Vec<SpineNode> = newly_entered
        .iter()
        .filter_map(|&e| {
            build_spine_node(
                e,
                &EventKind::PointerEnter,
                &pointer,
                &id_map,
                &subs_query,
                &node_query,
                &ui_transforms,
            )
        })
        .collect();
    if !enter_spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerEnter,
            pointer,
            spine: enter_spine,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_out(
    mut event: On<Pointer<Out>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<
        (),
        Or<(
            With<ByoView>,
            With<ByoText>,
            With<ByoLayer>,
            With<ByoWindow>,
        )>,
    >,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // Stop Bevy's built-in bubbling (same reason as on_pointer_over).
    event.propagate(false);

    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        true,
        modifiers,
        target,
        tw,
        th,
    );

    // Fire pointerout (bubbling via our engine's spine dispatch).
    let out_spine = build_spine(
        event.entity,
        &EventKind::PointerOut,
        &pointer,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
    if !out_spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind: EventKind::PointerOut,
            pointer,
            spine: out_spine,
        });
    }

    // NOTE: We do NOT update the enter chain here. The enter/leave chain diff
    // is handled entirely by on_pointer_over — when the pointer moves to a
    // new target, Over fires and the chain diff fires pointerleave for entities
    // that left the subtree. If the cursor leaves the window entirely (no Over
    // follows), handle_cursor_left cleans up the chain.
}

// ---------------------------------------------------------------------------
// Window-level capture pointer synthesis
// ---------------------------------------------------------------------------

/// System that synthesizes PointerMove/PointerUp events during active pointer
/// captures. Bevy's entity-level picking observers only fire when the cursor is
/// over a pickable entity — during a drag over empty space (or outside the
/// window), no events reach the propagation thread. This system uses
/// window-level `CursorMoved` + `ButtonInput<MouseButton>` to fill the gap
/// with proper element-local coordinates.
#[allow(clippy::too_many_arguments)]
pub fn handle_captured_pointer(
    capture_state: Res<CaptureState>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut cursor_moved: MessageReader<CursorMoved>,
    windows: Query<&Window>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    if capture_state.is_empty() {
        // Drain to avoid stale messages building up
        cursor_moved.read().for_each(drop);
        return;
    }

    // Collect last cursor position this frame
    let last_move_pos = cursor_moved.read().last().map(|e| e.position);
    let button_released = mouse_buttons.just_released(MouseButton::Left);

    if last_move_pos.is_none() && !button_released {
        return;
    }

    // Prefer last CursorMoved position, fall back to window cursor position
    let position =
        last_move_pos.or_else(|| windows.iter().next().and_then(|w| w.cursor_position()));
    let Some(pos) = position else { return };

    let modifiers = read_modifiers(&keys);

    // Compute buttons bitmask from current mouse state
    let mut buttons: u16 = 0;
    if mouse_buttons.pressed(MouseButton::Left) {
        buttons |= 1;
    }
    if mouse_buttons.pressed(MouseButton::Right) {
        buttons |= 2;
    }
    if mouse_buttons.pressed(MouseButton::Middle) {
        buttons |= 4;
    }

    for (pointer_id, byo_id) in capture_state.snapshot() {
        let Some(entity) = id_map.get_entity(&byo_id) else {
            continue;
        };

        let (local_x, local_y, w, h) = compute_local_coords(
            entity,
            pos.x as f64,
            pos.y as f64,
            &node_query,
            &ui_transforms,
        );

        let spine = vec![SpineNode {
            byo_id: byo_id.clone(),
            phase: super::config::Phase::Bubble,
            passive: false,
            verbose: false,
            local_x,
            local_y,
            width: w,
            height: h,
        }];

        if last_move_pos.is_some() {
            let mut pointer = PointerData::mouse(pos.x as f64, pos.y as f64, true);
            pointer.pointer_id = pointer_id;
            pointer.button = -1;
            pointer.buttons = buttons;
            pointer.pressure = if buttons > 0 { 0.5 } else { 0.0 };
            pointer.modifiers = modifiers;
            pointer.target = byo_id.clone();
            pointer.target_width = w;
            pointer.target_height = h;

            engine.send(EngineInput::NewEvent {
                kind: EventKind::PointerMove,
                pointer,
                spine: spine.clone(),
            });
        }

        if button_released {
            let mut pointer = PointerData::mouse(pos.x as f64, pos.y as f64, true);
            pointer.pointer_id = pointer_id;
            pointer.button = Button::Primary.wire_value();
            pointer.buttons = 0;
            pointer.pressure = 0.0;
            pointer.modifiers = modifiers;
            pointer.target = byo_id.clone();
            pointer.target_width = w;
            pointer.target_height = h;

            engine.send(EngineInput::NewEvent {
                kind: EventKind::PointerUp,
                pointer,
                spine,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Cursor-left-window cleanup
// ---------------------------------------------------------------------------

/// System that clears enter state and fires pointerleave when the cursor
/// leaves the compositor window entirely (no `Over` will follow).
pub fn handle_cursor_left(
    mut messages: MessageReader<CursorLeft>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    mut enter_state: ResMut<PointerEnterState>,
    subs_query: Query<&EventSubscriptions>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    for _event in messages.read() {
        // Clear all pointer chains and fire pointerleave for all entered entities
        let chains: Vec<(i64, Vec<Entity>)> = enter_state.chains.drain().collect();
        for (pointer_id, chain) in chains {
            let mut pointer = PointerData::mouse(0.0, 0.0, true);
            pointer.pointer_id = pointer_id;

            // Fire pointerleave in leaf→root order
            let leave_spine: Vec<SpineNode> = chain
                .iter()
                .rev()
                .filter_map(|&e| {
                    build_spine_node(
                        e,
                        &EventKind::PointerLeave,
                        &pointer,
                        &id_map,
                        &subs_query,
                        &node_query,
                        &ui_transforms,
                    )
                })
                .collect();
            if !leave_spine.is_empty() {
                engine.send(EngineInput::NewEvent {
                    kind: EventKind::PointerLeave,
                    pointer,
                    spine: leave_spine,
                });
            }
        }
    }
}
