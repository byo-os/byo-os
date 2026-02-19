//! Event subscription configuration — parses the `events` prop CSS-like syntax.
//!
//! Syntax: comma-separated entries, space-separated options per entry.
//!
//! ```text
//! events="pointerdown verbose, pointermove capture verbose, scroll capture passive"
//! ```
//!
//! Options:
//! - Phase: `capture`, `bubble` (default if neither specified)
//! - `passive` — handler won't ACK with handled=true
//! - `verbose` — emit all props, even at default values

use bevy::prelude::*;
use byo::protocol::EventKind;
use std::collections::HashMap;

/// Phase(s) an element subscribes to for an event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Capture phase only (root → leaf)
    Capture,
    /// Bubble phase only (leaf → root, default)
    Bubble,
    /// Both capture and bubble phases
    Both,
}

impl Phase {
    pub fn includes_capture(self) -> bool {
        matches!(self, Phase::Capture | Phase::Both)
    }

    pub fn includes_bubble(self) -> bool {
        matches!(self, Phase::Bubble | Phase::Both)
    }
}

/// Configuration for a single event subscription entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSub {
    pub kind: EventKind,
    pub phase: Phase,
    pub passive: bool,
    pub verbose: bool,
}

/// Parsed event subscription config, stored as a Bevy Component.
///
/// Maps event kinds to their subscription configuration.
/// An empty map means no subscriptions (but the element still blocks
/// unless `pointer-events=none`).
#[derive(Component, Debug, Clone, Default)]
pub struct EventSubscriptions {
    subs: HashMap<EventKind, EventSub>,
}

impl EventSubscriptions {
    /// Parse the `events` prop value.
    ///
    /// Format: `"pointerdown verbose, pointermove capture verbose, scroll capture passive"`
    pub fn parse(input: &str) -> Self {
        let mut subs = HashMap::new();
        for entry in input.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if let Some(sub) = parse_entry(entry) {
                subs.insert(sub.kind.clone(), sub);
            }
        }
        Self { subs }
    }

    /// Returns the subscription for a given event kind, if any.
    pub fn get(&self, kind: &EventKind) -> Option<&EventSub> {
        self.subs.get(kind)
    }
}

/// Parse a single comma-separated entry like `"pointermove capture verbose"`.
fn parse_entry(entry: &str) -> Option<EventSub> {
    let mut tokens = entry.split_whitespace();
    let event_name = tokens.next()?;

    let kind = EventKind::from_wire(event_name);

    let mut has_capture = false;
    let mut has_bubble = false;
    let mut passive = false;
    let mut verbose = false;

    for token in tokens {
        match token {
            "capture" => has_capture = true,
            "bubble" => has_bubble = true,
            "passive" => passive = true,
            "verbose" => verbose = true,
            _ => {} // ignore unknown options
        }
    }

    let phase = match (has_capture, has_bubble) {
        (true, true) => Phase::Both,
        (true, false) => Phase::Capture,
        // default: bubble
        _ => Phase::Bubble,
    };

    Some(EventSub {
        kind,
        phase,
        passive,
        verbose,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let subs = EventSubscriptions::parse("pointerdown");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(!sub.verbose);
    }

    #[test]
    fn parse_with_options() {
        let subs = EventSubscriptions::parse("pointerdown verbose");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(sub.verbose);
    }

    #[test]
    fn parse_capture_phase() {
        let subs = EventSubscriptions::parse("pointermove capture");
        let sub = subs.get(&EventKind::PointerMove).unwrap();
        assert_eq!(sub.phase, Phase::Capture);
    }

    #[test]
    fn parse_both_phases() {
        let subs = EventSubscriptions::parse("pointerdown capture bubble");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Both);
    }

    #[test]
    fn parse_multiple_entries() {
        let subs = EventSubscriptions::parse(
            "pointerdown verbose, pointermove capture verbose, scroll capture passive",
        );
        assert_eq!(subs.subs.len(), 3);

        let down = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(down.phase, Phase::Bubble);
        assert!(down.verbose);
        assert!(!down.passive);

        let r#move = subs.get(&EventKind::PointerMove).unwrap();
        assert_eq!(r#move.phase, Phase::Capture);
        assert!(r#move.verbose);
        assert!(!r#move.passive);

        let scroll = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(scroll.phase, Phase::Capture);
        assert!(!scroll.verbose);
        assert!(scroll.passive);
    }

    #[test]
    fn parse_empty() {
        let subs = EventSubscriptions::parse("");
        assert!(subs.is_empty());
    }

    #[test]
    fn parse_whitespace_tolerance() {
        let subs = EventSubscriptions::parse("  pointerdown  verbose  ,  click  ");
        assert_eq!(subs.subs.len(), 2);
        assert!(subs.get(&EventKind::PointerDown).is_some());
        assert!(subs.get(&EventKind::Click).is_some());
    }

    #[test]
    fn subscribes_in_phase() {
        let subs = EventSubscriptions::parse("pointerdown capture bubble, pointermove capture");

        assert!(subs.subscribes_in_phase(&EventKind::PointerDown, true));
        assert!(subs.subscribes_in_phase(&EventKind::PointerDown, false));

        assert!(subs.subscribes_in_phase(&EventKind::PointerMove, true));
        assert!(!subs.subscribes_in_phase(&EventKind::PointerMove, false));

        assert!(!subs.subscribes_in_phase(&EventKind::Click, true));
        assert!(!subs.subscribes_in_phase(&EventKind::Click, false));
    }

    #[test]
    fn parse_third_party_event() {
        let subs = EventSubscriptions::parse("com.example.custom verbose");
        let kind = EventKind::from_wire("com.example.custom");
        let sub = subs.get(&kind).unwrap();
        assert!(sub.verbose);
    }

    #[test]
    fn default_is_empty() {
        let subs = EventSubscriptions::default();
        assert!(subs.is_empty());
    }

    #[test]
    fn parse_all_pointer_events() {
        let subs = EventSubscriptions::parse(
            "pointerdown, pointerup, pointermove, pointerover, pointerout, \
             pointerenter, pointerleave, pointercancel, \
             gotpointercapture, lostpointercapture, \
             click, auxclick, dblclick, scroll",
        );
        assert_eq!(subs.subs.len(), 14);
    }

    #[test]
    fn parse_passive_and_verbose_together() {
        let subs = EventSubscriptions::parse("scroll capture passive verbose");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.phase, Phase::Capture);
        assert!(sub.passive);
        assert!(sub.verbose);
    }

    #[test]
    fn unknown_options_ignored() {
        let subs = EventSubscriptions::parse("pointerdown fancy turbo");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(!sub.verbose);
    }
}
