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

use std::collections::{HashMap, HashSet};

use byo::protocol::{Command, Prop};
use byo::tree::{self, ObjectKind, map_to_props};

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
pub fn project_tree(tree: &ObjectTree, observed_types: &HashSet<String>) -> Vec<Command> {
    let mut cmds = Vec::new();

    // Find all root nodes (no parent).
    let mut roots: Vec<&QualifiedId> = tree.roots();
    roots.sort_by_key(|a| a.to_string());

    for root in roots {
        project_subtree(tree, root, observed_types, &mut cmds);
    }
    cmds
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
    cmds: &mut Vec<Command>,
) {
    let Some(obj) = tree.get(qid) else {
        return;
    };

    let Some(kind) = obj.kind.as_type() else {
        // Slot or SlotContent node.
        match &obj.kind {
            ObjectKind::SlotContent => {
                // App-side slot content — skip here. Children will be
                // emitted at the expansion-side Slot's location via
                // slot_ref when the Slot node is encountered.
            }
            ObjectKind::Slot => {
                // Expansion-side slot placeholder — emit children of the
                // linked SlotContent (the app's content for this slot).
                if let Some(ref content_qid) = obj.slot_ref
                    && let Some(content_obj) = tree.get(content_qid)
                {
                    for child in &content_obj.children {
                        project_subtree(tree, child, observed_types, cmds);
                    }
                }
            }
            _ => {}
        }
        return;
    };
    if observed_types.contains(kind) {
        // Emit this object.
        let id = obj.id.to_string();
        cmds.push(Command::Upsert {
            kind: kind.into(),
            id: id.as_str().into(),
            props: map_to_props(&obj.props),
        });

        if has_observed_descendants(tree, qid, observed_types) {
            cmds.push(Command::Push { slot: None });
            for child in &obj.children {
                project_subtree(tree, child, observed_types, cmds);
            }
            cmds.push(Command::Pop);
        }
    } else {
        // Skip this object, recurse into children.
        for child in &obj.children {
            project_subtree(tree, child, observed_types, cmds);
        }
    }
}

/// Returns true if `qid` has any descendants whose type is in `observed_types`.
/// Follows slot_ref links so that Slot nodes check the linked SlotContent's
/// children for observed types.
fn has_observed_descendants(
    tree: &ObjectTree,
    qid: &QualifiedId,
    observed_types: &HashSet<String>,
) -> bool {
    let Some(obj) = tree.get(qid) else {
        return false;
    };
    for child in &obj.children {
        let Some(child_obj) = tree.get(child) else {
            continue;
        };
        match &child_obj.kind {
            ObjectKind::SlotContent => {
                // Skip — content will be checked at the Slot's location.
            }
            ObjectKind::Slot => {
                // Check the linked SlotContent's children.
                if let Some(ref content_qid) = child_obj.slot_ref
                    && has_observed_descendants(tree, content_qid, observed_types)
                {
                    return true;
                }
            }
            ObjectKind::Type(k) => {
                if observed_types.contains(k.as_str()) {
                    return true;
                }
                if has_observed_descendants(tree, child, observed_types) {
                    return true;
                }
            }
        }
    }
    false
}

/// Build a `Command::Upsert` for a reduced object.
///
/// Returns `None` if the object doesn't exist or is a slot node.
pub fn reduced_upsert(tree: &ObjectTree, qid: &QualifiedId) -> Option<Command> {
    let obj = tree.get(qid)?;
    let kind = obj.kind.as_type()?;
    let id = obj.id.to_string();
    Some(Command::Upsert {
        kind: kind.into(),
        id: id.as_str().into(),
        props: map_to_props(&obj.props),
    })
}

/// Get the kind and props for an expand request.
///
/// Returns `None` if the object doesn't exist or is a slot node.
pub fn expand_props(tree: &ObjectTree, qid: &QualifiedId) -> Option<(String, Vec<Prop>)> {
    let obj = tree.get(qid)?;
    let kind = obj.kind.as_type()?;
    let mut props = vec![Prop::val("kind", kind)];
    props.extend(map_to_props(&obj.props));
    Some((kind.to_string(), props))
}

