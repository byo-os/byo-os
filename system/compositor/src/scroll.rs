//! Scroll physics — signal filter between raw OS scroll events and the BYO
//! event system. Smooths discrete events into continuous momentum, handling
//! platform differences transparently. The daemon always sees a smooth stream
//! of `!scroll` events regardless of platform.
//!
//! Architecture:
//! ```text
//! Raw OS scroll (Pointer<Scroll>) → ScrollPhysics filter → BYO !scroll events
//! ```
//!
//! On macOS, native trackpad momentum keeps arriving as `Pointer<Scroll>` with
//! decreasing deltas — the filter is transparent. On Linux / discrete wheel,
//! events arrive in bursts then stop; after a gap, the filter enters Momentum
//! phase and synthesizes smooth decreasing deltas.

use std::collections::HashMap;

use bevy::prelude::*;

/// Per-entity scroll physics state.
///
/// `scroll_x/y` is the **unclamped** source of truth — it can go negative
/// (past min) or beyond max (past end). Everything else is derived:
///   clamped  = clamp(scroll, 0, max) → written to Bevy ScrollPosition
///   overflow = scroll - clamped       → rubber_band() → visual UiTransform
#[derive(Debug)]
struct EntityScrollState {
    /// Current velocity in px/s (X axis).
    velocity_x: f64,
    /// Current velocity in px/s (Y axis).
    velocity_y: f64,
    /// Time of last raw scroll event (seconds since app start).
    last_event_time: f64,
    /// Whether synthetic momentum is active.
    momentum_active: bool,
    /// Current scroll phase.
    phase: ScrollPhase,
    /// Content dimensions (cached from last scroll event).
    content_width: f64,
    content_height: f64,
    viewport_width: f64,
    viewport_height: f64,
    /// Unclamped scroll position. Can go negative or past max for overscroll.
    scroll_x: f64,
    scroll_y: f64,
    /// Whether scrolling is enabled on each axis (from overflow configuration).
    /// When false, deltas and velocities on that axis are zeroed out.
    scroll_x_enabled: bool,
    scroll_y_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ScrollPhase {
    /// No scroll activity.
    Idle,
    /// User is actively scrolling (raw events arriving).
    Active,
    /// Synthetic momentum after user stops scrolling.
    Momentum,
}

impl Default for EntityScrollState {
    fn default() -> Self {
        Self {
            velocity_x: 0.0,
            velocity_y: 0.0,
            last_event_time: 0.0,
            momentum_active: false,
            phase: ScrollPhase::Idle,
            content_width: 0.0,
            content_height: 0.0,
            viewport_width: 0.0,
            viewport_height: 0.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_x_enabled: true,
            scroll_y_enabled: true,
        }
    }
}

impl EntityScrollState {
    /// Max scrollable offset per axis.
    fn max_x(&self) -> f64 {
        (self.content_width - self.viewport_width).max(0.0)
    }
    fn max_y(&self) -> f64 {
        (self.content_height - self.viewport_height).max(0.0)
    }
    /// Clamped scroll position (what Bevy should use).
    fn clamped_x(&self) -> f64 {
        self.scroll_x.clamp(0.0, self.max_x())
    }
    fn clamped_y(&self) -> f64 {
        self.scroll_y.clamp(0.0, self.max_y())
    }
    /// Overflow: how far past bounds (negative = past min, positive = past max).
    /// Returns 0 if the axis is disabled (no scrolling = no overscroll).
    fn overflow_x(&self) -> f64 {
        if self.scroll_x_enabled && self.max_x() > 0.0 {
            self.scroll_x - self.clamped_x()
        } else {
            0.0
        }
    }
    fn overflow_y(&self) -> f64 {
        if self.scroll_y_enabled && self.max_y() > 0.0 {
            self.scroll_y - self.clamped_y()
        } else {
            0.0
        }
    }
    /// Whether there is meaningful overflow on either axis.
    fn has_overflow(&self) -> bool {
        self.overflow_x().abs() > OVERSCROLL_SNAP_THRESHOLD
            || self.overflow_y().abs() > OVERSCROLL_SNAP_THRESHOLD
    }
}

/// Tracks the OS-level scroll gesture lifecycle.
///
/// On macOS, a trackpad scroll gesture produces two phases:
///   Started→Moved→Ended (user input) → Started→Moved→Ended (native momentum)
///
/// We use this to:
/// 1. Know when the finger is held (suppress spring snapback)
/// 2. Filter native momentum events (use our own synthetic momentum instead)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum GesturePhase {
    /// No active gesture.
    #[default]
    Idle,
    /// User's finger is on the trackpad (Started/Moved received).
    Touching,
    /// User lifted finger (first Ended received). Waiting to see if native
    /// momentum follows (Started) or if gesture is truly done.
    Released,
    /// OS is sending native momentum events (second Started after Released).
    /// Scroll events in this phase should be filtered out.
    NativeMomentum,
}

