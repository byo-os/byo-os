//! Propagation engine — dedicated thread driving capture/bubble event dispatch.
//!
//! The engine receives new pointer events (with pre-built spines) from ECS
//! and ACKs from the stdin reader thread. It drives the W3C-style propagation
//! model: capture phase (root → leaf), then bubble phase (leaf → root),
//! emitting events one at a time and waiting for ACKs before proceeding.
//!
//! Non-bubbling events (pointerenter, pointerleave, focus, blur) are emitted
//! directly without propagation.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use byo::protocol::{EventKind, Prop};

use super::config::Phase;
use crate::io::StdoutEmitter;

/// Timeout for waiting for an ACK before moving to the next spine node.
const ACK_TIMEOUT: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Public types shared across threads
// ---------------------------------------------------------------------------

/// Pointer type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerType {
    Mouse,
    #[allow(dead_code)]
    Pen,
    Touch,
}

impl PointerType {
    pub fn as_str(self) -> &'static str {
        match self {
            PointerType::Mouse => "mouse",
            PointerType::Pen => "pen",
            PointerType::Touch => "touch",
        }
    }
}

/// Pointer button identifier (matches W3C spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// Primary (left click, touch contact)
    Primary,
    /// Middle (wheel click)
    Middle,
    /// Secondary (right click)
    Secondary,
}

impl Button {
    /// Wire format value: 0=primary, 1=middle, 2=secondary.
    pub fn wire_value(self) -> i8 {
        match self {
            Button::Primary => 0,
            Button::Middle => 1,
            Button::Secondary => 2,
        }
    }

    /// Bitmask bit for this button.
    pub fn bitmask(self) -> u16 {
        match self {
            Button::Primary => 1,
            Button::Middle => 4,
            Button::Secondary => 2,
        }
    }
}

/// Modifier key state at the time of the event.
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Full pointer event data, collected from Bevy's picking system.
#[derive(Debug, Clone)]
pub struct PointerData {
    pub pointer_id: i64,
    pub pointer_type: PointerType,
    /// Position relative to the compositor viewport (logical px).
    pub client_x: f64,
    pub client_y: f64,
    /// Button that changed state (-1 for move events).
    pub button: i8,
    /// Bitmask of currently pressed buttons.
    pub buttons: u16,
    /// Contact geometry (logical px, default 1.0).
    pub width: f64,
    pub height: f64,
    /// Pressure (0.0–1.0, mouse default: 0.5 when pressed).
    pub pressure: f32,
    pub tangential_pressure: f32,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub twist: u32,
    /// Altitude angle in radians (π/2 = perpendicular).
    pub altitude: f64,
    /// Azimuth angle in radians.
    pub azimuth: f64,
    /// Is this the primary pointer of its type?
    pub primary: bool,
    pub modifiers: Modifiers,
}

impl PointerData {
    /// Create a default mouse pointer event data.
    pub fn mouse(client_x: f64, client_y: f64, primary: bool) -> Self {
        Self {
            pointer_id: 1,
            pointer_type: PointerType::Mouse,
            client_x,
            client_y,
            button: -1,
            buttons: 0,
            width: 1.0,
            height: 1.0,
            pressure: 0.0,
            tangential_pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            twist: 0,
            altitude: std::f64::consts::FRAC_PI_2,
            azimuth: 0.0,
            primary,
            modifiers: Modifiers::default(),
        }
    }
}

/// A node in the propagation spine (pre-built by the ECS system).
#[derive(Debug, Clone)]
pub struct SpineNode {
    /// BYO qualified ID for this entity.
    pub byo_id: String,
    /// Which phases this node subscribes to for this event type.
    pub phase: Phase,
    /// Whether this subscription is passive (won't ACK handled=true).
    pub passive: bool,
    /// Whether to emit all props (verbose mode).
    pub verbose: bool,
    /// Position relative to this element's origin (logical px).
    pub local_x: f64,
    pub local_y: f64,
}