/// Get the children of an object as `Command` values.
///
/// Slot children are emitted as `Push { slot: Some(...) }` blocks, and regular
/// type children as `Upsert { ... }` with `Push`/`Pop`. Used to reconstruct
/// app-provided slot content from the state tree during daemon crash
/// recovery replay.
pub fn children_commands(tree: &ObjectTree, qid: &QualifiedId) -> Vec<Command> {
    let Some(obj) = tree.get(qid) else {
        return Vec::new();
    };
    let mut cmds = Vec::new();
    for child_id in &obj.children {
        child_node_commands(tree, child_id, &mut cmds);
    }
    cmds
}

/// Extract prior slot contents from the state tree for a source object.
///
/// Finds all `SlotContent` children of the given object, builds their
/// children as `Command` values, and returns a map of slot name → commands.
/// Used to provide prior slot state for `@` (Patch) re-expansions.
pub fn collect_prior_slots(tree: &ObjectTree, qid: &QualifiedId) -> HashMap<String, Vec<Command>> {
    let Some(obj) = tree.get(qid) else {
        return HashMap::new();
    };
    let mut result = HashMap::new();
    for child_id in &obj.children {
        let Some(child_obj) = tree.get(child_id) else {
            continue;
        };
        if !matches!(child_obj.kind, ObjectKind::SlotContent) {
            continue;
        }
        debug_assert!(
            child_obj.slot_ref.is_some(),
            "SlotContent {} has no linked Slot — link_slot_refs not called?",
            child_id,
        );

        // Extract slot name from QID (e.g. `app:d::header` → `header`)
        let local = child_obj.id.local_id();
        let Some(sep_pos) = local.rfind("::") else {
            continue;
        };
        let name = &local[sep_pos + 2..];

        // Build commands for the slot's children
        let mut cmds = Vec::new();
        for grandchild_id in &child_obj.children {
            child_node_commands(tree, grandchild_id, &mut cmds);
        }

        if !cmds.is_empty() {
            result.insert(name.to_string(), cmds);
        }
    }
    result
}