/// Resource tracking scroll physics state for all entities.
#[derive(Resource)]
pub struct ScrollPhysics {
    states: HashMap<Entity, EntityScrollState>,
    /// Current gesture phase, tracked from winit's raw TouchPhase events.
    gesture: GesturePhase,
    /// Whether the platform reliably delivers scroll touch phase events.
    /// On macOS this is hardcoded `true` (guaranteed by the OS). On other
    /// platforms it auto-detects: becomes true after we observe the first
    /// `Ended`/`Cancelled` phase, proving the platform completes gesture
    /// cycles. Until then, gesture tracking is ignored and we fall back to
    /// time-gap detection.
    phase_reliable: bool,
}

impl Default for ScrollPhysics {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            gesture: GesturePhase::Idle,
            phase_reliable: cfg!(target_os = "macos"),
        }
    }
}

impl ScrollPhysics {
    /// Whether the user's finger is currently on the trackpad.
    fn touch_held(&self) -> bool {
        self.phase_reliable && self.gesture == GesturePhase::Touching
    }

    /// Whether native OS momentum is active and should be filtered out.
    pub fn is_native_momentum(&self) -> bool {
        self.phase_reliable && self.gesture == GesturePhase::NativeMomentum
    }
}

/// A synthetic scroll event to be dispatched through the BYO event system.
pub struct SyntheticScroll {
    pub entity: Entity,
    pub delta_x: f64,
    pub delta_y: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
}

/// Per-entity overscroll visual offset (applied as UiTransform translation).
pub struct OverscrollUpdate {
    pub entity: Entity,
    pub offset_x: f64,
    pub offset_y: f64,
}

/// Per-entity clamped scroll position to write to Bevy's ScrollPosition.
pub struct ScrollPositionUpdate {
    pub entity: Entity,
    pub x: f64,
    pub y: f64,
}

/// Duration of inactivity (seconds) before transitioning from Active → Momentum.
const MOMENTUM_GAP_SECS: f64 = 0.05;

/// Velocity threshold below which momentum stops (px/s).
const VELOCITY_THRESHOLD: f64 = 10.0;

/// Per-frame velocity decay exponent (applied as `velocity *= DECAY^(dt*60)`).
/// 0.92 at 60fps means ~8% decay per frame — snappier stop than 0.95.
const MOMENTUM_DECAY: f64 = 0.92;

/// Velocity smoothing factor for incoming events (exponential moving average).
/// Lower = smoother velocity estimates, higher = more responsive.
const VELOCITY_SMOOTHING: f64 = 0.3;

/// Maximum time gap (seconds) between events before velocity is reset.
/// If more than this time has passed since the last scroll event, the
/// previous velocity is discarded (it's stale and irrelevant).
const STALE_EVENT_THRESHOLD: f64 = 1.0;

/// Overscroll damping constant for logarithmic resistance (pixels).
/// Controls how quickly resistance builds up. Higher = more forgiving pull.
/// `visual_offset = DAMPING * ln(1 + |overflow| / DAMPING)` — at small deltas
/// the response is nearly 1:1, at large accumulated overflow it asymptotes.
const OVERSCROLL_DAMPING: f64 = 200.0;

/// Spring constant for overscroll snapback (higher = faster return).
/// Only active after the user releases (Momentum/Idle phases).
const OVERSCROLL_SPRING: f64 = 12.0;

