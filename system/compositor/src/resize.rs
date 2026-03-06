//! Resize event detection — emits `!resize` events when `ComputedNode`
//! dimensions change for entities that subscribe to `Resize` events.
//!
//! Runs in PostUpdate, after layout reconciliation. Bypasses the propagation
//! engine (no capture/bubble — each subscriber gets its own event directly).

use bevy::prelude::*;
use byo::protocol::{EventKind, Prop};

use crate::events::config::EventSubscriptions;
use crate::id_map::IdMap;
use crate::io::StdoutEmitter;

/// Tracks the last-emitted dimensions for an entity, to detect changes.
#[derive(Component)]
pub struct PreviousSize {
    width: f64,
    height: f64,
    content_width: f64,
    content_height: f64,
}

/// Monotonic sequence counter for `!resize` events.
#[derive(Resource, Default)]
pub struct ResizeSeqCounter(u64);

impl ResizeSeqCounter {
    fn next(&mut self) -> u64 {
        let seq = self.0;
        self.0 += 1;
        seq
    }
}

/// Minimum change in logical pixels to trigger a resize event.
/// Avoids noise from sub-pixel rounding.
const MIN_CHANGE: f64 = 0.5;

/// Extract (width, height, content_width, content_height) in logical pixels.
fn compute_dimensions(computed: &ComputedNode) -> (f64, f64, f64, f64) {
    let isf = computed.inverse_scale_factor as f64;
    let size = computed.size();
    let content = computed.content_size();
    (
        size.x as f64 * isf,
        size.y as f64 * isf,
        content.x as f64 * isf,
        content.y as f64 * isf,
    )
}

/// PostUpdate system: detect `ComputedNode` size changes and emit `!resize`
/// events for entities subscribed to `Resize`.
pub fn emit_resize_events(
    mut commands: Commands,
    mut seq_counter: ResMut<ResizeSeqCounter>,
    emitter: Res<StdoutEmitter>,
    id_map: Res<IdMap>,
    // Entities with resize subscription but no PreviousSize yet (first detection)
    new_query: Query<(Entity, &EventSubscriptions, &ComputedNode), Without<PreviousSize>>,
    // Entities with resize subscription AND PreviousSize (change detection)
    mut existing_query: Query<(
        Entity,
        &EventSubscriptions,
        &ComputedNode,
        &mut PreviousSize,
    )>,
) {
    // Collect events to emit (avoid holding borrows during emission)
    let mut events: Vec<(String, f64, f64, f64, f64)> = Vec::new();

    // --- First-time detection: insert PreviousSize and fire initial event ---
    for (entity, subs, computed) in &new_query {
        if subs.get(&EventKind::Resize).is_none() {
            continue;
        }
        let Some(byo_id) = id_map.get_id(entity) else {
            continue;
        };

        let (w, h, cw, ch) = compute_dimensions(computed);

        commands.entity(entity).insert(PreviousSize {
            width: w,
            height: h,
            content_width: cw,
            content_height: ch,
        });

        // Fire initial event if dimensions are non-zero
        if w > 0.0 || h > 0.0 {
            events.push((byo_id.to_string(), w, h, cw, ch));
        }
    }

    // --- Change detection: compare and fire if changed ---
    for (entity, subs, computed, mut prev) in &mut existing_query {
        if subs.get(&EventKind::Resize).is_none() {
            continue;
        }
        let Some(byo_id) = id_map.get_id(entity) else {
            continue;
        };

        let (w, h, cw, ch) = compute_dimensions(computed);

        let changed = (w - prev.width).abs() > MIN_CHANGE
            || (h - prev.height).abs() > MIN_CHANGE
            || (cw - prev.content_width).abs() > MIN_CHANGE
            || (ch - prev.content_height).abs() > MIN_CHANGE;

        if changed {
            prev.width = w;
            prev.height = h;
            prev.content_width = cw;
            prev.content_height = ch;
            events.push((byo_id.to_string(), w, h, cw, ch));
        }
    }

    // --- Emit all resize events ---
    if !events.is_empty() {
        for (byo_id, w, h, cw, ch) in &events {
            info!("resize event: {byo_id} {w:.1}x{h:.1} content={cw:.1}x{ch:.1}");
        }
        for (byo_id, w, h, cw, ch) in events {
            let seq = seq_counter.next();
            let props = [
                Prop::val("width", format!("{w:.1}")),
                Prop::val("height", format!("{h:.1}")),
                Prop::val("content-width", format!("{cw:.1}")),
                Prop::val("content-height", format!("{ch:.1}")),
            ];
            emitter.frame(|em| em.event("resize", seq, &byo_id, &props));
        }
    }
}
