//! Bevy observer integration — intercepts picking events and submits them
//! to the propagation engine with pre-built spines.

use std::collections::HashMap;

use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::{MouseButton, MouseScrollUnit};
use bevy::picking::events::Scroll;
use bevy::picking::pointer::{Location, PointerId};
use bevy::prelude::*;
use bevy::ui::{OverflowAxis, ScrollPosition, UiGlobalTransform};
use bevy::window::{CursorLeft, CursorMoved};

use crate::components::{ByoLayer, ByoText, ByoView, ByoWindow};
use crate::events::config::EventSubscriptions;
use crate::id_map::IdMap;

/// Query filter matching any BYO entity type.
type ByoEntityFilter = Or<(
    With<ByoView>,
    With<ByoText>,
    With<ByoLayer>,
    With<ByoWindow>,
)>;

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
        .add_observer(on_pointer_out)
        .add_observer(on_pointer_scroll);
}

/// Build the propagation spine from a hit entity up to the root.
/// Returns spine ordered root → leaf, filtering only BYO entities
/// that have event subscriptions for the given event kind.
#[allow(clippy::too_many_arguments)]
fn build_spine(
    entity: Entity,
    kind: &EventKind,
    pointer: &PointerData,
    id_map: &IdMap,
    parent_query: &Query<&ChildOf>,
    subs_query: &Query<&EventSubscriptions>,
    byo_entities: &Query<(), ByoEntityFilter>,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) -> Vec<SpineNode> {
    let mut spine = Vec::new();
    let mut current = Some(entity);

    while let Some(e) = current {
        // Only include BYO entities that are in the IdMap
        if byo_entities.get(e).is_ok()
            && let Some(byo_id) = id_map.get_id(e)
        {
            // Compute local coordinates and dimensions relative to this element
            let (local_x, local_y, w, h) = compute_local_coords(
                e,
                pointer.client_x,
                pointer.client_y,
                node_query,
                ui_transforms,
            );

            if let Ok(subs) = subs_query.get(e)
                && let Some(sub) = subs.get(kind)
            {
                // Build forward spine if this node has a forward target
                let forward_spine = sub.forward_target.as_ref().and_then(|target_id| {
                    let target_entity =
                        resolve_forward_target(target_id, byo_id, id_map)?;
                    let fwd = build_forward_spine(
                        target_entity,
                        e, // stop before reaching the forwarding node
                        kind,
                        pointer,
                        id_map,
                        parent_query,
                        subs_query,
                        byo_entities,
                        node_query,
                        ui_transforms,
                    );
                    if fwd.is_empty() { None } else { Some(fwd) }
                });

                spine.push(SpineNode {
                    byo_id: byo_id.to_string(),
                    phase: sub.phase,
                    passive: sub.passive,
                    verbose: sub.verbose,
                    local_x,
                    local_y,
                    width: w,
                    height: h,
                    forward_spine,
                });
            }
            // Even if no subscription, we still walk up the tree
        }

        // Walk up to parent
        current = parent_query.get(e).ok().map(|c| c.parent());
    }

    // Reverse so it's root → leaf
    spine.reverse();
    spine
}

/// Resolve a forward target local ID to an Entity.
///
/// The target ID from `forward(id)` is local (e.g. `viewport`). We qualify
/// it using the forwarding node's client prefix (e.g. `controls:viewport`).
pub(crate) fn resolve_forward_target(
    target_local_id: &str,
    forwarding_node_qid: &str,
    id_map: &IdMap,
) -> Option<Entity> {
    if let Some((prefix, _)) = forwarding_node_qid.split_once(':') {
        let qualified = format!("{prefix}:{target_local_id}");
        id_map.get_entity(&qualified)
    } else {
        id_map.get_entity(target_local_id)
    }
}