/// Overscroll offset below which we snap to zero (avoids endless springback).
const OVERSCROLL_SNAP_THRESHOLD: f64 = 0.5;

/// Input resistance factor for overscroll (pixels). When the user is scrolling
/// past bounds, each additional pixel of input produces less and less actual
/// movement: `effective = delta / (1 + |overflow| / RESISTANCE)`. At 0px
/// overflow the ratio is 1:1; at RESISTANCE px overflow it's 50%.
const OVERSCROLL_RESISTANCE: f64 = 100.0;

impl ScrollPhysics {
    /// Handle a winit `TouchPhase::Started` event.
    pub fn on_gesture_started(&mut self) {
        self.gesture = match self.gesture {
            GesturePhase::Idle => GesturePhase::Touching,
            GesturePhase::Released => GesturePhase::NativeMomentum,
            // Re-touch during momentum cancels it (new gesture)
            GesturePhase::NativeMomentum => GesturePhase::Touching,
            other => other,
        };
    }

    /// Handle a winit `TouchPhase::Moved` event.
    pub fn on_gesture_moved(&mut self) {
        // Moved doesn't change state — just confirms we're still in the
        // current phase (Touching or NativeMomentum).
    }

    /// Handle a winit `TouchPhase::Ended` or `Cancelled` event.
    pub fn on_gesture_ended(&mut self) {
        if !self.phase_reliable {
            // First completed cycle proves the platform delivers phases.
            self.phase_reliable = true;
        }
        self.gesture = match self.gesture {
            GesturePhase::Touching => GesturePhase::Released,
            GesturePhase::NativeMomentum => GesturePhase::Idle,
            other => other,
        };
    }

    /// Get the current clamped scroll position for an entity.
    pub fn clamped_position(&self, entity: Entity) -> (f64, f64) {
        self.states
            .get(&entity)
            .map(|s| (s.clamped_x(), s.clamped_y()))
            .unwrap_or((0.0, 0.0))
    }

    /// Get the current overscroll overflow for an entity.
    pub fn overflow(&self, entity: Entity) -> (f64, f64) {
        self.states
            .get(&entity)
            .map(|s| (s.overflow_x(), s.overflow_y()))
            .unwrap_or((0.0, 0.0))
    }

    /// Override physics state from controlled scroll props.
    /// Kills momentum and sets position to match the prop value on each axis.
    pub fn apply_controlled(&mut self, entity: Entity, sx: Option<f32>, sy: Option<f32>) {
        if let Some(state) = self.states.get_mut(&entity) {
            if let Some(sx) = sx {
                state.scroll_x = (sx as f64).clamp(0.0, state.max_x());
                state.velocity_x = 0.0;
            }
            if let Some(sy) = sy {
                state.scroll_y = (sy as f64).clamp(0.0, state.max_y());
                state.velocity_y = 0.0;
            }
        }
    }

