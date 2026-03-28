//! Control state tracking — maps source qualified IDs to control state,
//! reverse maps expansion IDs to source IDs, and manages event sequence counters.

use std::collections::HashMap;

use byo::events::EventSubscriptionSet;
use byo::protocol::EventKind;

/// The kind of control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Button,
    Checkbox,
    Slider,
    Scrollbar,
    ScrollView,
}

impl ControlKind {
    /// Expansion sub-element suffixes for this control kind.
    /// Used for registering/removing reverse ID mappings.
    pub fn expansion_suffixes(self) -> &'static [&'static str] {
        match self {
            Self::Button => &["root", "label"],
            Self::Checkbox => &["root", "box", "check", "label"],
            Self::Slider => &["root", "track", "fill", "header", "label", "value"],
            Self::Scrollbar => &["root", "track", "thumb"],
            Self::ScrollView => &["root", "viewport", "scrollbar-y", "scrollbar-x"],
        }
    }
}

/// Kind-specific state for a control instance.
#[derive(Debug)]
pub enum KindState {
    Button {
        hover: bool,
        pressed: bool,
    },
    Checkbox {
        hover: bool,
        pressed: bool,
        checked: Option<bool>,
    },
    Slider {
        hover: bool,
        pressed: bool,
        value: Option<f64>,
    },
    Scrollbar {
        thumb_hover: bool,
        thumb_pressed: bool,
        track_hover: bool,
        /// Drag start position (viewport-relative client-x/y coordinates).
        drag_start_pos: f64,
        drag_start_scroll: f64,
        /// Track pixel size at drag start (derived from thumb pixel size / thumb_pct).
        drag_track_size: f64,
        content_size: f64,
        viewport_size: f64,
        /// Overscroll overflow amount (negative = past start, positive = past end).
        scroll_overflow: f64,
        /// Whether the scrollbar is currently faded in (modern style auto-hide).
        fade_visible: bool,
    },
    ScrollView {
        scroll_x: f64,
        scroll_y: f64,
    },
}

impl KindState {
    /// Return the `ControlKind` discriminant for this state.
    pub fn kind(&self) -> ControlKind {
        match self {
            Self::Button { .. } => ControlKind::Button,
            Self::Checkbox { .. } => ControlKind::Checkbox,
            Self::Slider { .. } => ControlKind::Slider,
            Self::Scrollbar { .. } => ControlKind::Scrollbar,
            Self::ScrollView { .. } => ControlKind::ScrollView,
        }
    }
}

/// State for a single control instance.
#[derive(Debug)]
pub struct ControlState {
    /// The source qualified ID (e.g. "app:save").
    pub source_qid: String,
    /// The local part of the source ID (after ':'), used as prefix for expansion IDs.
    pub local_id: String,
    /// Semantic props from the app (label, variant, etc.).
    pub props: HashMap<String, String>,
    /// Parsed event subscriptions from the app's `events` prop.
    pub app_events: EventSubscriptionSet,
    /// Whether the control is disabled.
    pub disabled: bool,
    /// Kind-specific interaction and layout state.
    pub kind_state: KindState,
}

