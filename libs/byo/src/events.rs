//! Event subscription parsing — parses the `events` prop CSS-like syntax.
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

use crate::protocol::EventKind;
use std::collections::HashMap;

/// Phase(s) an element subscribes to for an event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Capture phase only (root -> leaf)
    Capture,
    /// Bubble phase only (leaf -> root, default)
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
    /// Optional forward target: when this node is reached during dispatch,
    /// splice a nested capture/bubble cycle on the target's subtree.
    pub forward_target: Option<String>,
}

/// Parsed event subscription set.
///
/// Maps event kinds to their subscription configuration.
/// An empty map means no subscriptions.
#[derive(Debug, Clone, Default)]
pub struct EventSubscriptionSet {
    subs: HashMap<EventKind, EventSub>,
}

impl EventSubscriptionSet {
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

    /// Returns `true` if the set contains a subscription for the given kind.
    pub fn contains(&self, kind: &EventKind) -> bool {
        self.subs.contains_key(kind)
    }

    /// Returns `true` if the set has no subscriptions.
    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    /// Returns the number of subscriptions.
    pub fn len(&self) -> usize {
        self.subs.len()
    }

    /// Iterates over all subscriptions.
    pub fn iter(&self) -> impl Iterator<Item = (&EventKind, &EventSub)> {
        self.subs.iter()
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
    let mut forward_target = None;

    for token in tokens {
        match token {
            "capture" => has_capture = true,
            "bubble" => has_bubble = true,
            "passive" => passive = true,
            "verbose" => verbose = true,
            _ => {
                if let Some(id) = token.strip_prefix("forward(").and_then(|s| s.strip_suffix(')')) {
                    forward_target = Some(id.to_string());
                }
            }
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
        forward_target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let subs = EventSubscriptionSet::parse("pointerdown");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(!sub.verbose);
    }

    #[test]
    fn parse_with_options() {
        let subs = EventSubscriptionSet::parse("pointerdown verbose");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(sub.verbose);
    }

    #[test]
    fn parse_capture_phase() {
        let subs = EventSubscriptionSet::parse("pointermove capture");
        let sub = subs.get(&EventKind::PointerMove).unwrap();
        assert_eq!(sub.phase, Phase::Capture);
    }

    #[test]
    fn parse_both_phases() {
        let subs = EventSubscriptionSet::parse("pointerdown capture bubble");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Both);
    }

    #[test]
    fn parse_multiple_entries() {
        let subs = EventSubscriptionSet::parse(
            "pointerdown verbose, pointermove capture verbose, scroll capture passive",
        );
        assert_eq!(subs.len(), 3);

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
        let subs = EventSubscriptionSet::parse("");
        assert!(subs.is_empty());
    }

    #[test]
    fn parse_whitespace_tolerance() {
        let subs = EventSubscriptionSet::parse("  pointerdown  verbose  ,  click  ");
        assert_eq!(subs.len(), 2);
        assert!(subs.contains(&EventKind::PointerDown));
        assert!(subs.contains(&EventKind::Click));
    }

    #[test]
    fn subscribes_in_phase() {
        let subs = EventSubscriptionSet::parse("pointerdown capture bubble, pointermove capture");

        let down = subs.get(&EventKind::PointerDown).unwrap();
        assert!(down.phase.includes_capture());
        assert!(down.phase.includes_bubble());

        let mv = subs.get(&EventKind::PointerMove).unwrap();
        assert!(mv.phase.includes_capture());
        assert!(!mv.phase.includes_bubble());

        assert!(subs.get(&EventKind::Click).is_none());
    }

    #[test]
    fn parse_third_party_event() {
        let subs = EventSubscriptionSet::parse("com.example.custom verbose");
        let kind = EventKind::from_wire("com.example.custom");
        let sub = subs.get(&kind).unwrap();
        assert!(sub.verbose);
    }

    #[test]
    fn default_is_empty() {
        let subs = EventSubscriptionSet::default();
        assert!(subs.is_empty());
    }

    #[test]
    fn parse_all_pointer_events() {
        let subs = EventSubscriptionSet::parse(
            "pointerdown, pointerup, pointermove, pointerover, pointerout, \
             pointerenter, pointerleave, pointercancel, \
             gotpointercapture, lostpointercapture, \
             click, auxclick, dblclick, scroll",
        );
        assert_eq!(subs.len(), 14);
    }

    #[test]
    fn parse_passive_and_verbose_together() {
        let subs = EventSubscriptionSet::parse("scroll capture passive verbose");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.phase, Phase::Capture);
        assert!(sub.passive);
        assert!(sub.verbose);
    }

    #[test]
    fn unknown_options_ignored() {
        let subs = EventSubscriptionSet::parse("pointerdown fancy turbo");
        let sub = subs.get(&EventKind::PointerDown).unwrap();
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
        assert!(!sub.verbose);
    }

    #[test]
    fn contains_and_len() {
        let subs = EventSubscriptionSet::parse("click, pointerdown");
        assert!(subs.contains(&EventKind::Click));
        assert!(subs.contains(&EventKind::PointerDown));
        assert!(!subs.contains(&EventKind::Scroll));
        assert_eq!(subs.len(), 2);
    }

    #[test]
    fn forward_basic() {
        let subs = EventSubscriptionSet::parse("scroll forward(viewport)");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.forward_target.as_deref(), Some("viewport"));
        assert_eq!(sub.phase, Phase::Bubble);
        assert!(!sub.passive);
    }

    #[test]
    fn forward_with_modifiers() {
        let subs = EventSubscriptionSet::parse("scroll capture forward(viewport)");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.forward_target.as_deref(), Some("viewport"));
        assert_eq!(sub.phase, Phase::Capture);
    }

    #[test]
    fn forward_multiple_events() {
        let subs =
            EventSubscriptionSet::parse("scroll forward(vp), click forward(btn)");
        let scroll = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(scroll.forward_target.as_deref(), Some("vp"));
        let click = subs.get(&EventKind::Click).unwrap();
        assert_eq!(click.forward_target.as_deref(), Some("btn"));
    }

    #[test]
    fn forward_preserves_other_modifiers() {
        let subs =
            EventSubscriptionSet::parse("scroll capture passive verbose forward(vp)");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.forward_target.as_deref(), Some("vp"));
        assert_eq!(sub.phase, Phase::Capture);
        assert!(sub.passive);
        assert!(sub.verbose);
    }

    #[test]
    fn no_forward() {
        let subs = EventSubscriptionSet::parse("scroll capture");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert!(sub.forward_target.is_none());
    }

    #[test]
    fn forward_with_hyphenated_id() {
        let subs = EventSubscriptionSet::parse("scroll forward(content-viewport)");
        let sub = subs.get(&EventKind::Scroll).unwrap();
        assert_eq!(sub.forward_target.as_deref(), Some("content-viewport"));
    }
}