    /// Record a raw scroll event from the compositor observer.
    #[allow(clippy::too_many_arguments)]
    pub fn on_raw_scroll(
        &mut self,
        entity: Entity,
        delta_x: f64,
        delta_y: f64,
        time: f64,
        content_width: f64,
        content_height: f64,
        viewport_width: f64,
        viewport_height: f64,
        cur_scroll_x: f64,
        cur_scroll_y: f64,
        scroll_x_enabled: bool,
        scroll_y_enabled: bool,
    ) {
        // Zero out deltas on disabled axes
        let delta_x = if scroll_x_enabled { delta_x } else { 0.0 };
        let delta_y = if scroll_y_enabled { delta_y } else { 0.0 };

        // If both axes are disabled, nothing to do
        if delta_x == 0.0 && delta_y == 0.0 && !self.states.contains_key(&entity) {
            return;
        }

        let is_new = !self.states.contains_key(&entity);
        let state = self.states.entry(entity).or_default();

        // Store axis configuration
        state.scroll_x_enabled = scroll_x_enabled;
        state.scroll_y_enabled = scroll_y_enabled;

        // Update dimensions first — needed for max_x/max_y clamping below.
        state.content_width = content_width;
        state.content_height = content_height;
        state.viewport_width = viewport_width;
        state.viewport_height = viewport_height;

        // Initialize scroll position from Bevy's actual value when state is
        // freshly created, so overscroll detection works correctly at any
        // scroll offset (not just the top). Clamp to valid range to avoid
        // massive overscroll if controlled scroll-y was set to a huge value
        // (e.g. 999999 to mean "scroll to bottom").
        if is_new {
            state.scroll_x = cur_scroll_x.clamp(0.0, state.max_x());
            state.scroll_y = cur_scroll_y.clamp(0.0, state.max_y());
        }

        // Cancel any active momentum — user re-engaged
        if state.phase == ScrollPhase::Momentum {
            state.momentum_active = false;
        }

        let dt = time - state.last_event_time;
        let is_stale = dt > STALE_EVENT_THRESHOLD;

        if is_stale {
            state.velocity_x = 0.0;
            state.velocity_y = 0.0;
        }

        let dt = dt.max(1.0 / 240.0);
        let instant_vx = delta_x / dt;
        let instant_vy = delta_y / dt;

        if is_stale {
            state.velocity_x = instant_vx;
            state.velocity_y = instant_vy;
        } else {
            state.velocity_x =
                state.velocity_x * (1.0 - VELOCITY_SMOOTHING) + instant_vx * VELOCITY_SMOOTHING;
            state.velocity_y =
                state.velocity_y * (1.0 - VELOCITY_SMOOTHING) + instant_vy * VELOCITY_SMOOTHING;
        }

        state.last_event_time = time;
        state.phase = ScrollPhase::Active;

        // Apply overscroll resistance: dampen deltas when past bounds.
        // Only dampen the component pushing *further* past bounds.
        let effective_dx = dampen_overscroll_delta(delta_x, state.overflow_x());
        let effective_dy = dampen_overscroll_delta(delta_y, state.overflow_y());

        // Unclamped scroll — overscroll is just scroll going past [0, max].
        // Negate: Bevy delta_y < 0 = scroll down, but ScrollPosition increases downward.
        state.scroll_x -= effective_dx;
        state.scroll_y -= effective_dy;
    }

    /// Tick the scroll physics for all entities. Returns synthetic scroll events
    /// that should be dispatched through the BYO event system.
    pub fn tick(&mut self, time: f64, dt: f64) -> Vec<SyntheticScroll> {
        let mut events = Vec::new();
        let mut to_remove = Vec::new();
        let touch_held = self.touch_held();

        for (&entity, state) in &mut self.states {
            match state.phase {
                ScrollPhase::Active => {
                    // No spring during Active — overscroll accumulates freely
                    // while the user is pulling.
                    let gap = time - state.last_event_time;
                    // Don't transition out of Active while the user's finger
                    // is on the trackpad — the gap is just a pause in gesture,
                    // not a release.
                    if gap > MOMENTUM_GAP_SECS && !touch_held {
                        if gap > STALE_EVENT_THRESHOLD {
                            state.phase = ScrollPhase::Idle;
                            state.velocity_x = 0.0;
                            state.velocity_y = 0.0;
                            if !state.has_overflow() {
                                to_remove.push(entity);
                            }
                        } else {
                            let speed = (state.velocity_x * state.velocity_x
                                + state.velocity_y * state.velocity_y)
                                .sqrt();
                            if speed > VELOCITY_THRESHOLD {
                                state.phase = ScrollPhase::Momentum;
                                state.momentum_active = true;
                            } else {
                                state.phase = ScrollPhase::Idle;
                                state.velocity_x = 0.0;
                                state.velocity_y = 0.0;
                                if !state.has_overflow() {
                                    to_remove.push(entity);
                                }
                            }
                        }
                    }
                }
                ScrollPhase::Momentum => {
                    let decay = MOMENTUM_DECAY.powf(dt * 60.0);
                    state.velocity_x *= decay;
                    state.velocity_y *= decay;

                    // Enforce axis locking on momentum velocity
                    if !state.scroll_x_enabled {
                        state.velocity_x = 0.0;
                    }
                    if !state.scroll_y_enabled {
                        state.velocity_y = 0.0;
                    }

                    let speed = (state.velocity_x * state.velocity_x
                        + state.velocity_y * state.velocity_y)
                        .sqrt();

                    if speed < VELOCITY_THRESHOLD {
                        state.phase = ScrollPhase::Idle;
                        state.momentum_active = false;
                        state.velocity_x = 0.0;
                        state.velocity_y = 0.0;
                        if !state.has_overflow() {
                            to_remove.push(entity);
                        }
                    } else {
                        let delta_x = state.velocity_x * dt;
                        let delta_y = state.velocity_y * dt;
                        // Apply momentum delta (negated, same convention as on_raw_scroll)
                        state.scroll_x -= delta_x;
                        state.scroll_y -= delta_y;
                        events.push(SyntheticScroll {
                            entity,
                            delta_x,
                            delta_y,
                            content_width: state.content_width,
                            content_height: state.content_height,
                            viewport_width: state.viewport_width,
                            viewport_height: state.viewport_height,
                        });
                    }

                    // Spring scroll back toward [0, max] after release
                    spring_toward_bounds(state, dt);
                }
                ScrollPhase::Idle => {
                    // Spring scroll back toward [0, max]
                    spring_toward_bounds(state, dt);

                    if !state.has_overflow() {
                        // Snap exactly to bounds
                        state.scroll_x = state.clamped_x();
                        state.scroll_y = state.clamped_y();
                        to_remove.push(entity);
                    }
                }
            }
        }

        for entity in to_remove {
            self.states.remove(&entity);
        }

        // When all scroll states are cleaned up, reset Released gesture to
        // Idle. The Released state waits for native momentum (Started), but
        // if our own synthetic momentum ran to completion instead, there's
        // nothing left to track.
        if self.states.is_empty() && self.gesture == GesturePhase::Released {
            self.gesture = GesturePhase::Idle;
        }

        events
    }