/// Messages sent to the propagation engine.
pub enum EngineInput {
    /// A new pointer event from Bevy's picking system.
    NewEvent {
        kind: EventKind,
        pointer: PointerData,
        /// Spine ordered root → leaf.
        spine: Vec<SpineNode>,
    },
    /// ACK received from the stdin reader thread.
    Ack {
        kind: EventKind,
        seq: u64,
        handled: bool,
        capture: bool,
    },
    /// Shutdown the engine thread.
    #[allow(dead_code)]
    Shutdown,
}

/// Handle for sending messages to the propagation engine.
#[derive(Resource, Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<EngineInput>,
}

impl EngineHandle {
    /// Submit a new pointer event for propagation.
    pub fn send(&self, input: EngineInput) {
        let _ = self.tx.send(input);
    }
}

// ---------------------------------------------------------------------------
// Engine internals
// ---------------------------------------------------------------------------

/// Per-event-type sequence counter.
#[derive(Default)]
struct SeqCounters {
    counters: HashMap<String, u64>,
}

impl SeqCounters {
    fn next(&mut self, kind: &EventKind) -> u64 {
        let counter = self.counters.entry(kind.as_str().to_string()).or_insert(0);
        let seq = *counter;
        *counter += 1;
        seq
    }
}

/// State of an in-flight propagation.
struct InFlightPropagation {
    kind: EventKind,
    pointer: PointerData,
    /// Full spine (root → leaf). Only nodes relevant to this event type.
    spine: Vec<SpineNode>,
    /// Current dispatch order: capture nodes then bubble nodes.
    dispatch_order: Vec<usize>,
    /// Current index into `dispatch_order`.
    current_step: usize,
    /// Unique in-flight ID for this propagation (avoids cross-type seq collisions).
    inflight_id: u64,
    /// The ack_map key for the currently outstanding event (for timeout cleanup).
    pending_ack_key: Option<(String, u64)>,
    /// When we sent the current event (for timeout).
    sent_at: Option<Instant>,
    /// Whether propagation was stopped.
    stopped: bool,
}

/// Tracks a pointer press to derive click/dblclick on release.
struct PressState {
    /// Which button was pressed.
    button: i8,
    /// When the press occurred.
    pressed_at: Instant,
    /// Client coordinates at press (for distance threshold).
    client_x: f64,
    client_y: f64,
}

/// Tracks recent clicks for dblclick derivation.
struct ClickState {
    /// When the last click occurred.
    clicked_at: Instant,
    /// Client coordinates of the last click.
    client_x: f64,
    client_y: f64,
}

/// Maximum distance (logical px) pointer can move between press and release to count as click.
const CLICK_DISTANCE_THRESHOLD: f64 = 10.0;
/// Maximum time between press and release to count as click.
const CLICK_TIME_THRESHOLD: Duration = Duration::from_millis(500);
/// Maximum time between two clicks for dblclick.
const DBLCLICK_TIME_THRESHOLD: Duration = Duration::from_millis(300);
/// Maximum distance between two clicks for dblclick.
const DBLCLICK_DISTANCE_THRESHOLD: f64 = 10.0;

/// The propagation engine state.
struct Engine {
    emitter: StdoutEmitter,
    seq: SeqCounters,
    /// Monotonic counter for unique in-flight propagation IDs.
    next_inflight_id: u64,
    /// In-flight propagations keyed by unique inflight_id.
    in_flight: HashMap<u64, InFlightPropagation>,
    /// Map from (event_kind_str, wire_seq) to the inflight_id.
    ack_map: HashMap<(String, u64), u64>,
    /// Pointer capture: pointer_id → target BYO ID.
    captures: HashMap<i64, String>,
    /// Active press states per pointer ID.
    press_states: HashMap<i64, PressState>,
    /// Recent click states per (pointer_id, target_id) for dblclick derivation.
    click_states: HashMap<(i64, String), ClickState>,
    /// Deferred events to dispatch after current propagation completes.
    deferred: Vec<(EventKind, PointerData, Vec<SpineNode>)>,
}

impl Engine {
    fn new(emitter: StdoutEmitter) -> Self {
        Self {
            emitter,
            seq: SeqCounters::default(),
            next_inflight_id: 0,
            in_flight: HashMap::new(),
            ack_map: HashMap::new(),
            captures: HashMap::new(),
            press_states: HashMap::new(),
            click_states: HashMap::new(),
            deferred: Vec::new(),
        }
    }