impl ControlState {
    /// Create a new ControlState from an expand request.
    pub fn new(kind: ControlKind, source_qid: &str, props: &HashMap<String, String>) -> Self {
        let local_id = source_qid
            .split_once(':')
            .map(|(_, id)| id)
            .unwrap_or(source_qid)
            .to_owned();

        let app_events = props
            .get("events")
            .map(|e| EventSubscriptionSet::parse(e))
            .unwrap_or_default();

        let disabled = props.contains_key("disabled");

        let kind_state = match kind {
            ControlKind::Button => KindState::Button {
                hover: false,
                pressed: false,
            },
            ControlKind::Checkbox => {
                let checked = if props.contains_key("checked") {
                    Some(props.get("checked").is_none_or(|v| v != "false"))
                } else if props.contains_key("default-checked") {
                    Some(props.get("default-checked").is_none_or(|v| v != "false"))
                } else {
                    Some(false)
                };
                KindState::Checkbox {
                    hover: false,
                    pressed: false,
                    checked,
                }
            }
            ControlKind::Slider => {
                let value = if let Some(v) = props.get("value") {
                    v.parse().ok()
                } else if let Some(v) = props.get("default-value") {
                    v.parse().ok()
                } else {
                    Some(Self::default_slider_value(props))
                };
                KindState::Slider {
                    hover: false,
                    pressed: false,
                    value,
                }
            }
            ControlKind::Scrollbar => {
                let content_size = props
                    .get("content-size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let viewport_size = props
                    .get("viewport-size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                KindState::Scrollbar {
                    thumb_hover: false,
                    thumb_pressed: false,
                    track_hover: false,
                    drag_start_pos: 0.0,
                    drag_start_scroll: 0.0,
                    drag_track_size: 0.0,
                    content_size,
                    viewport_size,
                    scroll_overflow: 0.0,
                    fade_visible: false,
                }
            }
            ControlKind::ScrollView => {
                let scroll_x = props
                    .get("scroll-x")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let scroll_y = props
                    .get("scroll-y")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                KindState::ScrollView { scroll_x, scroll_y }
            }
        };

        Self {
            source_qid: source_qid.to_owned(),
            local_id,
            props: props.clone(),
            app_events,
            disabled,
            kind_state,
        }
    }

    /// The kind of this control.
    pub fn kind(&self) -> ControlKind {
        self.kind_state.kind()
    }

    /// Whether the control is hovered (button, checkbox, slider).
    pub fn hover(&self) -> bool {
        match &self.kind_state {
            KindState::Button { hover, .. }
            | KindState::Checkbox { hover, .. }
            | KindState::Slider { hover, .. } => *hover,
            _ => false,
        }
    }

    /// Whether the control is pressed (button, checkbox, slider).
    pub fn pressed(&self) -> bool {
        match &self.kind_state {
            KindState::Button { pressed, .. }
            | KindState::Checkbox { pressed, .. }
            | KindState::Slider { pressed, .. } => *pressed,
            _ => false,
        }
    }

    /// Internal checked state (checkbox only, None for other kinds).
    pub fn checked(&self) -> Option<bool> {
        match &self.kind_state {
            KindState::Checkbox { checked, .. } => *checked,
            _ => None,
        }
    }

    /// Internal value (slider only, None for other kinds).
    pub fn value(&self) -> Option<f64> {
        match &self.kind_state {
            KindState::Slider { value, .. } => *value,
            _ => None,
        }
    }

    /// Whether the scrollbar has any active interaction (hover or drag).
    pub fn is_scrollbar_active(&self) -> bool {
        matches!(
            self.kind_state,
            KindState::Scrollbar {
                thumb_hover: true, ..
            } | KindState::Scrollbar {
                track_hover: true, ..
            } | KindState::Scrollbar {
                thumb_pressed: true,
                ..
            }
        )
    }

    /// Resolve the scrollbar style for this control.
    pub fn scrollbar_style(&self) -> &'static str {
        crate::expand::resolve_scrollbar_style(&self.props)
    }

    /// Update props and prop-derived fields for a re-expansion.
    ///
    /// Preserves interaction state (hover, pressed, thumb states, drag state)
    /// and any internal state that isn't directly derived from props (scroll
    /// position, content/viewport sizes — which may have been updated by
    /// resize events since the last expansion).
    pub fn update_props(&mut self, new_props: &HashMap<String, String>) {
        self.app_events = new_props
            .get("events")
            .map(|e| EventSubscriptionSet::parse(e))
            .unwrap_or_default();
        self.disabled = new_props.contains_key("disabled");

        match &mut self.kind_state {
            KindState::Checkbox { checked, .. } => {
                // Controlled checkbox: update checked from prop
                if new_props.contains_key("checked") {
                    *checked = Some(new_props.get("checked").is_none_or(|v| v != "false"));
                }
                // (Uncontrolled: keep existing internal checked state)
            }
            KindState::Slider { value, .. } => {
                // Controlled slider: update value from prop
                if new_props.contains_key("value") {
                    *value = new_props.get("value").and_then(|v| v.parse().ok());
                }
                // (Uncontrolled: keep existing internal value)
            }
            KindState::Scrollbar {
                content_size,
                viewport_size,
                ..
            } => {
                // Update content/viewport sizes from props (arrive via reduced state)
                if let Some(v) = new_props.get("content-size").and_then(|v| v.parse().ok()) {
                    *content_size = v;
                }
                if let Some(v) = new_props.get("viewport-size").and_then(|v| v.parse().ok()) {
                    *viewport_size = v;
                }
            }
            KindState::ScrollView { scroll_x, scroll_y } => {
                if let Some(v) = new_props.get("scroll-x").and_then(|v| v.parse().ok()) {
                    *scroll_x = v;
                }
                if let Some(v) = new_props.get("scroll-y").and_then(|v| v.parse().ok()) {
                    *scroll_y = v;
                }
            }
            _ => {}
        }

        self.props = new_props.clone();
    }

    fn default_slider_value(props: &HashMap<String, String>) -> f64 {
        let min = props
            .get("min")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let max = props
            .get("max")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(100.0);
        (min + max) / 2.0
    }

    /// Whether this is a controlled checkbox (app provides `checked` prop).
    pub fn is_controlled_checkbox(&self) -> bool {
        self.kind() == ControlKind::Checkbox && self.props.contains_key("checked")
    }