    /// Get the clamped scroll positions to write to Bevy's ScrollPosition.
    pub fn scroll_position_updates(&self) -> Vec<ScrollPositionUpdate> {
        self.states
            .iter()
            .map(|(&entity, s)| ScrollPositionUpdate {
                entity,
                x: s.clamped_x(),
                y: s.clamped_y(),
            })
            .collect()
    }

    /// Get the current visual overscroll offsets for all active entities.
    /// Converts overflow to visual offset via logarithmic damping.
    /// Always returns all entities in physics.states so that OverscrollState
    /// is kept in sync — even when overflow is near zero, we must write (0,0)
    /// to clear any previous non-zero offset.
    pub fn overscroll_updates(&self) -> Vec<OverscrollUpdate> {
        self.states
            .iter()
            .map(|(&entity, s)| OverscrollUpdate {
                entity,
                // Negate: overflow < 0 (past top) → translate content DOWN (positive)
                offset_x: -rubber_band(s.overflow_x()),
                offset_y: -rubber_band(s.overflow_y()),
            })
            .collect()
    }

    /// Whether any entity has active momentum or overscroll (used for RequestRedraw).
    #[allow(dead_code)]
    pub fn has_active_momentum(&self) -> bool {
        self.states
            .values()
            .any(|s| s.phase == ScrollPhase::Momentum || s.has_overflow())
    }
}

/// Dampen a scroll delta when already in overscroll. If the delta would push
/// *further* past bounds, it's reduced by `1 / (1 + |overflow| / RESISTANCE)`.
/// If the delta is pulling *back* toward bounds (reducing overflow), it passes
/// through unmodified so the user can easily return.
fn dampen_overscroll_delta(delta: f64, overflow: f64) -> f64 {
    if overflow.abs() < 0.01 {
        return delta;
    }
    // Delta is negated before applying to scroll: `scroll -= delta`.
    // So `delta > 0` means scroll decreases (scroll up), `delta < 0` means scroll increases (scroll down).
    // Overflow < 0 = past top, overflow > 0 = past bottom.
    // "Pushing further" = overflow < 0 && delta > 0 (scrolling up past top)
    //                    = overflow > 0 && delta < 0 (scrolling down past bottom)
    let pushing_further = (overflow < 0.0 && delta > 0.0) || (overflow > 0.0 && delta < 0.0);
    if pushing_further {
        delta / (1.0 + overflow.abs() / OVERSCROLL_RESISTANCE)
    } else {
        delta
    }
}