    fn handle_new_event(&mut self, kind: EventKind, pointer: PointerData, spine: Vec<SpineNode>) {
        if !kind.bubbles() {
            // Non-bubbling events (pointerenter, pointerleave, focus, blur):
            // fire directly on each subscribed spine node, no propagation.
            self.emit_non_bubbling(&kind, &pointer, &spine);
            return;
        }

        if spine.is_empty() {
            return;
        }

        // Track press state for click derivation
        if kind == EventKind::PointerDown && !spine.is_empty() {
            self.press_states.insert(
                pointer.pointer_id,
                PressState {
                    button: pointer.button,
                    pressed_at: Instant::now(),
                    client_x: pointer.client_x,
                    client_y: pointer.client_y,
                },
            );
        }

        // Check pointer capture: if captured, override spine to just the target
        let effective_spine = if let Some(capture_id) = self.captures.get(&pointer.pointer_id) {
            // Find the capture target in the spine, or use it alone
            if let Some(node) = spine.iter().find(|n| n.byo_id == *capture_id) {
                vec![node.clone()]
            } else {
                // Capture target not in spine — still deliver to it
                vec![SpineNode {
                    byo_id: capture_id.clone(),
                    phase: Phase::Bubble,
                    passive: false,
                    verbose: false,
                    local_x: pointer.client_x,
                    local_y: pointer.client_y,
                }]
            }
        } else {
            spine
        };

        // Build dispatch order: capture phase (root→leaf), then bubble phase (leaf→root)
        let mut dispatch_order = Vec::new();

        // Capture phase: root → leaf
        for (i, node) in effective_spine.iter().enumerate() {
            if node.phase.includes_capture() {
                dispatch_order.push(i);
            }
        }

        // Bubble phase: leaf → root
        for (i, node) in effective_spine.iter().enumerate().rev() {
            if node.phase.includes_bubble() {
                dispatch_order.push(i);
            }
        }

        if dispatch_order.is_empty() {
            // Even with no subscribers, derive click from pointerup
            if kind == EventKind::PointerUp {
                let inflight_id = self.next_inflight_id;
                self.next_inflight_id += 1;
                let prop = InFlightPropagation {
                    kind,
                    pointer,
                    spine: effective_spine,
                    dispatch_order: Vec::new(),
                    current_step: 0,
                    inflight_id,
                    pending_ack_key: None,
                    sent_at: None,
                    stopped: false,
                };
                self.on_propagation_complete(&prop);
                self.drain_deferred();
            }
            return;
        }

        let inflight_id = self.next_inflight_id;
        self.next_inflight_id += 1;

        let mut propagation = InFlightPropagation {
            kind,
            pointer,
            spine: effective_spine,
            dispatch_order,
            current_step: 0,
            inflight_id,
            pending_ack_key: None,
            sent_at: None,
            stopped: false,
        };

        // Start dispatching
        self.dispatch_next(&mut propagation);

        if !propagation.stopped && propagation.current_step < propagation.dispatch_order.len() {
            // Still in-flight — store it keyed by unique inflight_id
            self.in_flight.insert(inflight_id, propagation);
        } else {
            // Propagation completed immediately (all passive, or no non-passive subscribers)
            self.on_propagation_complete(&propagation);
            self.drain_deferred();
        }
    }

    fn dispatch_next(&mut self, prop: &mut InFlightPropagation) {
        if prop.stopped || prop.current_step >= prop.dispatch_order.len() {
            return;
        }

        let spine_idx = prop.dispatch_order[prop.current_step];
        let node = &prop.spine[spine_idx];
        let seq = self.seq.next(&prop.kind);

        // Build props
        let props = build_event_props(&prop.pointer, node, node.verbose);

        // Emit the event
        let kind_str = prop.kind.as_str().to_string();
        let id = node.byo_id.clone();
        self.emitter
            .frame(|em| em.event(&kind_str, seq, &id, &props));

        // Track: ack_map maps (kind, wire_seq) → inflight_id
        let ack_key = (kind_str, seq);
        prop.pending_ack_key = Some(ack_key.clone());
        prop.sent_at = Some(Instant::now());
        self.ack_map.insert(ack_key, prop.inflight_id);

        if node.passive {
            // Passive: don't wait for ACK, advance immediately
            prop.current_step += 1;
            self.dispatch_next(prop);
        }
    }

