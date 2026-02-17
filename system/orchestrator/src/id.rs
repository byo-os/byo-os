//! Qualified ID handling for the orchestrator.
//!
//! IDs are local per client (per process). The orchestrator qualifies them
//! internally as `client:id` for cross-client use. This module provides
//! the [`QualifiedId`] type and helpers for qualification/dequalification.

use std::fmt;

/// A qualified object ID: `client:local_id`.
///
/// Stored as a single `String` with the colon separator. The client name
/// and local ID are accessible via [`client()`](Self::client) and
/// [`local_id()`](Self::local_id).
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct QualifiedId(String);

#[allow(dead_code)]
impl QualifiedId {
    /// Create a qualified ID from client name and local ID.
    pub fn new(client: &str, local_id: &str) -> Self {
        let mut s = String::with_capacity(client.len() + 1 + local_id.len());
        s.push_str(client);
        s.push(':');
        s.push_str(local_id);
        Self(s)
    }

    /// Parse a qualified ID string (`client:local_id`).
    ///
    /// Returns `None` if there is no colon separator.
    pub fn parse(s: &str) -> Option<Self> {
        if s.contains(':') {
            Some(Self(s.to_owned()))
        } else {
            None
        }
    }

    /// Returns the client name portion (before the colon).
    pub fn client(&self) -> &str {
        &self.0[..self.colon_pos()]
    }

    /// Returns the local ID portion (after the colon).
    pub fn local_id(&self) -> &str {
        &self.0[self.colon_pos() + 1..]
    }

    /// Returns the full qualified string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if this ID belongs to the given client.
    pub fn is_owned_by(&self, client: &str) -> bool {
        self.client() == client
    }

    fn colon_pos(&self) -> usize {
        self.0
            .find(':')
            .expect("QualifiedId invariant: contains ':'")
    }
}

impl fmt::Debug for QualifiedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QualifiedId({:?})", self.0)
    }
}

impl fmt::Display for QualifiedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Qualify a local ID with a client name.
///
/// If the ID is already qualified (contains `:`), returns it unchanged.
/// Otherwise, prepends `client:`.
pub fn qualify(client: &str, id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        let mut s = String::with_capacity(client.len() + 1 + id.len());
        s.push_str(client);
        s.push(':');
        s.push_str(id);
        s
    }
}

/// Dequalify a qualified ID for sending to a client.
///
/// If the ID starts with `client:`, strips the prefix. Otherwise returns
/// the ID unchanged (it belongs to another client, keep qualified).
#[allow(dead_code)]
pub fn dequalify<'a>(client: &str, id: &'a str) -> &'a str {
    if let Some(rest) = id.strip_prefix(client) {
        if let Some(stripped) = rest.strip_prefix(':') {
            stripped
        } else {
            id
        }
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_id_new() {
        let qid = QualifiedId::new("notes-app", "sidebar");
        assert_eq!(qid.client(), "notes-app");
        assert_eq!(qid.local_id(), "sidebar");
        assert_eq!(qid.as_str(), "notes-app:sidebar");
    }

    #[test]
    fn qualified_id_parse() {
        let qid = QualifiedId::parse("controls:save-bg").unwrap();
        assert_eq!(qid.client(), "controls");
        assert_eq!(qid.local_id(), "save-bg");
    }

    #[test]
    fn qualified_id_parse_no_colon() {
        assert!(QualifiedId::parse("sidebar").is_none());
    }

    #[test]
    fn qualified_id_is_owned_by() {
        let qid = QualifiedId::new("notes-app", "sidebar");
        assert!(qid.is_owned_by("notes-app"));
        assert!(!qid.is_owned_by("controls"));
    }

    #[test]
    fn qualified_id_display() {
        let qid = QualifiedId::new("app", "btn");
        assert_eq!(format!("{qid}"), "app:btn");
    }

    #[test]
    fn qualify_local_id() {
        assert_eq!(qualify("notes-app", "sidebar"), "notes-app:sidebar");
    }

    #[test]
    fn qualify_already_qualified() {
        assert_eq!(qualify("notes-app", "controls:save"), "controls:save");
    }

    #[test]
    fn dequalify_own_id() {
        assert_eq!(dequalify("notes-app", "notes-app:sidebar"), "sidebar");
    }

    #[test]
    fn dequalify_other_id() {
        assert_eq!(
            dequalify("notes-app", "controls:save-bg"),
            "controls:save-bg"
        );
    }

    #[test]
    fn dequalify_already_local() {
        assert_eq!(dequalify("notes-app", "sidebar"), "sidebar");
    }

    #[test]
    fn qualified_id_eq_and_hash() {
        use std::collections::HashSet;
        let a = QualifiedId::new("app", "x");
        let b = QualifiedId::new("app", "x");
        let c = QualifiedId::new("app", "y");
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }
}