/// Logarithmic rubberband: converts raw overflow (px) to visual offset (px).
/// At small overflows the response is nearly 1:1; at large overflows it
/// asymptotes toward `OVERSCROLL_DAMPING`. Sign is preserved.
fn rubber_band(overflow: f64) -> f64 {
    if overflow.abs() < 0.01 {
        return 0.0;
    }
    let sign = overflow.signum();
    sign * OVERSCROLL_DAMPING * (1.0 + overflow.abs() / OVERSCROLL_DAMPING).ln()
}

/// Spring `scroll_x/y` back toward `[0, max]` bounds (exponential decay of overflow).
fn spring_toward_bounds(state: &mut EntityScrollState, dt: f64) {
    let factor = (-OVERSCROLL_SPRING * dt).exp();

    let overflow_x = state.overflow_x();
    if overflow_x.abs() > OVERSCROLL_SNAP_THRESHOLD {
        // Decay the overflow portion, keep the clamped portion
        state.scroll_x = state.clamped_x() + overflow_x * factor;
    } else if overflow_x.abs() > 0.0 {
        // Snap to bounds
        state.scroll_x = state.clamped_x();
    }

    let overflow_y = state.overflow_y();
    if overflow_y.abs() > OVERSCROLL_SNAP_THRESHOLD {
        state.scroll_y = state.clamped_y() + overflow_y * factor;
    } else if overflow_y.abs() > 0.0 {
        state.scroll_y = state.clamped_y();
    }
}

// ---------------------------------------------------------------------------
// Bevy system: tick scroll physics and dispatch synthetic events
// ---------------------------------------------------------------------------

use crate::events::config::EventSubscriptions;
use crate::events::engine::{EngineHandle, EngineInput, PointerData, SpineNode};
use crate::events::observers::{compute_element_size, find_scroll_event_target};
use crate::id_map::IdMap;
use bevy::ui::ScrollPosition;
use bevy::window::RequestRedraw;
use bevy::winit::RawWinitWindowEvent;
use byo::protocol::EventKind;

/// ECS component that tracks the current overscroll visual offset for an entity.
/// Applied as a `UiTransform` translation offset in the style reconciliation.
#[derive(Component, Debug, Default)]
pub struct OverscrollState {
    pub offset_x: f32,
    pub offset_y: f32,
}