    fn handle_ack(&mut self, kind: EventKind, seq: u64, handled: bool, capture: bool) {
        let ack_key = (kind.as_str().to_string(), seq);
        let Some(inflight_id) = self.ack_map.remove(&ack_key) else {
            return;
        };

        let Some(mut prop) = self.in_flight.remove(&inflight_id) else {
            return;
        };

        // Handle pointer capture request
        if capture {
            let spine_idx = prop.dispatch_order[prop.current_step];
            let node = &prop.spine[spine_idx];
            self.captures
                .insert(prop.pointer.pointer_id, node.byo_id.clone());

            // Emit gotpointercapture
            let cap_seq = self.seq.next(&EventKind::GotPointerCapture);
            let id = node.byo_id.clone();
            self.emitter.frame(|em| {
                em.event(
                    "gotpointercapture",
                    cap_seq,
                    &id,
                    &[Prop::val("pointer-id", prop.pointer.pointer_id.to_string())],
                )
            });
        }

        if handled {
            // Stop propagation
            prop.stopped = true;
            self.on_propagation_complete(&prop);
            self.drain_deferred();
            return;
        }

        // Advance to next step
        prop.current_step += 1;
        self.dispatch_next(&mut prop);

        if !prop.stopped && prop.current_step < prop.dispatch_order.len() {
            self.in_flight.insert(prop.inflight_id, prop);
        } else {
            // Propagation complete
            self.on_propagation_complete(&prop);
            self.drain_deferred();
        }
    }

    fn emit_non_bubbling(&mut self, kind: &EventKind, pointer: &PointerData, spine: &[SpineNode]) {
        // For non-bubbling events, just fire on each subscribed node directly
        for node in spine {
            let seq = self.seq.next(kind);
            let props = build_event_props(pointer, node, node.verbose);
            let kind_str = kind.as_str().to_string();
            let id = node.byo_id.clone();
            self.emitter
                .frame(|em| em.event(&kind_str, seq, &id, &props));
        }
    }

    /// Check for timed-out in-flight propagations and advance them.
    fn check_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<u64> = self
            .in_flight
            .iter()
            .filter(|(_, prop)| {
                prop.sent_at
                    .is_some_and(|sent| now.duration_since(sent) > ACK_TIMEOUT)
            })
            .map(|(k, _)| *k)
            .collect();

        for inflight_id in timed_out {
            if let Some(mut prop) = self.in_flight.remove(&inflight_id) {
                // Remove from ack_map
                if let Some(ref ack_key) = prop.pending_ack_key {
                    self.ack_map.remove(ack_key);
                }

                warn!(
                    "ACK timeout for {} on {}",
                    prop.kind.as_str(),
                    prop.spine
                        .get(*prop.dispatch_order.get(prop.current_step).unwrap_or(&0))
                        .map(|n| n.byo_id.as_str())
                        .unwrap_or("?")
                );

                // Advance past the timed-out step
                prop.current_step += 1;
                self.dispatch_next(&mut prop);

                if !prop.stopped && prop.current_step < prop.dispatch_order.len() {
                    self.in_flight.insert(prop.inflight_id, prop);
                } else {
                    // Propagation complete (after timeout)
                    self.on_propagation_complete(&prop);
                }
            }
        }

