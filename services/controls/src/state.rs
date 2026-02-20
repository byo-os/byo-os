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
}

/// State for a single control instance.
#[derive(Debug)]
pub struct ControlState {
    pub kind: ControlKind,
    /// The source qualified ID (e.g. "app:save").
    pub source_qid: String,
    /// The local part of the source ID (after ':'), used as prefix for expansion IDs.
    pub local_id: String,
    /// Semantic props from the app (label, variant, etc.).
    pub props: HashMap<String, String>,
    /// Parsed event subscriptions from the app's `events` prop.
    pub app_events: EventSubscriptionSet,
    /// Whether the control is currently hovered.
    pub hover: bool,
    /// Whether the control is currently pressed (pointer down).
    pub pressed: bool,
    /// Internal checked state for uncontrolled checkboxes.
    pub checked: Option<bool>,
    /// Internal value for uncontrolled sliders.
    pub value: Option<f64>,
    /// Whether the control is disabled.
    pub disabled: bool,
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

        let checked = if props.contains_key("checked") {
            // Controlled mode — use the prop value
            Some(props.get("checked").is_none_or(|v| v != "false"))
        } else if props.contains_key("default-checked") {
            // Uncontrolled mode — use default
            Some(props.get("default-checked").is_none_or(|v| v != "false"))
        } else if kind == ControlKind::Checkbox {
            // Uncontrolled with no default
            Some(false)
        } else {
            None
        };

        let value = if kind == ControlKind::Slider {
            if let Some(v) = props.get("value") {
                v.parse().ok()
            } else if let Some(v) = props.get("default-value") {
                v.parse().ok()
            } else {
                Some(Self::default_slider_value(props))
            }
        } else {
            None
        };

        Self {
            kind,
            source_qid: source_qid.to_owned(),
            local_id,
            props: props.clone(),
            app_events,
            hover: false,
            pressed: false,
            checked,
            value,
            disabled,
        }
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
        self.kind == ControlKind::Checkbox && self.props.contains_key("checked")
    }

    /// Whether this is a controlled slider (app provides `value` prop).
    pub fn is_controlled_slider(&self) -> bool {
        self.kind == ControlKind::Slider && self.props.contains_key("value")
    }

    /// Whether the app subscribed to a specific semantic event.
    pub fn app_wants_event(&self, kind: &EventKind) -> bool {
        // If no events prop specified, use defaults per control type
        if self.app_events.is_empty() {
            return match self.kind {
                ControlKind::Button => *kind == EventKind::Press,
                ControlKind::Checkbox => *kind == EventKind::Change,
                ControlKind::Slider => *kind == EventKind::Change || *kind == EventKind::Input,
            };
        }
        self.app_events.contains(kind)
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
        let suffixes = match state.kind {
            ControlKind::Button => vec!["root", "label"],
            ControlKind::Checkbox => vec!["root", "box", "check", "label"],
            ControlKind::Slider => vec!["root", "track", "fill", "header", "label", "value"],
        };

        for suffix in suffixes {
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
            let suffixes = match state.kind {
                ControlKind::Button => vec!["root", "label"],
                ControlKind::Checkbox => vec!["root", "box", "check", "label"],
                ControlKind::Slider => {
                    vec!["root", "track", "fill", "header", "label", "value"]
                }
            };
            for suffix in suffixes {
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
        assert_eq!(state.checked, Some(true));
    }

    #[test]
    fn uncontrolled_checkbox_default() {
        let props = make_props(&[("label", "Check"), ("default-checked", "true")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        assert!(!state.is_controlled_checkbox());
        assert_eq!(state.checked, Some(true));
    }

    #[test]
    fn uncontrolled_checkbox_no_default() {
        let props = make_props(&[("label", "Check")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:check", &props);
        assert!(!state.is_controlled_checkbox());
        assert_eq!(state.checked, Some(false));
    }

    #[test]
    fn slider_default_value() {
        let props = make_props(&[("label", "Volume")]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        assert_eq!(state.value, Some(50.0));
    }

    #[test]
    fn slider_custom_range() {
        let props = make_props(&[("label", "Vol"), ("min", "10"), ("max", "20")]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        assert_eq!(state.value, Some(15.0));
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