/// Build commands for a single child node (slot or regular type).
fn child_node_commands(tree: &ObjectTree, qid: &QualifiedId, cmds: &mut Vec<Command>) {
    let Some(obj) = tree.get(qid) else {
        return;
    };
    match &obj.kind {
        ObjectKind::SlotContent => {
            // Slot QIDs use `parent::name` format (e.g. `app:d::header`).
            // Extract the bare slot name after `::` for the wire.
            let local = obj.id.local_id();
            let slot_name = local.rfind("::").map(|i| &local[i + 2..]).unwrap_or(local);
            cmds.push(Command::Push {
                slot: Some(slot_name.into()),
            });
            for child_id in &obj.children {
                child_node_commands(tree, child_id, cmds);
            }
            cmds.push(Command::Pop);
        }
        ObjectKind::Slot => {
            // Expansion-side slot placeholder — skip during serialization.
            // Content comes from the linked SlotContent node via slot_ref.
        }
        ObjectKind::Type(kind) => {
            let id = obj.id.to_string();
            cmds.push(Command::Upsert {
                kind: kind.as_str().into(),
                id: id.as_str().into(),
                props: map_to_props(&obj.props),
            });
            if !obj.children.is_empty() {
                cmds.push(Command::Push { slot: None });
                for child_id in &obj.children {
                    child_node_commands(tree, child_id, cmds);
                }
                cmds.push(Command::Pop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;
    use byo::emitter::Emitter;
    use indexmap::IndexMap;

    /// Serialize commands to bytes for test assertions.
    fn cmds_to_bytes(cmds: &[Command]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        let _ = em.commands(cmds);
        buf
    }

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
            &cmds_to_bytes(&result),
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
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            "+view app:root { +text app:label }",
        );
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
        assert_eq_bytes(&cmds_to_bytes(&result), "+text app:label");
    }

    #[test]
    fn project_empty_filter() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));

        let observed: HashSet<String> = HashSet::new();
        let result = project_tree(&tree, &observed);
        assert!(cmds_to_bytes(&result).is_empty());
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
    fn reduced_upsert_with_comma_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "x");
        tree.upsert("view".into(), &id, &props(&[("data", "a,b")]), pid(1));

        let cmd = reduced_upsert(&tree, &id).unwrap();
        let s = String::from_utf8(cmds_to_bytes(&[cmd])).unwrap();
        assert_eq!(s, "\n+view app:x data=\"a,b\"");
    }

    #[test]
    fn reduced_upsert_basic() {
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view".into(),
            &qid("app", "sidebar"),
            &props(&[("class", "w-64"), ("order", "0")]),
            pid(1),
        );

        let cmd = reduced_upsert(&tree, &qid("app", "sidebar")).unwrap();
        assert_eq_bytes(
            &cmds_to_bytes(&[cmd]),
            "+view app:sidebar class=w-64 order=0",
        );
    }

    #[test]
    fn reduced_upsert_with_flags() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        let mut p = props(&[("class", "w-64")]);
        p.insert("hidden".into(), PropValue::Flag);
        tree.upsert("view".into(), &id, &p, pid(1));

        let cmd = reduced_upsert(&tree, &id).unwrap();
        assert_eq_bytes(
            &cmds_to_bytes(&[cmd]),
            "+view app:sidebar class=w-64 hidden",
        );
    }

    #[test]
    fn reduced_upsert_quoted_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "label");
        tree.upsert(
            "text".into(),
            &id,
            &props(&[("content", "Hello, world")]),
            pid(1),
        );

        let cmd = reduced_upsert(&tree, &id).unwrap();
        let s = String::from_utf8(cmds_to_bytes(&[cmd])).unwrap();
        assert_eq!(s, "\n+text app:label content=\"Hello, world\"");
    }

    #[test]
    fn reduced_upsert_nonexistent() {
        let tree = ObjectTree::new();
        assert!(reduced_upsert(&tree, &qid("app", "nope")).is_none());
    }

    #[test]
    fn reduced_upsert_slot_content_skipped() {
        use byo::tree::ObjectKind;
        let mut tree = ObjectTree::new();
        tree.upsert(
            ObjectKind::SlotContent,
            &qid("app", "d::header"),
            &IndexMap::new(),
            pid(1),
        );
        assert!(reduced_upsert(&tree, &qid("app", "d::header")).is_none());
    }

    #[test]
    fn reduced_upsert_slot_skipped() {
        use byo::tree::ObjectKind;
        let mut tree = ObjectTree::new();
        tree.upsert(
            ObjectKind::Slot,
            &qid("controls", "hdr-wrap::header"),
            &IndexMap::new(),
            pid(2),
        );
        assert!(reduced_upsert(&tree, &qid("controls", "hdr-wrap::header")).is_none());
    }

    #[test]
    fn children_commands_basic() {
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "text".into(),
            &qid("app", "label"),
            &props(&[("content", "Hi")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "label"), &qid("app", "root"));

        let cmds = children_commands(&tree, &qid("app", "root"));
        assert!(!cmds.is_empty());
        assert_eq_bytes(&cmds_to_bytes(&cmds), "+text app:label content=Hi");
    }

    #[test]
    fn children_commands_with_slots() {
        use byo::tree::ObjectKind;
        let mut tree = ObjectTree::new();
        tree.upsert(
            "view".into(),
            &qid("app", "dialog"),
            &IndexMap::new(),
            pid(1),
        );

        // SlotContent node for "header" — QID: app:dialog::header
        tree.upsert(
            ObjectKind::SlotContent,
            &qid("app", "dialog::header"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "dialog::header"), &qid("app", "dialog"));

        // Child inside the header slot
        tree.upsert(
            "text".into(),
            &qid("app", "title"),
            &props(&[("content", "Hello")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "title"), &qid("app", "dialog::header"));

        // SlotContent node for default "_" — QID: app:dialog::_
        tree.upsert(
            ObjectKind::SlotContent,
            &qid("app", "dialog::_"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "dialog::_"), &qid("app", "dialog"));

        // Child inside default slot
        tree.upsert("view".into(), &qid("app", "body"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "body"), &qid("app", "dialog::_"));

        let cmds = children_commands(&tree, &qid("app", "dialog"));
        assert!(!cmds.is_empty());
        let s = String::from_utf8(cmds_to_bytes(&cmds)).unwrap();
        assert!(s.contains("::header {"), "expected ::header {{ in: {s}");
        assert!(
            s.contains("+text app:title content=Hello"),
            "expected title in: {s}"
        );
        assert!(s.contains("::_ {"), "expected ::_ {{ in: {s}");
        assert!(s.contains("+view app:body"), "expected body in: {s}");
    }

    #[test]
    fn children_commands_nested() {
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

        let cmds = children_commands(&tree, &qid("app", "root"));
        assert!(!cmds.is_empty());
        assert_eq_bytes(
            &cmds_to_bytes(&cmds),
            "+view app:child class=p-4 { +text app:label }",
        );
    }

    // ---------------------------------------------------------------
    // Slot-aware projection tests
    // ---------------------------------------------------------------

    /// Build a tree that mimics a scroll-view expansion:
    ///
    /// app:parent (view)
    ///   app:sv (scroll-view, NOT observed)
    ///     app:sv::_ (SlotContent) ← linked to controls:vp::_
    ///       app:content (view)
    ///         app:item (text)
    ///     controls:root (view)
    ///       controls:vp (view)
    ///         controls:vp::_ (Slot) ← linked to app:sv::_
    fn build_slot_tree() -> ObjectTree {
        let mut tree = ObjectTree::new();

        // App-side tree
        tree.upsert(
            "view".into(),
            &qid("app", "parent"),
            &IndexMap::new(),
            pid(1),
        );
        tree.upsert(
            "scroll-view".into(),
            &qid("app", "sv"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "sv"), &qid("app", "parent"));

        // SlotContent node (app's bare children wrapped retroactively)
        tree.upsert(
            ObjectKind::SlotContent,
            &qid("app", "sv::_"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "sv::_"), &qid("app", "sv"));

        // App content inside the slot
        tree.upsert(
            "view".into(),
            &qid("app", "content"),
            &props(&[("class", "p-2")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "content"), &qid("app", "sv::_"));

        tree.upsert(
            "text".into(),
            &qid("app", "item"),
            &props(&[("content", "Hello")]),
            pid(1),
        );
        tree.set_parent(&qid("app", "item"), &qid("app", "content"));

        // Expansion-side tree (daemon output)
        tree.upsert(
            "view".into(),
            &qid("controls", "root"),
            &props(&[("class", "relative")]),
            pid(2),
        );
        tree.set_parent(&qid("controls", "root"), &qid("app", "sv"));

        tree.upsert(
            "view".into(),
            &qid("controls", "vp"),
            &props(&[("overflow-y", "scroll")]),
            pid(2),
        );
        tree.set_parent(&qid("controls", "vp"), &qid("controls", "root"));

        // Expansion-side Slot placeholder
        tree.upsert(
            ObjectKind::Slot,
            &qid("controls", "vp::_"),
            &IndexMap::new(),
            pid(2),
        );
        tree.set_parent(&qid("controls", "vp::_"), &qid("controls", "vp"));

        // Link slot_ref bidirectionally
        tree.set_slot_ref(&qid("app", "sv::_"), &qid("controls", "vp::_"));
        tree.set_slot_ref(&qid("controls", "vp::_"), &qid("app", "sv::_"));

        tree
    }

    #[test]
    fn project_slot_content_inside_expansion_viewport() {
        let tree = build_slot_tree();
        let observed = types(&["view", "text"]);
        let result = project_tree(&tree, &observed);
        let s = String::from_utf8(cmds_to_bytes(&result)).unwrap();

        // App content should appear INSIDE the viewport, not as a sibling.
        assert!(
            s.contains("controls:vp"),
            "expected viewport in output: {s}"
        );
        assert!(
            s.contains("app:content"),
            "expected app content in output: {s}"
        );

        // Verify correct nesting: content is inside viewport's { }
        // Parse and check that content appears after vp's opening brace.
        let vp_pos = s.find("controls:vp").unwrap();
        let content_pos = s.find("app:content").unwrap();
        assert!(
            content_pos > vp_pos,
            "app:content should appear after controls:vp"
        );

        // Verify it's nested inside viewport (between its { and })
        let vp_brace = s[vp_pos..].find('{').map(|i| i + vp_pos).unwrap();
        assert!(
            content_pos > vp_brace,
            "app:content should be inside viewport's braces"
        );
    }

    #[test]
    fn project_slot_content_not_emitted_at_slot_content_location() {
        let tree = build_slot_tree();
        let observed = types(&["view", "text"]);
        let result = project_tree(&tree, &observed);
        let s = String::from_utf8(cmds_to_bytes(&result)).unwrap();

        // app:content should NOT appear as a sibling of controls:root
        // (i.e. directly under app:parent). It should only appear inside
        // the viewport via the slot_ref link.
        let parent_brace = s.find("app:parent").unwrap();
        let _parent_brace_open = s[parent_brace..]
            .find('{')
            .map(|i| i + parent_brace)
            .unwrap();

        // Find all occurrences of app:content — should be exactly one.
        let count = s.matches("app:content").count();
        assert_eq!(count, 1, "app:content should appear exactly once: {s}");

        // The single occurrence must be inside controls:vp's braces.
        let root_pos = s.find("controls:root").unwrap();
        let content_pos = s.find("app:content").unwrap();
        assert!(
            content_pos > root_pos,
            "app:content must be inside expansion, not before it: {s}"
        );
    }

    #[test]
    fn project_slot_unlinked_slot_emits_nothing() {
        // If a Slot has no linked SlotContent, it should emit nothing.
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "view".into(),
            &qid("controls", "vp"),
            &IndexMap::new(),
            pid(2),
        );
        tree.set_parent(&qid("controls", "vp"), &qid("app", "root"));

        // Slot with no slot_ref
        tree.upsert(
            ObjectKind::Slot,
            &qid("controls", "vp::_"),
            &IndexMap::new(),
            pid(2),
        );
        tree.set_parent(&qid("controls", "vp::_"), &qid("controls", "vp"));

        let observed = types(&["view"]);
        let result = project_tree(&tree, &observed);
        // Viewport should exist but have no children (empty slot).
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            "+view app:root { +view controls:vp }",
        );
    }

    #[test]
    fn has_observed_descendants_follows_slot_ref() {
        let tree = build_slot_tree();
        let observed = types(&["view", "text"]);

        // The viewport has a Slot child whose linked SlotContent has
        // observed descendants → should return true.
        assert!(has_observed_descendants(
            &tree,
            &qid("controls", "vp"),
            &observed
        ));
    }

    #[test]
    fn has_observed_descendants_skips_slot_content() {
        // SlotContent's children should NOT count at the SlotContent's
        // location — they're emitted at the Slot's location instead.
        let mut tree = ObjectTree::new();
        tree.upsert("view".into(), &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert(
            "scroll-view".into(),
            &qid("app", "sv"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "sv"), &qid("app", "root"));

        tree.upsert(
            ObjectKind::SlotContent,
            &qid("app", "sv::_"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "sv::_"), &qid("app", "sv"));

        tree.upsert(
            "view".into(),
            &qid("app", "child"),
            &IndexMap::new(),
            pid(1),
        );
        tree.set_parent(&qid("app", "child"), &qid("app", "sv::_"));

        let observed = types(&["view"]);

        // scroll-view (non-observed) has SlotContent with an observed child,
        // but SlotContent children should be skipped at this location.
        // Only an expansion-side Slot with a matching link would count.
        assert!(!has_observed_descendants(
            &tree,
            &qid("app", "sv"),
            &observed
        ));
    }
}
