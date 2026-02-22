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

use byo::emitter::Emitter;
use byo::tree::{self, ObjectKind};

use crate::id::QualifiedId;
use crate::process::ProcessId;

// Re-export tree types specialized for orchestrator use.
pub use tree::PropValue;

/// The orchestrator's object tree, keyed on [`QualifiedId`] with
/// [`ProcessId`] as per-object data (the owning process).
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
/// If the object's type is observed, emit it as a `+kind qid props...`
/// with any observed descendants as children. If the object's type is
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

    let Some(kind) = obj.kind.as_type() else {
        // Slot nodes are never observed — skip and recurse into children.
        for child in &obj.children {
            project_subtree(tree, child, observed_types, buf);
        }
        return;
    };
    if observed_types.contains(kind) {
        // Emit this object.
        let id = obj.id.to_string();
        let mut em = Emitter::new(&mut *buf);
        let _ = em.upsert(kind, &id, &obj.props);

        if has_observed_descendants(tree, qid, observed_types) {
            buf.extend_from_slice(b" {");
            for child in &obj.children {
                project_subtree(tree, child, observed_types, buf);
            }
            buf.extend_from_slice(b"\n}");
        }
    } else {
        // Skip this object, recurse into children.
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
            && child_obj
                .kind
                .as_type()
                .is_some_and(|k| observed_types.contains(k))
        {
            return true;
        }
        if has_observed_descendants(tree, child, observed_types) {
            return true;
        }
    }
    false
}

/// Append `\n+kind id props...` for a reduced object to `buf`.
///
/// Returns `true` if the object was found and written.
/// Slot nodes are never serialized.
pub fn write_reduced_upsert(tree: &ObjectTree, qid: &QualifiedId, buf: &mut Vec<u8>) -> bool {
    let Some(obj) = tree.get(qid) else {
        return false;
    };
    let Some(kind) = obj.kind.as_type() else {
        return false;
    };
    let id = obj.id.to_string();
    let mut em = Emitter::new(&mut *buf);
    let _ = em.upsert(kind, &id, &obj.props);
    true
}

/// Append ` kind={kind} props...` for an expand request to `buf`.
///
/// Returns `true` if the object was found and written.
pub fn write_expand_props(tree: &ObjectTree, qid: &QualifiedId, buf: &mut Vec<u8>) -> bool {
    let Some(obj) = tree.get(qid) else {
        return false;
    };
    let Some(kind) = obj.kind.as_type() else {
        return false;
    };
    let mut em = Emitter::new(&mut *buf);
    let _ = em.expand_props(kind, &obj.props);
    true
}

/// Write the children of an object as serialized BYO commands.
///
/// Slot children are emitted as `{slot_name ...}` blocks, and regular
/// type children as `+kind id props... { children }`. Used to reconstruct
/// app-provided slot content from the state tree during daemon crash
/// recovery replay.
///
/// Returns `true` if any children were written.
pub fn write_children(tree: &ObjectTree, qid: &QualifiedId, buf: &mut Vec<u8>) -> bool {
    let Some(obj) = tree.get(qid) else {
        return false;
    };
    if obj.children.is_empty() {
        return false;
    }
    for child_id in &obj.children {
        write_child_node(tree, child_id, buf);
    }
    true
}