        // Drain any deferred events from completions
        self.drain_deferred();
    }

    /// Called when an in-flight propagation completes (all steps done or stopped).
    /// Derives click/auxclick/dblclick events from pointerup completions.
    fn on_propagation_complete(&mut self, prop: &InFlightPropagation) {
        if prop.kind != EventKind::PointerUp {
            return;
        }

        let Some(press) = self.press_states.remove(&prop.pointer.pointer_id) else {
            return;
        };

        // Check distance and time thresholds
        let dx = prop.pointer.client_x - press.client_x;
        let dy = prop.pointer.client_y - press.client_y;
        let dist = (dx * dx + dy * dy).sqrt();
        let elapsed = press.pressed_at.elapsed();

        if dist > CLICK_DISTANCE_THRESHOLD || elapsed > CLICK_TIME_THRESHOLD {
            return;
        }

        // Derive click kind based on button
        let click_kind = if press.button == 0 {
            EventKind::Click
        } else {
            EventKind::AuxClick
        };

        // Use the pointerup's spine — click bubbles through the same path
        self.deferred
            .push((click_kind, prop.pointer.clone(), prop.spine.clone()));

        // Check for dblclick (only primary button)
        if press.button == 0 {
            let target_id = prop
                .spine
                .last()
                .map(|n| n.byo_id.clone())
                .unwrap_or_default();
            let key = (prop.pointer.pointer_id, target_id);

            if let Some(prev_click) = self.click_states.get(&key) {
                let click_dx = prop.pointer.client_x - prev_click.client_x;
                let click_dy = prop.pointer.client_y - prev_click.client_y;
                let click_dist = (click_dx * click_dx + click_dy * click_dy).sqrt();
                let click_elapsed = prev_click.clicked_at.elapsed();

                if click_dist <= DBLCLICK_DISTANCE_THRESHOLD
                    && click_elapsed <= DBLCLICK_TIME_THRESHOLD
                {
                    self.deferred.push((
                        EventKind::DblClick,
                        prop.pointer.clone(),
                        prop.spine.clone(),
                    ));
                    // Reset — next click starts fresh
                    self.click_states.remove(&key);
                    return;
                }
            }

            // Record this click for future dblclick detection
            let key = (
                prop.pointer.pointer_id,
                prop.spine
                    .last()
                    .map(|n| n.byo_id.clone())
                    .unwrap_or_default(),
            );
            self.click_states.insert(
                key,
                ClickState {
                    clicked_at: Instant::now(),
                    client_x: prop.pointer.client_x,
                    client_y: prop.pointer.client_y,
                },
            );
        }
    }

    /// Drain and dispatch any deferred events (click/dblclick derived from pointerup).
    fn drain_deferred(&mut self) {
        let deferred = std::mem::take(&mut self.deferred);
        for (kind, pointer, spine) in deferred {
            self.handle_new_event(kind, pointer, spine);
        }
    }

    /// Release pointer capture for a given pointer ID.
    fn release_capture(&mut self, pointer_id: i64) {
        if let Some(target_id) = self.captures.remove(&pointer_id) {
            let seq = self.seq.next(&EventKind::LostPointerCapture);
            self.emitter.frame(|em| {
                em.event(
                    "lostpointercapture",
                    seq,
                    &target_id,
                    &[Prop::val("pointer-id", pointer_id.to_string())],
                )
            });
        }
    }
}