/// Build a spine from `target_entity` upward, stopping before `stop_at`.
///
/// Used to create the nested capture/bubble cycle for `forward()` targets.
/// The `stop_at` entity (the forwarding node) is excluded to prevent cycles.
#[allow(clippy::too_many_arguments)]
fn build_forward_spine(
    target_entity: Entity,
    stop_at: Entity,
    kind: &EventKind,
    pointer: &PointerData,
    id_map: &IdMap,
    parent_query: &Query<&ChildOf>,
    subs_query: &Query<&EventSubscriptions>,
    byo_entities: &Query<(), ByoEntityFilter>,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) -> Vec<SpineNode> {
    let mut spine = Vec::new();
    let mut current = Some(target_entity);

    while let Some(e) = current {
        if e == stop_at {
            break;
        }
        if byo_entities.get(e).is_ok()
            && let Some(byo_id) = id_map.get_id(e)
        {
            let (local_x, local_y, w, h) = compute_local_coords(
                e,
                pointer.client_x,
                pointer.client_y,
                node_query,
                ui_transforms,
            );
            if let Ok(subs) = subs_query.get(e)
                && let Some(sub) = subs.get(kind)
            {
                spine.push(SpineNode {
                    byo_id: byo_id.to_string(),
                    phase: sub.phase,
                    passive: sub.passive,
                    verbose: sub.verbose,
                    local_x,
                    local_y,
                    width: w,
                    height: h,
                    forward_spine: None, // No nested forwards in forward spines
                });
            }
        }
        current = parent_query.get(e).ok().map(|c| c.parent());
    }

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
    byo_entities: &Query<(), ByoEntityFilter>,
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
        forward_spine: None,
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
pub(crate) fn compute_element_size(
    entity: Entity,
    node_query: &Query<&ComputedNode>,
) -> (f64, f64) {
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

/// Create PointerData from Bevy pointer event fields.
#[allow(clippy::too_many_arguments)]
fn make_pointer_data(
    pointer_id: &PointerId,
    location: &Location,
    button: i8,
    buttons: u16,
    pressure: f32,
    modifiers: Modifiers,
    target: String,
    target_width: f64,
    target_height: f64,
) -> PointerData {
    PointerData {
        pointer_id: pointer_id_to_i64(pointer_id),
        pointer_type: pointer_id_to_type(pointer_id),
        client_x: location.position.x as f64,
        client_y: location.position.y as f64,
        button,
        buttons,
        pressure,
        modifiers,
        target,
        target_width,
        target_height,
        primary: true,
        contact_width: 1.0,
        contact_height: 1.0,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
        altitude: std::f64::consts::FRAC_PI_2,
        azimuth: 0.0,
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
        content_width: 0.0,
        content_height: 0.0,
        viewport_width: 0.0,
        viewport_height: 0.0,
        scroll_x: 0.0,
        scroll_y: 0.0,
        scroll_overflow_x: 0.0,
        scroll_overflow_y: 0.0,
    }
}

/// Build PointerData, spine, and dispatch to the engine in one step.
/// Used by the simple pointer observers (down, up, move, out).
#[allow(clippy::too_many_arguments)]
fn dispatch_pointer_event(
    entity: Entity,
    kind: EventKind,
    pointer: PointerData,
    engine: &EngineHandle,
    id_map: &IdMap,
    parent_query: &Query<&ChildOf>,
    subs_query: &Query<&EventSubscriptions>,
    byo_entities: &Query<(), ByoEntityFilter>,
    node_query: &Query<&ComputedNode>,
    ui_transforms: &Query<&UiGlobalTransform>,
) {
    let spine = build_spine(
        entity,
        &kind,
        &pointer,
        id_map,
        parent_query,
        subs_query,
        byo_entities,
        node_query,
        ui_transforms,
    );
    if !spine.is_empty() {
        engine.send(EngineInput::NewEvent {
            kind,
            pointer: Box::new(pointer),
            spine,
        });
    }
}

// ---------------------------------------------------------------------------
// Global observers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn on_pointer_down(
    mut event: On<Pointer<Press>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    event.propagate(false);
    let btn = bevy_button(event.button);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        btn.wire_value(),
        btn.bitmask(),
        0.5, // mouse default when pressed
        read_modifiers(&keys),
        resolve_target_id(event.entity, &id_map, &parent_query),
        tw,
        th,
    );
    dispatch_pointer_event(
        event.entity,
        EventKind::PointerDown,
        pointer,
        &engine,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_up(
    mut event: On<Pointer<Release>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    capture_state: Res<CaptureState>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    event.propagate(false);
    // During active capture, handle_captured_pointer synthesizes PointerUp
    if capture_state
        .get(pointer_id_to_i64(&event.pointer_id))
        .is_some()
    {
        return;
    }
    let btn = bevy_button(event.button);
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        btn.wire_value(),
        0,
        0.0,
        read_modifiers(&keys),
        resolve_target_id(event.entity, &id_map, &parent_query),
        tw,
        th,
    );
    dispatch_pointer_event(
        event.entity,
        EventKind::PointerUp,
        pointer,
        &engine,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
}

#[allow(clippy::too_many_arguments)]
fn on_pointer_move(
    mut event: On<Pointer<Move>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    capture_state: Res<CaptureState>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    event.propagate(false);
    // During active capture, handle_captured_pointer synthesizes PointerMove
    if capture_state
        .get(pointer_id_to_i64(&event.pointer_id))
        .is_some()
    {
        return;
    }
    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        read_modifiers(&keys),
        resolve_target_id(event.entity, &id_map, &parent_query),
        tw,
        th,
    );
    dispatch_pointer_event(
        event.entity,
        EventKind::PointerMove,
        pointer,
        &engine,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
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
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // Stop Bevy's built-in bubbling — we handle propagation ourselves via the
    // engine's spine-based dispatch. Without this, the observer fires once per
    // ancestor in the hierarchy, each time with a shorter chain, undoing the
    // enter state set by the previous invocation.
    event.propagate(false);

    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        read_modifiers(&keys),
        resolve_target_id(event.entity, &id_map, &parent_query),
        tw,
        th,
    );

    // --- Subtree-aware enter/leave tracking ---
    // Build the full BYO ancestor chain for the new hover target.
    let ancestor_chain = build_ancestor_chain(event.entity, &id_map, &parent_query, &byo_entities);

    let pointer_id = pointer_id_to_i64(&event.pointer_id);
    let prev_chain = enter_state.chains.entry(pointer_id).or_default();

    // Entities that left: in previous chain but not in new (leaf→root for pointerleave).
    // Linear scan is faster than HashSet for typical UI depths (5-15 elements).
    let mut newly_left: Vec<Entity> = prev_chain
        .iter()
        .filter(|e| !ancestor_chain.contains(e))
        .copied()
        .collect();
    newly_left.reverse(); // root→leaf collected, reverse to leaf→root

    // Entities that entered: in new chain but not in previous (root→leaf for pointerenter)
    let newly_entered: Vec<Entity> = ancestor_chain
        .iter()
        .filter(|e| !prev_chain.contains(e))
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
            pointer: Box::new(pointer.clone()),
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
            pointer: Box::new(pointer.clone()),
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
            pointer: Box::new(pointer),
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
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    ui_transforms: Query<&UiGlobalTransform>,
) {
    // Stop Bevy's built-in bubbling (same reason as on_pointer_over).
    event.propagate(false);

    let (tw, th) = compute_element_size(event.entity, &node_query);
    let pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        read_modifiers(&keys),
        resolve_target_id(event.entity, &id_map, &parent_query),
        tw,
        th,
    );
    dispatch_pointer_event(
        event.entity,
        EventKind::PointerOut,
        pointer,
        &engine,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );

    // NOTE: We do NOT update the enter chain here. The enter/leave chain diff
    // is handled entirely by on_pointer_over — when the pointer moves to a
    // new target, Over fires and the chain diff fires pointerleave for entities
    // that left the subtree. If the cursor leaves the window entirely (no Over
    // follows), handle_cursor_left cleans up the chain.
}