/// System that ticks scroll physics each frame and dispatches synthetic
/// momentum scroll events through the propagation engine.
///
/// During active momentum, writes `RequestRedraw` to keep Bevy's reactive
/// loop updating.
#[allow(clippy::too_many_arguments)]
pub fn tick_scroll_physics(
    mut commands: Commands,
    mut physics: ResMut<ScrollPhysics>,
    time: Res<Time>,
    engine: Res<EngineHandle>,
    id_map: Res<IdMap>,
    node_query: Query<&ComputedNode>,
    subs_query: Query<&EventSubscriptions>,
    parent_query: Query<&ChildOf>,
    scroll_query: Query<(Entity, &ScrollPosition), With<Node>>,
    mut overscroll_query: Query<&mut OverscrollState>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64();

    if dt <= 0.0 {
        return;
    }

    // No sync from Bevy — our scroll_x/y is the source of truth.
    let synthetic_events = physics.tick(now, dt);

    // Keep Bevy's reactive loop updating while physics are active
    // (momentum decay, overscroll springback, etc.)
    if !physics.states.is_empty() {
        redraw.write(RequestRedraw);
    }

    // Apply overscroll offsets to ECS components
    for update in physics.overscroll_updates() {
        if let Ok(mut state) = overscroll_query.get_mut(update.entity) {
            state.offset_x = update.offset_x as f32;
            state.offset_y = update.offset_y as f32;
        } else {
            commands.entity(update.entity).insert(OverscrollState {
                offset_x: update.offset_x as f32,
                offset_y: update.offset_y as f32,
            });
        }
    }

    // Clear overscroll on entities that no longer have active overscroll
    for (entity, _) in &scroll_query {
        if !physics.states.contains_key(&entity)
            && let Ok(mut state) = overscroll_query.get_mut(entity)
            && (state.offset_x != 0.0 || state.offset_y != 0.0)
        {
            state.offset_x = 0.0;
            state.offset_y = 0.0;
        }
    }

    for event in synthetic_events {
        // The event.entity is the viewport (has ScrollPosition and events="scroll").
        let Some(event_target) = find_scroll_event_target(event.entity, &parent_query, &subs_query)
        else {
            continue;
        };

        let Some(target_byo_id) = id_map.get_id(event_target) else {
            continue;
        };

        let (w, h) = compute_element_size(event_target, &node_query);

        // Read authoritative clamped position and overflow (compositor owns scroll state)
        let (scroll_x, scroll_y) = physics.clamped_position(event.entity);
        let (overflow_x, overflow_y) = physics.overflow(event.entity);

        let mut pointer = PointerData::mouse(0.0, 0.0, true);
        pointer.scroll_delta_x = event.delta_x;
        pointer.scroll_delta_y = event.delta_y;
        pointer.content_width = event.content_width;
        pointer.content_height = event.content_height;
        pointer.viewport_width = event.viewport_width;
        pointer.viewport_height = event.viewport_height;
        pointer.scroll_x = scroll_x;
        pointer.scroll_y = scroll_y;
        pointer.scroll_overflow_x = overflow_x;
        pointer.scroll_overflow_y = overflow_y;
        pointer.target = target_byo_id.to_string();
        pointer.target_width = w;
        pointer.target_height = h;

        let spine = vec![SpineNode::direct(target_byo_id.to_string(), 0.0, 0.0, w, h)];

        engine.send(EngineInput::NewEvent {
            kind: EventKind::Scroll,
            pointer: Box::new(pointer),
            spine,
        });
    }
}