    /// Whether this is a controlled slider (app provides `value` prop).
    pub fn is_controlled_slider(&self) -> bool {
        self.kind() == ControlKind::Slider && self.props.contains_key("value")
    }

    /// Whether the app subscribed to a specific semantic event.
    pub fn app_wants_event(&self, kind: &EventKind) -> bool {
        // If no events prop specified, use defaults per control type
        if self.app_events.is_empty() {
            return match self.kind_state {
                KindState::Button { .. } => *kind == EventKind::Press,
                KindState::Checkbox { .. } => *kind == EventKind::Change,
                KindState::Slider { .. } => *kind == EventKind::Change || *kind == EventKind::Input,
                KindState::ScrollView { .. } => {
                    *kind == EventKind::Scroll || *kind == EventKind::Resize
                }
                KindState::Scrollbar { .. } => false,
            };
        }
        self.app_events.contains(kind)
    }

    /// Whether this control's direction is vertical (default) vs horizontal.
    pub fn is_vertical(&self) -> bool {
        self.props
            .get("direction")
            .map(|s| s.as_str())
            .unwrap_or("vertical")
            != "horizontal"
    }

    /// Whether each scroll axis is enabled, based on the `direction` prop.
    /// Returns (y_enabled, x_enabled).
    pub fn scroll_axes(&self) -> (bool, bool) {
        let direction = self
            .props
            .get("direction")
            .map(|s| s.as_str())
            .unwrap_or("vertical");
        let y = direction == "vertical" || direction == "both";
        let x = direction == "horizontal" || direction == "both";
        (y, x)
    }

    /// Get the slider min value.
    pub fn slider_min(&self) -> f64 {
        self.props
            .get("min")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    }

