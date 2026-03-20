//! Scroll message handling — processes `.scroll-to` and `.scroll-by` messages.
//!
//! Messages are buffered in PreUpdate (by `process_commands`) and processed
//! in a PreUpdate system that runs after commands, so ScrollPosition state
//! is updated before the frame's tick.
//!
//! After layout (PostUpdate, after `UiSystems::Prepare`), a follow-up system
//! emits `!scroll` events for entities that were scrolled programmatically,
//! so daemons (e.g. controls) can update scrollbar visuals.

use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiSystems};
use byo::byo_write;
use byo::protocol::EventKind;

use crate::events::config::EventSubscriptions;
use crate::events::observers::resolve_forward_target;
use crate::id_map::IdMap;
use crate::io::StdoutEmitter;
use crate::resize;
use crate::scroll::{OverscrollState, rubber_band};

pub struct ScrollMessagePlugin;

impl Plugin for ScrollMessagePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ScrollMessage>()
            .init_resource::<PendingScrollEvents>()
            .init_resource::<ScrollSeqCounter>()
            .add_systems(
                PreUpdate,
                process_scroll_messages.after(crate::commands::process_commands),
            )
            .add_systems(PostUpdate, emit_scroll_events.after(UiSystems::Prepare));
    }
}

/// A buffered scroll message (`.scroll-to` or `.scroll-by`).
#[derive(Message)]
pub enum ScrollMessage {
    ScrollTo {
        target: String,
        x: Option<f32>,
        y: Option<f32>,
        /// When true, the scroll position came from an external source (e.g.
        /// a tap copy from another compositor). Don't emit a `!scroll` event
        /// back to the orchestrator to prevent feedback loops.
        external: bool,
        /// Raw overscroll overflow values (for external scroll sync).
        /// Applied as visual offset via rubber_band() → OverscrollState.
        overflow_x: Option<f32>,
        overflow_y: Option<f32>,
    },
    ScrollBy {
        target: String,
        dx: f32,
        dy: f32,
    },
}

/// Entities that had their scroll position changed programmatically this frame.
#[derive(Resource, Default)]
struct PendingScrollEvents(Vec<Entity>);

/// Monotonic sequence counter for programmatic `!scroll` events.
#[derive(Resource, Default)]
struct ScrollSeqCounter(u64);

impl ScrollSeqCounter {
    fn next(&mut self) -> u64 {
        let seq = self.0;
        self.0 += 1;
        seq
    }
}

fn process_scroll_messages(
    mut commands: Commands,
    mut messages: MessageReader<ScrollMessage>,
    id_map: Res<IdMap>,
    mut scroll_query: Query<&mut ScrollPosition>,
    subs_query: Query<&EventSubscriptions>,
    mut pending: ResMut<PendingScrollEvents>,
    mut redraw: MessageWriter<bevy::window::RequestRedraw>,
) {
    let mut any = false;

    for msg in messages.read() {
        let (entity, sx, sy, external, overflow_x, overflow_y) = match msg {
            ScrollMessage::ScrollTo {
                target,
                x,
                y,
                external,
                overflow_x,
                overflow_y,
            } => {
                let Some(entity) = id_map.get_entity(target) else {
                    continue;
                };
                let entity = resolve_scroll_entity(entity, target, &id_map, &subs_query);
                let Ok(sp) = scroll_query.get(entity) else {
                    continue;
                };
                (entity, x.unwrap_or(sp.x), y.unwrap_or(sp.y), *external, *overflow_x, *overflow_y)
            }
            ScrollMessage::ScrollBy { target, dx, dy } => {
                let Some(entity) = id_map.get_entity(target) else {
                    continue;
                };
                let entity = resolve_scroll_entity(entity, target, &id_map, &subs_query);
                let Ok(sp) = scroll_query.get(entity) else {
                    continue;
                };
                (entity, sp.x + dx, sp.y + dy, false, None, None)
            }
        };

        if let Ok(mut sp) = scroll_query.get_mut(entity) {
            sp.x = sx;
            sp.y = sy;
            if !external && !pending.0.contains(&entity) {
                pending.0.push(entity);
            }
            // Apply overscroll visual offset if provided (from tap sync).
            if overflow_x.is_some() || overflow_y.is_some() {
                let ox = overflow_x.map(|v| -rubber_band(v as f64) as f32).unwrap_or(0.0);
                let oy = overflow_y.map(|v| -rubber_band(v as f64) as f32).unwrap_or(0.0);
                commands.entity(entity).insert(OverscrollState {
                    offset_x: ox,
                    offset_y: oy,
                    external: true,
                });
            }

            any = true;
        }
    }

    if any {
        redraw.write(bevy::window::RequestRedraw);
    }
}

/// PostUpdate system: emit `!scroll` events for programmatically scrolled entities.
///
/// Runs after `UiSystems::Prepare` so `ComputedNode` has authoritative dimensions
/// and `ScrollPosition` has been clamped by Bevy's layout.
fn emit_scroll_events(
    mut pending: ResMut<PendingScrollEvents>,
    mut seq_counter: ResMut<ScrollSeqCounter>,
    id_map: Res<IdMap>,
    emitter: Res<StdoutEmitter>,
    computed_query: Query<&ComputedNode>,
    scroll_query: Query<&ScrollPosition>,
) {
    if pending.0.is_empty() {
        return;
    }

    for entity in pending.0.drain(..) {
        let Some(byo_id) = id_map.get_id(entity) else {
            continue;
        };

        let (vw, vh, cw, ch) = computed_query
            .get(entity)
            .map(resize::compute_dimensions)
            .unwrap_or((0.0, 0.0, 0.0, 0.0));

        let (sx, sy) = scroll_query
            .get(entity)
            .map(|sp| (sp.x as f64, sp.y as f64))
            .unwrap_or((0.0, 0.0));

        let seq = seq_counter.next();
        emitter.frame(|em| {
            byo_write!(em,
                !scroll {seq} {byo_id}
                    scroll-x={format!("{sx:.1}")}
                    scroll-y={format!("{sy:.1}")}
                    content-width={format!("{cw:.1}")}
                    content-height={format!("{ch:.1}")}
                    viewport-width={format!("{vw:.1}")}
                    viewport-height={format!("{vh:.1}")}
            )
        });
    }
}

/// If `entity` has a scroll `forward()` target in its `EventSubscriptions`,
/// resolve to that entity instead.
///
/// This handles tap copies that target the scroll-view root (which has
/// `events="scroll forward(viewport)"`) — the scroll position lives on the
/// viewport, not the root.
fn resolve_scroll_entity(
    entity: Entity,
    target_byo_id: &str,
    id_map: &IdMap,
    subs_query: &Query<&EventSubscriptions>,
) -> Entity {
    if let Ok(subs) = subs_query.get(entity)
        && let Some(sub) = subs.get(&EventKind::Scroll)
        && let Some(ref fwd_id) = sub.forward_target
        && let Some(fwd_entity) = resolve_forward_target(fwd_id, target_byo_id, id_map)
    {
        return fwd_entity;
    }
    entity
}