/// Build wire-format props for a pointer event.
fn build_event_props(pointer: &PointerData, node: &SpineNode, verbose: bool) -> Vec<Prop> {
    let mut props = Vec::with_capacity(16);

    // Always included
    props.push(Prop::val("pointer-id", pointer.pointer_id.to_string()));
    props.push(Prop::val("pointer-type", pointer.pointer_type.as_str()));
    props.push(Prop::val("x", format!("{:.1}", node.local_x)));
    props.push(Prop::val("y", format!("{:.1}", node.local_y)));
    props.push(Prop::val("client-x", format!("{:.1}", pointer.client_x)));
    props.push(Prop::val("client-y", format!("{:.1}", pointer.client_y)));

    // Button (always for down/up, skip for move if -1 and not verbose)
    if pointer.button >= 0 || verbose {
        props.push(Prop::val("button", pointer.button.to_string()));
    }

    // Buttons bitmask (skip if 0 and not verbose)
    if pointer.buttons > 0 || verbose {
        props.push(Prop::val("buttons", pointer.buttons.to_string()));
    }

    // Primary flag
    if pointer.primary {
        props.push(Prop::flag("primary"));
    }

    // Pressure (skip if default and not verbose)
    if verbose || (pointer.pressure - 0.0).abs() > f32::EPSILON {
        props.push(Prop::val("pressure", format!("{:.2}", pointer.pressure)));
    }

    // Verbose: include all spec properties even at defaults
    if verbose {
        props.push(Prop::val("width", format!("{:.1}", pointer.width)));
        props.push(Prop::val("height", format!("{:.1}", pointer.height)));
        props.push(Prop::val(
            "tangential-pressure",
            format!("{:.2}", pointer.tangential_pressure),
        ));
        props.push(Prop::val("tilt-x", pointer.tilt_x.to_string()));
        props.push(Prop::val("tilt-y", pointer.tilt_y.to_string()));
        props.push(Prop::val("twist", pointer.twist.to_string()));
        props.push(Prop::val("altitude", format!("{:.4}", pointer.altitude)));
        props.push(Prop::val("azimuth", format!("{:.4}", pointer.azimuth)));
    } else {
        // Non-verbose: only emit non-default values
        if (pointer.width - 1.0).abs() > f64::EPSILON {
            props.push(Prop::val("width", format!("{:.1}", pointer.width)));
        }
        if (pointer.height - 1.0).abs() > f64::EPSILON {
            props.push(Prop::val("height", format!("{:.1}", pointer.height)));
        }
        if pointer.tangential_pressure.abs() > f32::EPSILON {
            props.push(Prop::val(
                "tangential-pressure",
                format!("{:.2}", pointer.tangential_pressure),
            ));
        }
        if pointer.tilt_x != 0 {
            props.push(Prop::val("tilt-x", pointer.tilt_x.to_string()));
        }
        if pointer.tilt_y != 0 {
            props.push(Prop::val("tilt-y", pointer.tilt_y.to_string()));
        }
        if pointer.twist != 0 {
            props.push(Prop::val("twist", pointer.twist.to_string()));
        }
        if (pointer.altitude - std::f64::consts::FRAC_PI_2).abs() > 0.0001 {
            props.push(Prop::val("altitude", format!("{:.4}", pointer.altitude)));
        }
        if pointer.azimuth.abs() > 0.0001 {
            props.push(Prop::val("azimuth", format!("{:.4}", pointer.azimuth)));
        }
    }

    // Modifier flags
    if pointer.modifiers.shift {
        props.push(Prop::flag("mod-shift"));
    }
    if pointer.modifiers.ctrl {
        props.push(Prop::flag("mod-ctrl"));
    }
    if pointer.modifiers.alt {
        props.push(Prop::flag("mod-alt"));
    }
    if pointer.modifiers.meta {
        props.push(Prop::flag("mod-meta"));
    }

    props
}

// ---------------------------------------------------------------------------
// Public API: spawn the engine thread
// ---------------------------------------------------------------------------

/// Spawn the propagation engine thread and return a handle + ACK sender.
///
/// - `emitter`: shared stdout emitter for writing events
///
/// Returns `(EngineHandle, AckSender)` where `AckSender` is a channel
/// for the stdin reader to forward ACKs to the engine.
pub fn spawn_engine(emitter: StdoutEmitter) -> EngineHandle {
    let (tx, rx) = mpsc::channel::<EngineInput>();

    std::thread::Builder::new()
        .name("byo-propagation".into())
        .spawn(move || {
            info!("propagation engine started");
            let mut engine = Engine::new(emitter);

            loop {
                // Use recv_timeout to periodically check for ACK timeouts
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(EngineInput::NewEvent {
                        kind,
                        pointer,
                        spine,
                    }) => {
                        // Release capture on pointerup/pointercancel
                        if matches!(kind, EventKind::PointerUp | EventKind::PointerCancel) {
                            engine.release_capture(pointer.pointer_id);
                        }
                        engine.handle_new_event(kind, pointer, spine);
                    }
                    Ok(EngineInput::Ack {
                        kind,
                        seq,
                        handled,
                        capture,
                    }) => {
                        engine.handle_ack(kind, seq, handled, capture);
                    }
                    Ok(EngineInput::Shutdown) => {
                        info!("propagation engine shutting down");
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        debug!("propagation engine channel disconnected");
                        break;
                    }
                }

                engine.check_timeouts();
            }
        })
        .expect("failed to spawn propagation engine thread");

    EngineHandle { tx }
}