    /// Get the slider max value.
    pub fn slider_max(&self) -> f64 {
        self.props
            .get("max")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100.0)
    }

    /// Get the slider step value.
    pub fn slider_step(&self) -> f64 {
        self.props
            .get("step")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0)
    }

    /// Compute the slider fill percentage (0–100) from the current value.
    pub fn slider_pct(&self) -> f64 {
        let value = self.value().unwrap_or(50.0);
        let min = self.slider_min();
        let max = self.slider_max();
        if (max - min).abs() > f64::EPSILON {
            ((value - min) / (max - min) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }

    /// Snap a value to the step grid and clamp to range.
    pub fn snap_slider_value(&self, raw: f64) -> f64 {
        let min = self.slider_min();
        let max = self.slider_max();
        let step = self.slider_step();
        let clamped = raw.clamp(min, max);
        if step > 0.0 {
            let steps = ((clamped - min) / step).round();
            (min + steps * step).clamp(min, max)
        } else {
            clamped
        }
    }
}

/// Manages all controls, reverse maps, and sequence counters.
pub struct Daemon {
    /// Maps source qualified ID → ControlState.
    pub controls: HashMap<String, ControlState>,
    /// Maps expansion local ID (e.g. "save-root") → source qualified ID (e.g. "app:save").
    pub reverse: HashMap<String, String>,
    /// Per-event-type outbound sequence counters for events emitted to apps.
    pub event_seqs: HashMap<String, u64>,
}

impl Daemon {
    pub fn new() -> Self {
        Self {
            controls: HashMap::new(),
            reverse: HashMap::new(),
            event_seqs: HashMap::new(),
        }
    }

    /// Get the next sequence number for a given event type.
    pub fn next_seq(&mut self, event_type: &str) -> u64 {
        let seq = self.event_seqs.entry(event_type.to_owned()).or_insert(0);
        let current = *seq;
        *seq += 1;
        current
    }

    /// Register a control and its expansion ID reverse mappings.
    pub fn register(&mut self, state: ControlState) {
        let source_qid = state.source_qid.clone();
        let local_id = state.local_id.clone();

        // Register reverse mappings for all expansion IDs
        for suffix in state.kind().expansion_suffixes() {
            let expansion_id = format!("{local_id}-{suffix}");
            self.reverse.insert(expansion_id, source_qid.clone());
        }

        self.controls.insert(source_qid, state);
    }

    /// Look up the source qualified ID for an expansion local ID.
    pub fn resolve(&self, expansion_id: &str) -> Option<&str> {
        self.reverse.get(expansion_id).map(|s| s.as_str())
    }

    /// Get a control state by source qualified ID.
    pub fn get(&self, source_qid: &str) -> Option<&ControlState> {
        self.controls.get(source_qid)
    }

    /// Get a mutable control state by source qualified ID.
    pub fn get_mut(&mut self, source_qid: &str) -> Option<&mut ControlState> {
        self.controls.get_mut(source_qid)
    }

    /// Remove a control and its reverse mappings.
    pub fn remove(&mut self, source_qid: &str) {
        if let Some(state) = self.controls.remove(source_qid) {
            for suffix in state.kind().expansion_suffixes() {
                let expansion_id = format!("{}-{suffix}", state.local_id);
                self.reverse.remove(&expansion_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn control_state_local_id_extraction() {
        let props = make_props(&[("label", "Save")]);
        let state = ControlState::new(ControlKind::Button, "app:save", &props);
        assert_eq!(state.local_id, "save");
    }

    #[test]
    fn control_state_unqualified_id() {
        let props = make_props(&[("label", "Save")]);
        let state = ControlState::new(ControlKind::Button, "save", &props);
        assert_eq!(state.local_id, "save");
    }

    #[test]
    fn button_default_events() {
        let props = make_props(&[("label", "OK")]);
        let state = ControlState::new(ControlKind::Button, "app:ok", &props);
        assert!(state.app_wants_event(&EventKind::Press));
        assert!(!state.app_wants_event(&EventKind::Change));
    }

    #[test]
    fn checkbox_default_events() {
        let props = make_props(&[("label", "Agree")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:agree", &props);
        assert!(state.app_wants_event(&EventKind::Change));
        assert!(!state.app_wants_event(&EventKind::Press));
    }

    #[test]
    fn explicit_events_override_defaults() {
        let props = make_props(&[("label", "OK"), ("events", "press, hover")]);
        let state = ControlState::new(ControlKind::Button, "app:ok", &props);
        assert!(state.app_wants_event(&EventKind::Press));
        assert!(state.app_events.contains(&EventKind::from_wire("hover")));
        assert!(!state.app_wants_event(&EventKind::Change));
    }

    #[test]
    fn controlled_checkbox() {
        let props = make_props(&[("label", "Check"), ("checked", "true")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        assert!(state.is_controlled_checkbox());
        assert_eq!(state.checked(), Some(true));
    }

    #[test]
    fn uncontrolled_checkbox_default() {
        let props = make_props(&[("label", "Check"), ("default-checked", "true")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        assert!(!state.is_controlled_checkbox());
        assert_eq!(state.checked(), Some(true));
    }

    #[test]
    fn uncontrolled_checkbox_no_default() {
        let props = make_props(&[("label", "Check")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        assert!(!state.is_controlled_checkbox());
        assert_eq!(state.checked(), Some(false));
    }

    #[test]
    fn slider_default_value() {
        let props = make_props(&[("label", "Volume")]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        assert_eq!(state.value(), Some(50.0));
    }

    #[test]
    fn slider_custom_range() {
        let props = make_props(&[("label", "Vol"), ("min", "10"), ("max", "20")]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        assert_eq!(state.value(), Some(15.0));
    }

    #[test]
    fn snap_slider_value() {
        let props = make_props(&[
            ("label", "Vol"),
            ("min", "0"),
            ("max", "100"),
            ("step", "10"),
        ]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        assert_eq!(state.snap_slider_value(43.0), 40.0);
        assert_eq!(state.snap_slider_value(47.0), 50.0);
        assert_eq!(state.snap_slider_value(-5.0), 0.0);
        assert_eq!(state.snap_slider_value(105.0), 100.0);
    }

    #[test]
    fn daemon_register_and_resolve() {
        let mut daemon = Daemon::new();
        let props = make_props(&[("label", "Save")]);
        let state = ControlState::new(ControlKind::Button, "app:save", &props);
        daemon.register(state);

        assert_eq!(daemon.resolve("save-root"), Some("app:save"));
        assert_eq!(daemon.resolve("save-label"), Some("app:save"));
        assert!(daemon.resolve("save-box").is_none()); // not a button suffix
    }

    #[test]
    fn daemon_remove_cleans_reverse() {
        let mut daemon = Daemon::new();
        let props = make_props(&[("label", "Check")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        daemon.register(state);

        assert!(daemon.resolve("check-root").is_some());
        daemon.remove("app:check");
        assert!(daemon.resolve("check-root").is_none());
        assert!(daemon.get("app:check").is_none());
    }

    #[test]
    fn daemon_seq_counter() {
        let mut daemon = Daemon::new();
        assert_eq!(daemon.next_seq("press"), 0);
        assert_eq!(daemon.next_seq("press"), 1);
        assert_eq!(daemon.next_seq("change"), 0);
        assert_eq!(daemon.next_seq("press"), 2);
    }

    #[test]
    fn disabled_state() {
        let props = make_props(&[("label", "OK"), ("disabled", "")]);
        let state = ControlState::new(ControlKind::Button, "app:ok", &props);
        assert!(state.disabled);
    }
}
