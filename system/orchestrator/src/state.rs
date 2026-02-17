//! Object tree state for crash recovery and late-daemon replay.
//!
//! The orchestrator maintains a reduced state for every object: the latest
//! `+` merged with all subsequent `@` patches, producing the equivalent of
//! a single `+` with the final props. This enables daemon crash recovery
//! and late daemon startup replay.
//!
//! The generic tree implementation lives in [`byo::tree`]. This module
//! specializes it for the orchestrator (keyed on [`QualifiedId`], data is
//! [`ProcessId`]) and adds orchestrator-specific operations.

use std::collections::HashSet;
use std::io::Write;

use byo::emitter::write_value;
use byo::tree;

use crate::id::QualifiedId;
use crate::process::ProcessId;

// Re-export tree types specialized for orchestrator use.
pub use tree::PropValue;

/// The orchestrator's object tree, keyed on [`QualifiedId`] with
/// [`ProcessId`] as per-node data (the owning process).
pub type ObjectTree = tree::ObjectTree<QualifiedId, ProcessId>;

/// A single object in the orchestrator's tree.
pub type ObjectState = tree::ObjectState<QualifiedId, ProcessId>;

/// Remove all objects owned by a specific process.
pub fn remove_by_owner(tree: &mut ObjectTree, owner: ProcessId) -> Vec<QualifiedId> {
    tree.remove_where(|obj| obj.data == owner)
}

/// Walk up parent links to find the nearest ancestor whose type is
/// in `observed_types`.
pub fn nearest_observed_ancestor(
    tree: &ObjectTree,
    qid: &QualifiedId,
    observed_types: &HashSet<String>,
) -> Option<QualifiedId> {
    tree.nearest_ancestor_where(qid, |kind| observed_types.contains(kind))
}

/// Project the full state tree for a set of observed types.
///
/// Walks all root nodes (objects with no parent) top-down, emitting only
/// observed types. Non-observed nodes are skipped but their children are
/// recursed into, naturally re-parenting observed descendants under their
/// nearest observed ancestor via `@ancestor id { +child ... }`.
pub fn project_tree(tree: &ObjectTree, observed_types: &HashSet<String>) -> Vec<u8> {
    let mut buf = Vec::new();

    // Find all root nodes (no parent).
    let mut roots: Vec<&QualifiedId> = tree.roots();
    roots.sort_by_key(|a| a.to_string());

    for root in roots {
        project_subtree(tree, root, observed_types, &mut buf);
    }
    buf
}

/// Recursively project a subtree rooted at `qid`.
///
/// If the node's type is observed, emit it as a `+kind qid props...`
/// with any observed descendants as children. If the node's type is
/// NOT observed, skip it and recurse into its children (they attach
/// to the parent context).
fn project_subtree(
    tree: &ObjectTree,
    qid: &QualifiedId,
    observed_types: &HashSet<String>,
    buf: &mut Vec<u8>,
) {
    let Some(obj) = tree.get(qid) else {
        return;
    };

    if observed_types.contains(&obj.kind) {
        // Emit this node.
        write_reduced_upsert(buf, obj);

        if has_observed_descendants(tree, qid, observed_types) {
            buf.extend_from_slice(b" {");
            for child in &obj.children {
                project_subtree(tree, child, observed_types, buf);
            }
            buf.extend_from_slice(b"\n}");
        }
    } else {
        // Skip this node, recurse into children.
        for child in &obj.children {
            project_subtree(tree, child, observed_types, buf);
        }
    }
}

/// Returns true if `qid` has any descendants whose type is in `observed_types`.
fn has_observed_descendants(
    tree: &ObjectTree,
    qid: &QualifiedId,
    observed_types: &HashSet<String>,
) -> bool {
    let Some(obj) = tree.get(qid) else {
        return false;
    };
    for child in &obj.children {
        if let Some(child_obj) = tree.get(child)
            && observed_types.contains(&child_obj.kind)
        {
            return true;
        }
        if has_observed_descendants(tree, child, observed_types) {
            return true;
        }
    }
    false
}

/// Write a `+kind qid props...` for a reduced object state.
fn write_reduced_upsert(buf: &mut Vec<u8>, obj: &ObjectState) {
    let _ = write!(buf, "\n+{} {}", obj.kind, obj.id);
    for (key, value) in &obj.props {
        match value {
            PropValue::Str(s) => {
                let _ = write!(buf, " {key}=");
                write_value(&mut *buf, s).unwrap();
            }
            PropValue::Flag => {
                let _ = write!(buf, " {key}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;
    use indexmap::IndexMap;

    fn pid(n: u32) -> ProcessId {
        ProcessId(n)
    }

    fn qid(client: &str, id: &str) -> QualifiedId {
        QualifiedId::new(client, id)
    }

    fn props(pairs: &[(&str, &str)]) -> IndexMap<String, PropValue> {
        let mut m = IndexMap::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), PropValue::Str(v.to_string()));
        }
        m
    }

    fn types(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn remove_by_owner_works() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "a"), &IndexMap::new(), pid(1));
        tree.upsert("view", &qid("app", "b"), &IndexMap::new(), pid(1));
        tree.upsert("view", &qid("daemon", "c"), &IndexMap::new(), pid(2));

        let removed = remove_by_owner(&mut tree, pid(1));
        assert_eq!(removed.len(), 2);
        assert_eq!(tree.len(), 1);
        assert!(tree.get(&qid("daemon", "c")).is_some());
    }

    #[test]
    fn project_all_observed() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view",
            &qid("app", "root"),
            &props(&[("class", "w-64")]),
            pid(1),
        );
        tree.upsert(
            "text",
            &qid("app", "label"),
            &props(&[("content", "Hi")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "label"), &qid("app", "root"));

        let observed = types(&["view", "text"]);
        let result = project_tree(&tree, &observed);
        assert_eq_bytes(
            &result,
            "+view app:root class=w-64 { +text app:label content=Hi }",
        );
    }

    #[test]
    fn project_skip_intermediate() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "btn"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view", "text"]);
        let result = project_tree(&tree, &observed);
        assert_eq_bytes(&result, "+view app:root { +text app:label }");
    }

    #[test]
    fn project_no_observed_root() {
        let mut tree = ObjectTree::new();
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["text"]);
        let result = project_tree(&tree, &observed);
        assert_eq_bytes(&result, "+text app:label");
    }

    #[test]
    fn project_empty_filter() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "root"), &IndexMap::new(), pid(1));

        let observed: HashSet<String> = HashSet::new();
        let result = project_tree(&tree, &observed);
        assert!(result.is_empty());
    }

    #[test]
    fn nearest_observed_ancestor_found() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "btn"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view"]);
        let ancestor = nearest_observed_ancestor(&tree, &qid("app", "label"), &observed);
        assert_eq!(ancestor, Some(qid("app", "root")));
    }

    #[test]
    fn nearest_observed_ancestor_not_found() {
        let mut tree = ObjectTree::new();
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view"]);
        let ancestor = nearest_observed_ancestor(&tree, &qid("app", "label"), &observed);
        assert_eq!(ancestor, None);
    }

    #[test]
    fn reduced_command_with_comma_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "x");
        tree.upsert("view", &id, &props(&[("data", "a,b")]), pid(1));

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+view app:x data=\"a,b\"");
    }
}