/// Pixels per discrete scroll line (matches browser convention).
const SCROLL_LINE_HEIGHT: f32 = 20.0;

#[allow(clippy::too_many_arguments)]
fn on_pointer_scroll(
    mut event: On<Pointer<Scroll>>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    keys: Res<ButtonInput<KeyCode>>,
    parent_query: Query<&ChildOf>,
    subs_query: Query<&EventSubscriptions>,
    byo_entities: Query<(), ByoEntityFilter>,
    node_query: Query<&ComputedNode>,
    style_node_query: Query<&Node>,
    ui_transforms: Query<&UiGlobalTransform>,
    scroll_pos_query: Query<&ScrollPosition>,
    mut scroll_physics: ResMut<crate::scroll::ScrollPhysics>,
    time: Res<Time>,
) {
    // Stop bubbling — Bevy fires this observer for every entity in the
    // hit chain (deepest → root). We handle the event once on the first
    // (deepest) entity and prevent duplicate processing on ancestors.
    event.propagate(false);

    // Filter out native OS momentum events — we generate our own synthetic
    // momentum with consistent physics across all platforms.
    if scroll_physics.is_native_momentum() {
        return;
    }

    let modifiers = read_modifiers(&keys);
    let target = resolve_target_id(event.entity, &id_map, &parent_query);
    let (tw, th) = compute_element_size(event.entity, &node_query);

    // Convert scroll deltas to logical pixels
    let (delta_x, delta_y) = match event.unit {
        MouseScrollUnit::Pixel => (event.x as f64, event.y as f64),
        MouseScrollUnit::Line => (
            event.x as f64 * SCROLL_LINE_HEIGHT as f64,
            event.y as f64 * SCROLL_LINE_HEIGHT as f64,
        ),
    };

    debug!(
        "raw_scroll: hit={} entity={:?} unit={:?} raw=({:.4},{:.4}) delta=({delta_x:.4},{delta_y:.4})",
        target, event.entity, event.unit, event.x, event.y
    );

    // Find the nearest ancestor with a scroll event subscription.
    let Some(event_target) = find_scroll_event_target(event.entity, &parent_query, &subs_query)
    else {
        return;
    };

    // If the event target has forward(), resolve the forward target as the
    // scroll entity (it has ScrollPosition + overflow:scroll). Otherwise
    // the event target itself is the scroll entity.
    let scroll_entity = subs_query
        .get(event_target)
        .ok()
        .and_then(|subs| subs.get(&EventKind::Scroll))
        .and_then(|sub| sub.forward_target.as_ref())
        .and_then(|fwd_id| {
            let byo_id = id_map.get_id(event_target)?;
            resolve_forward_target(fwd_id, byo_id, &id_map)
        })
        .unwrap_or(event_target);

    debug!(
        "raw_scroll: event_target={:?} scroll_entity={:?} byo_id={} (hit={:?})",
        event_target,
        scroll_entity,
        id_map.get_id(scroll_entity).unwrap_or_default(),
        event.entity
    );

    // Compute content/viewport dimensions from the scrollable viewport
    let (content_width, content_height, viewport_width, viewport_height) =
        if let Ok(computed) = node_query.get(scroll_entity) {
            let isf = computed.inverse_scale_factor as f64;
            let size = computed.size();
            let content = computed.content_size();
            (
                content.x as f64 * isf,
                content.y as f64 * isf,
                size.x as f64 * isf,
                size.y as f64 * isf,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

    // Read the current scroll position from Bevy so physics state starts
    // at the correct offset (not 0) when freshly created.
    let (cur_scroll_x, cur_scroll_y) = scroll_pos_query
        .get(scroll_entity)
        .map(|sp| (sp.x as f64, sp.y as f64))
        .unwrap_or((0.0, 0.0));

    // Determine which axes are scrollable from the viewport's overflow config.
    // Only axes with OverflowAxis::Scroll allow scrolling/overscrolling.
    let (scroll_x_enabled, scroll_y_enabled) = style_node_query
        .get(scroll_entity)
        .map(|node| {
            (
                node.overflow.x == OverflowAxis::Scroll,
                node.overflow.y == OverflowAxis::Scroll,
            )
        })
        .unwrap_or((false, false));

    // Feed into scroll physics keyed by the viewport entity
    scroll_physics.on_raw_scroll(
        scroll_entity,
        delta_x,
        delta_y,
        time.elapsed_secs_f64(),
        content_width,
        content_height,
        viewport_width,
        viewport_height,
        cur_scroll_x,
        cur_scroll_y,
        scroll_x_enabled,
        scroll_y_enabled,
    );

    // Read authoritative clamped position and overflow from scroll physics
    let (scroll_x, scroll_y) = scroll_physics.clamped_position(scroll_entity);
    let (overflow_x, overflow_y) = scroll_physics.overflow(scroll_entity);

    let mut pointer = make_pointer_data(
        &event.pointer_id,
        &event.pointer_location,
        -1,
        0,
        0.0,
        modifiers,
        target,
        tw,
        th,
    );
    pointer.scroll_delta_x = delta_x;
    pointer.scroll_delta_y = delta_y;
    pointer.content_width = content_width;
    pointer.content_height = content_height;
    pointer.viewport_width = viewport_width;
    pointer.viewport_height = viewport_height;
    pointer.scroll_x = scroll_x;
    pointer.scroll_y = scroll_y;
    pointer.scroll_overflow_x = overflow_x;
    pointer.scroll_overflow_y = overflow_y;

    // Build spine from the event target (which may forward to the viewport).
    dispatch_pointer_event(
        event_target,
        EventKind::Scroll,
        pointer,
        &engine,
        &id_map,
        &parent_query,
        &subs_query,
        &byo_entities,
        &node_query,
        &ui_transforms,
    );
}

/// Walk up from `entity` to find the nearest ancestor with a Scroll event
/// subscription (the viewport with `events="scroll"`).
pub(crate) fn find_scroll_event_target(
    entity: Entity,
    parent_query: &Query<&ChildOf>,
    subs_query: &Query<&EventSubscriptions>,
) -> Option<Entity> {
    let mut current = Some(entity);
    while let Some(e) = current {
        if subs_query
            .get(e)
            .is_ok_and(|s| s.get(&EventKind::Scroll).is_some())
        {
            return Some(e);
        }
        current = parent_query.get(e).ok().map(|c| c.parent());
    }
    None
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

    let modifiers = read_modifiers(&keys);

    // No cursor position (cursor outside window). If the button was released,
    // still synthesize PointerUp so the engine calls release_capture. Without
    // this, dragging outside the window and releasing leaks the capture forever.
    let Some(pos) = position else {
        if button_released {
            for (pointer_id, byo_id) in capture_state.snapshot() {
                let mut pointer = PointerData::mouse(0.0, 0.0, true);
                pointer.pointer_id = pointer_id;
                pointer.button = Button::Primary.wire_value();
                pointer.buttons = 0;
                pointer.pressure = 0.0;
                pointer.modifiers = modifiers;
                pointer.target = byo_id.clone();
                engine.send(EngineInput::NewEvent {
                    kind: EventKind::PointerUp,
                    pointer: Box::new(pointer),
                    spine: vec![SpineNode::direct(byo_id, 0.0, 0.0, 0.0, 0.0)],
                });
            }
        }
        return;
    };

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

        let spine = vec![SpineNode::direct(byo_id.clone(), local_x, local_y, w, h)];

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
                pointer: Box::new(pointer),
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
                pointer: Box::new(pointer),
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
                    pointer: Box::new(pointer),
                    spine: leave_spine,
                });
            }
        }
    }
}
