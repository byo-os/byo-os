//! Event subscription configuration — wraps `byo::events` with Bevy Component.
//!
//! The parsing logic lives in `byo::events::EventSubscriptionSet`.
//! This module re-exports the core types and adds a Bevy `Component` wrapper.

use bevy::prelude::*;
use byo::protocol::EventKind;

// Re-export core types so existing compositor code doesn't need to change imports.
pub use byo::events::{EventSub, EventSubscriptionSet, Phase};

/// Parsed event subscription config, stored as a Bevy Component.
///
/// Maps event kinds to their subscription configuration.
/// An empty map means no subscriptions (but the element still blocks
/// unless `pointer-events=none`).
#[derive(Component, Debug, Clone, Default)]
pub struct EventSubscriptions {
    inner: EventSubscriptionSet,
}

impl EventSubscriptions {
    /// Parse the `events` prop value.
    ///
    /// Format: `"pointerdown verbose, pointermove capture verbose, scroll capture passive"`
    pub fn parse(input: &str) -> Self {
        Self {
            inner: EventSubscriptionSet::parse(input),
        }
    }

    /// Returns the subscription for a given event kind, if any.
    pub fn get(&self, kind: &EventKind) -> Option<&EventSub> {
        self.inner.get(kind)
    }
}

impl std::ops::Deref for EventSubscriptions {
    type Target = EventSubscriptionSet;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
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
        assert_eq!(subs.inner.len(), 3);

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
        assert!(subs.inner.is_empty());
    }

    #[test]
    fn parse_whitespace_tolerance() {
        let subs = EventSubscriptions::parse("  pointerdown  verbose  ,  click  ");
        assert_eq!(subs.inner.len(), 2);
        assert!(subs.get(&EventKind::PointerDown).is_some());
        assert!(subs.get(&EventKind::Click).is_some());
    }

    #[test]
    fn subscribes_in_phase() {
        let subs = EventSubscriptions::parse("pointerdown capture bubble, pointermove capture");

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
        let subs = EventSubscriptions::parse("com.example.custom verbose");
        let kind = EventKind::from_wire("com.example.custom");
        let sub = subs.get(&kind).unwrap();
        assert!(sub.verbose);
    }

    #[test]
    fn default_is_empty() {
        let subs = EventSubscriptions::default();
        assert!(subs.inner.is_empty());
    }

    #[test]
    fn parse_all_pointer_events() {
        let subs = EventSubscriptions::parse(
            "pointerdown, pointerup, pointermove, pointerover, pointerout, \
             pointerenter, pointerleave, pointercancel, \
             gotpointercapture, lostpointercapture, \
             click, auxclick, dblclick, scroll",
        );
        assert_eq!(subs.inner.len(), 14);
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