/// System that reads raw winit events to track scroll gesture phase.
///
/// On macOS, a trackpad scroll produces two TouchPhase cycles:
///   Started→Moved→Ended (user input) then Started→Moved→Ended (native momentum)
///
/// We track a state machine to:
/// 1. Know when the finger is held (suppress spring snapback)
/// 2. Detect native momentum (filter it out, use our own synthetic momentum)
pub fn track_scroll_touch_phase(
    mut raw_events: MessageReader<RawWinitWindowEvent>,
    mut physics: ResMut<ScrollPhysics>,
) {
    use winit::event::WindowEvent;

    for raw in raw_events.read() {
        if let WindowEvent::MouseWheel { phase, .. } = &raw.event {
            match phase {
                winit::event::TouchPhase::Started => physics.on_gesture_started(),
                winit::event::TouchPhase::Moved => physics.on_gesture_moved(),
                winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
                    physics.on_gesture_ended();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_scroll_sets_active_phase() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        physics.on_raw_scroll(
            entity, 0.0, -20.0, 1.0, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
        );
        assert!(physics.states.contains_key(&entity));
        assert_eq!(physics.states[&entity].phase, ScrollPhase::Active);
    }

    #[test]
    fn momentum_starts_after_gap() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        // Send a burst of scroll events to build velocity
        for i in 0..5 {
            let t = 1.0 + i as f64 * 0.016;
            physics.on_raw_scroll(
                entity, 0.0, -20.0, t, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
            );
        }
        // Tick after the gap
        let events = physics.tick(1.0 + 5.0 * 0.016 + MOMENTUM_GAP_SECS + 0.01, 0.016);
        // Should have transitioned to Momentum
        assert_eq!(physics.states[&entity].phase, ScrollPhase::Momentum);
        // No events yet on the tick that starts momentum; events come next tick
        // (the phase transition happened, next tick will produce synthetic events)
        let _ = events;
    }

    #[test]
    fn momentum_produces_synthetic_events() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        for i in 0..5 {
            let t = 1.0 + i as f64 * 0.016;
            physics.on_raw_scroll(
                entity, 0.0, -30.0, t, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
            );
        }
        let base_time = 1.0 + 5.0 * 0.016;
        // Trigger Active → Momentum transition
        physics.tick(base_time + MOMENTUM_GAP_SECS + 0.01, 0.016);
        // Now tick during momentum
        let events = physics.tick(base_time + MOMENTUM_GAP_SECS + 0.026, 0.016);
        assert!(!events.is_empty(), "should produce synthetic scroll events");
        assert!(events[0].delta_y.abs() > 0.0);
    }

    #[test]
    fn new_raw_event_cancels_momentum() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        for i in 0..5 {
            let t = 1.0 + i as f64 * 0.016;
            physics.on_raw_scroll(
                entity, 0.0, -30.0, t, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
            );
        }
        let base_time = 1.0 + 5.0 * 0.016;
        physics.tick(base_time + MOMENTUM_GAP_SECS + 0.01, 0.016);
        assert_eq!(physics.states[&entity].phase, ScrollPhase::Momentum);

        // New raw event arrives — should cancel momentum
        physics.on_raw_scroll(
            entity,
            0.0,
            -10.0,
            base_time + 0.1,
            0.0,
            1000.0,
            0.0,
            500.0,
            0.0,
            0.0,
            true,
            true,
        );
        assert_eq!(physics.states[&entity].phase, ScrollPhase::Active);
    }

    #[test]
    fn momentum_eventually_stops() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        physics.on_raw_scroll(
            entity, 0.0, -5.0, 1.0, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
        );
        let base_time = 1.0;
        // Trigger momentum
        physics.tick(base_time + MOMENTUM_GAP_SECS + 0.01, 0.016);
        // Tick many frames
        let mut t = base_time + 0.1;
        for _ in 0..500 {
            physics.tick(t, 0.016);
            t += 0.016;
        }
        // Should have cleaned up
        assert!(physics.states.is_empty());
    }

    #[test]
    fn has_active_momentum_reflects_state() {
        let mut physics = ScrollPhysics::default();
        assert!(!physics.has_active_momentum());

        let entity = Entity::from_bits(1);
        // Scroll down (negative delta → scroll_y increases, within bounds of large content)
        for i in 0..5 {
            let t = 1.0 + i as f64 * 0.016;
            physics.on_raw_scroll(
                entity, 0.0, -30.0, t, 0.0, 10000.0, 0.0, 500.0, 0.0, 0.0, true, true,
            );
        }
        assert!(!physics.has_active_momentum()); // still Active, not Momentum

        let base_time = 1.0 + 5.0 * 0.016;
        physics.tick(base_time + MOMENTUM_GAP_SECS + 0.01, 0.016);
        assert!(physics.has_active_momentum());
    }

    #[test]
    fn overscroll_accumulates_at_bounds() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        // Scroll up (positive delta) at top → scroll_y = 0 - 20 = -20
        // clamped = 0, overflow = -20
        physics.on_raw_scroll(
            entity, 0.0, 20.0, 1.0, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
        );
        let state = &physics.states[&entity];
        assert_eq!(state.scroll_y, -20.0); // unclamped, past min
        assert_eq!(state.clamped_y(), 0.0);
        assert_eq!(state.overflow_y(), -20.0);
    }

    #[test]
    fn overscroll_springs_back() {
        let mut physics = ScrollPhysics::default();
        let entity = Entity::from_bits(1);
        // Scroll up (positive delta) at top → scroll_y = 0 - 50 = -50
        physics.on_raw_scroll(
            entity, 0.0, 50.0, 1.0, 0.0, 1000.0, 0.0, 500.0, 0.0, 0.0, true, true,
        );
        let initial = physics.states[&entity].overflow_y();
        assert!(initial < 0.0);

        // Tick in idle until overscroll springs back
        physics.states.get_mut(&entity).unwrap().phase = ScrollPhase::Idle;
        let mut t = 1.1;
        for _ in 0..100 {
            physics.tick(t, 0.016);
            t += 0.016;
        }
        // Should have sprung back and been cleaned up
        assert!(physics.states.is_empty() || physics.states[&entity].overflow_y().abs() < 0.5);
    }
}