/// Write a single child node (slot or regular type) into `buf`.
fn write_child_node(tree: &ObjectTree, qid: &QualifiedId, buf: &mut Vec<u8>) {
    let Some(obj) = tree.get(qid) else {
        return;
    };
    match &obj.kind {
        ObjectKind::Slot => {
            let id = obj.id.to_string();
            let mut em = Emitter::new(&mut *buf);
            let _ = em.slot_push(&id);
            for child_id in &obj.children {
                write_child_node(tree, child_id, buf);
            }
            let mut em = Emitter::new(&mut *buf);
            let _ = em.pop();
        }
        ObjectKind::Type(kind) => {
            let id = obj.id.to_string();
            if obj.children.is_empty() {
                let mut em = Emitter::new(&mut *buf);
                let _ = em.upsert(kind, &id, &obj.props);
            } else {
                let mut em = Emitter::new(&mut *buf);
                let _ = em.upsert(kind, &id, &obj.props);
                buf.extend_from_slice(b" {");
                for child_id in &obj.children {
                    write_child_node(tree, child_id, buf);
                }
                let mut em = Emitter::new(&mut *buf);
                let _ = em.pop();
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
        tree.upsert("view".into(), &qid("app", "a"), &IndexMap::new(), pid(1));
        tree.upsert("view".into(), &qid("app", "b"), &IndexMap::new(), pid(1));
        tree.upsert("view".into(), &qid("daemon", "c"), &IndexMap::new(), pid(2));

        let removed = remove_by_owner(&mut tree, pid(1));
        assert_eq!(removed.len(), 2);
        assert_eq!(tree.len(), 1);
        assert!(tree.get(&qid("daemon", "c")).is_some());
    }

    #[test]
    fn project_all_observed() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view".into(),
            &qid("app", "root"),
            &props(&[("class", "w-64")]),
            pid(1),
        );
        tree.upsert(
            "text".into(),
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
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "button".into(),
            &qid("app", "btn"),
            &IndexMap::new(),
            pid(1),
        );
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "btn"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view", "text"]);
        let result = project_tree(&tree, &observed);
        assert_eq_bytes(&result, "+view app:root { +text app:label }");
    }

    #[test]
    fn project_no_observed_root() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "button".into(),
            &qid("app", "btn"),
            &IndexMap::new(),
            pid(1),
        );
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["text"]);
        let result = project_tree(&tree, &observed);
        assert_eq_bytes(&result, "+text app:label");
    }

    #[test]
    fn project_empty_filter() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));

        let observed: HashSet<String> = HashSet::new();
        let result = project_tree(&tree, &observed);
        assert!(result.is_empty());
    }

    #[test]
    fn nearest_observed_ancestor_found() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "button".into(),
            &qid("app", "btn"),
            &IndexMap::new(),
            pid(1),
        );
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "btn"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view"]);
        let ancestor = nearest_observed_ancestor(&tree, &qid("app", "label"), &observed);
        assert_eq!(ancestor, Some(qid("app", "root")));
    }

    #[test]
    fn nearest_observed_ancestor_not_found() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "button".into(),
            &qid("app", "btn"),
            &IndexMap::new(),
            pid(1),
        );
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view"]);
        let ancestor = nearest_observed_ancestor(&tree, &qid("app", "label"), &observed);
        assert_eq!(ancestor, None);
    }

    #[test]
    fn write_reduced_upsert_with_comma_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "x");
        tree.upsert("view".into(), &id, &props(&[("data", "a,b")]), pid(1));

        let mut buf = Vec::new();
        assert!(write_reduced_upsert(&tree, &id, &mut buf));
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "\n+view app:x data=\"a,b\"");
    }

    #[test]
    fn write_reduced_upsert_basic() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view".into(),
            &qid("app", "sidebar"),
            &props(&[("class", "w-64"), ("order", "0")]),
            pid(1),
        );

        let mut buf = Vec::new();
        assert!(write_reduced_upsert(
            &tree,
            &qid("app", "sidebar"),
            &mut buf
        ));
        assert_eq_bytes(&buf, "+view app:sidebar class=w-64 order=0");
    }

    #[test]
    fn write_reduced_upsert_with_flags() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        let mut p = props(&[("class", "w-64")]);
        p.insert("hidden".into(), PropValue::Flag);
        tree.upsert("view".into(), &id, &p, pid(1));

        let mut buf = Vec::new();
        assert!(write_reduced_upsert(&tree, &id, &mut buf));
        assert_eq_bytes(&buf, "+view app:sidebar class=w-64 hidden");
    }

    #[test]
    fn write_reduced_upsert_quoted_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "label");
        tree.upsert(
            "text".into(),
            &id,
            &props(&[("content", "Hello, world")]),
            pid(1),
        );

        let mut buf = Vec::new();
        assert!(write_reduced_upsert(&tree, &id, &mut buf));
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "\n+text app:label content=\"Hello, world\"");
    }

    #[test]
    fn write_reduced_upsert_nonexistent() {
        let tree = ObjectTree::new();
        let mut buf = Vec::new();
        assert!(!write_reduced_upsert(&tree, &qid("app", "nope"), &mut buf));
        assert!(buf.is_empty());
    }

    #[test]
    fn write_reduced_upsert_slot_skipped() {
        use byo::tree::ObjectKind;
        let mut tree = ObjectTree::new();
        tree.upsert(
            ObjectKind::Slot,
            &qid("app", "header"),
            &IndexMap::new(),
            pid(1),
        );
        let mut buf = Vec::new();
        assert!(!write_reduced_upsert(
            &tree,
            &qid("app", "header"),
            &mut buf
        ));
        assert!(buf.is_empty());
    }

    #[test]
    fn write_children_basic() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &props(&[("content", "Hi")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "label"), &qid("app", "root"));

        let mut buf = Vec::new();
        assert!(write_children(&tree, &qid("app", "root"), &mut buf));
        assert_eq_bytes(&buf, "+text app:label content=Hi");
    }

    #[test]
    fn write_children_with_slots() {
        use byo::tree::ObjectKind;
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view".into(),
            &qid("app", "dialog"),
            &IndexMap::new(),
            pid(1),
        );

        // Slot node for "header"
        tree.upsert(
            ObjectKind::Slot,
            &qid("app", "header"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "header"), &qid("app", "dialog"));

        // Child inside the header slot
        tree.upsert(
            "text".into(),
            &qid("app", "title"),
            &props(&[("content", "Hello")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "title"), &qid("app", "header"));

        // Slot node for default "_"
        tree.upsert(ObjectKind::Slot, &qid("app", "_"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "_"), &qid("app", "dialog"));

        // Child inside default slot
        tree.upsert("view".into(), &qid("app", "body"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "body"), &qid("app", "_"));

        let mut buf = Vec::new();
        assert!(write_children(&tree, &qid("app", "dialog"), &mut buf));
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("{app:header"));
        assert!(s.contains("+text app:title content=Hello"));
        assert!(s.contains("{app:_"));
        assert!(s.contains("+view app:body"));
    }

    #[test]
    fn write_children_nested() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "view".into(),
            &qid("app", "child"),
            &props(&[("class", "p-4")]),
            pid(1),
        );
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "child"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "child"));

        let mut buf = Vec::new();
        assert!(write_children(&tree, &qid("app", "root"), &mut buf));
        assert_eq_bytes(&buf, "+view app:child class=p-4 { +text app:label }");
    }
}
