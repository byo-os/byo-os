//! Object tree state for crash recovery and late-daemon replay.
//!
//! The orchestrator maintains a reduced state for every object: the latest
//! `+` merged with all subsequent `@` patches, producing the equivalent of
//! a single `+` with the final props. This enables daemon crash recovery
//! and late daemon startup replay.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use indexmap::IndexMap;

use crate::id::QualifiedId;
use crate::process::ProcessId;

/// A stored property value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropValue {
    Str(String),
    Flag,
}

/// The reduced state of a single object.
#[derive(Debug, Clone)]
pub struct ObjectState {
    pub kind: String,
    pub qid: QualifiedId,
    pub props: IndexMap<String, PropValue>,
    pub children: Vec<QualifiedId>,
    pub parent: Option<QualifiedId>,
    pub owner: ProcessId,
}

/// The full object tree maintained by the orchestrator.
#[derive(Debug, Default)]
pub struct ObjectTree {
    objects: HashMap<QualifiedId, ObjectState>,
}

impl ObjectTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert (full replace) an object. Replaces all props with the given set.
    /// Preserves children and parent relationships.
    pub fn upsert(
        &mut self,
        kind: &str,
        qid: &QualifiedId,
        props: &IndexMap<String, PropValue>,
        owner: ProcessId,
    ) {
        if let Some(existing) = self.objects.get_mut(qid) {
            existing.kind = kind.to_owned();
            existing.props = props.clone();
            existing.owner = owner;
        } else {
            self.objects.insert(
                qid.clone(),
                ObjectState {
                    kind: kind.to_owned(),
                    qid: qid.clone(),
                    props: props.clone(),
                    children: Vec::new(),
                    parent: None,
                    owner,
                },
            );
        }
    }

    /// Patch (merge) props onto an existing object.
    /// `set` adds/updates props, `remove` removes props.
    pub fn patch(&mut self, qid: &QualifiedId, set: &IndexMap<String, PropValue>, remove: &[&str]) {
        if let Some(obj) = self.objects.get_mut(qid) {
            for (k, v) in set {
                obj.props.insert(k.clone(), v.clone());
            }
            for k in remove {
                obj.props.shift_remove(*k);
            }
        }
    }

    /// Set a parent-child relationship.
    pub fn set_parent(&mut self, child: &QualifiedId, parent: &QualifiedId) {
        if let Some(obj) = self.objects.get_mut(parent)
            && !obj.children.contains(child)
        {
            obj.children.push(child.clone());
        }
        if let Some(obj) = self.objects.get_mut(child) {
            obj.parent = Some(parent.clone());
        }
    }

    /// Destroy an object and all its descendants recursively.
    pub fn destroy(&mut self, qid: &QualifiedId) {
        // Collect all descendants first to avoid borrow issues.
        let descendants = self.collect_descendants(qid);

        // Remove from parent's children list.
        if let Some(obj) = self.objects.get(qid)
            && let Some(ref parent_qid) = obj.parent.clone()
            && let Some(parent) = self.objects.get_mut(parent_qid)
        {
            parent.children.retain(|c| c != qid);
        }

        // Remove the object and all descendants.
        for desc in &descendants {
            self.objects.remove(desc);
        }
        self.objects.remove(qid);
    }

    /// Collect all descendant QIDs (depth-first).
    fn collect_descendants(&self, qid: &QualifiedId) -> Vec<QualifiedId> {
        let mut result = Vec::new();
        if let Some(obj) = self.objects.get(qid) {
            for child in &obj.children {
                result.push(child.clone());
                result.extend(self.collect_descendants(child));
            }
        }
        result
    }

    /// Remove all objects owned by a process.
    pub fn remove_by_owner(&mut self, owner: ProcessId) -> Vec<QualifiedId> {
        let owned: Vec<QualifiedId> = self
            .objects
            .values()
            .filter(|o| o.owner == owner)
            .map(|o| o.qid.clone())
            .collect();

        for qid in &owned {
            // Remove from parent's children list.
            if let Some(obj) = self.objects.get(qid)
                && let Some(ref parent_qid) = obj.parent.clone()
                && let Some(parent) = self.objects.get_mut(parent_qid)
            {
                parent.children.retain(|c| c != qid);
            }
            self.objects.remove(qid);
        }
        owned
    }

    /// Get all objects of a given type.
    pub fn objects_of_type(&self, kind: &str) -> Vec<&ObjectState> {
        self.objects.values().filter(|o| o.kind == kind).collect()
    }

    /// Get an object by QID.
    pub fn get(&self, qid: &QualifiedId) -> Option<&ObjectState> {
        self.objects.get(qid)
    }

    /// Emit the reduced state of an object as wire-format bytes.
    ///
    /// Produces `+kind qid props...` as a byte string suitable for
    /// re-emission via the emitter.
    pub fn reduced_command(&self, qid: &QualifiedId) -> Option<Vec<u8>> {
        let obj = self.objects.get(qid)?;
        let mut buf = Vec::new();
        write!(buf, "+{} {}", obj.kind, obj.qid).unwrap();
        for (key, value) in &obj.props {
            match value {
                PropValue::Str(s) => {
                    write!(buf, " {key}=").unwrap();
                    write_quoted_value(&mut buf, s);
                }
                PropValue::Flag => {
                    write!(buf, " {key}").unwrap();
                }
            }
        }
        Some(buf)
    }

    /// Collect info about all descendants (depth-first).
    ///
    /// Returns `(qid, kind, owner)` tuples for each descendant,
    /// useful for notification before a destroy cascades.
    pub fn descendants_info(&self, qid: &QualifiedId) -> Vec<(QualifiedId, String, ProcessId)> {
        let mut result = Vec::new();
        if let Some(obj) = self.objects.get(qid) {
            for child in &obj.children {
                if let Some(child_obj) = self.objects.get(child) {
                    result.push((child.clone(), child_obj.kind.clone(), child_obj.owner));
                }
                result.extend(self.descendants_info(child));
            }
        }
        result
    }

    /// Walk up parent links to find the nearest ancestor whose type is
    /// in `observed_types`.
    pub fn nearest_observed_ancestor(
        &self,
        qid: &QualifiedId,
        observed_types: &HashSet<String>,
    ) -> Option<QualifiedId> {
        let obj = self.objects.get(qid)?;
        let mut current = obj.parent.clone()?;
        loop {
            let ancestor = self.objects.get(&current)?;
            if observed_types.contains(&ancestor.kind) {
                return Some(current);
            }
            current = ancestor.parent.clone()?;
        }
    }

    /// Project the full state tree for a set of observed types.
    ///
    /// Walks all root nodes (objects with no parent) top-down, emitting only
    /// observed types. Non-observed nodes are skipped but their children are
    /// recursed into, naturally re-parenting observed descendants under their
    /// nearest observed ancestor via `@ancestor id { +child ... }`.
    pub fn project_tree(&self, observed_types: &HashSet<String>) -> Vec<u8> {
        let mut buf = Vec::new();

        // Find all root nodes (no parent).
        let mut roots: Vec<&QualifiedId> = self
            .objects
            .values()
            .filter(|o| o.parent.is_none())
            .map(|o| &o.qid)
            .collect();
        roots.sort_by_key(|a| a.to_string());

        for root in roots {
            self.project_subtree(root, observed_types, &mut buf);
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
        &self,
        qid: &QualifiedId,
        observed_types: &HashSet<String>,
        buf: &mut Vec<u8>,
    ) {
        let Some(obj) = self.objects.get(qid) else {
            return;
        };

        if observed_types.contains(&obj.kind) {
            // Emit this node.
            write_reduced_upsert(buf, obj);

            if self.has_observed_descendants(qid, observed_types) {
                buf.extend_from_slice(b" {");
                for child in &obj.children {
                    self.project_subtree(child, observed_types, buf);
                }
                buf.extend_from_slice(b"\n}");
            }
        } else {
            // Skip this node, recurse into children.
            for child in &obj.children {
                self.project_subtree(child, observed_types, buf);
            }
        }
    }

    /// Returns true if `qid` has any descendants whose type is in `observed_types`.
    fn has_observed_descendants(
        &self,
        qid: &QualifiedId,
        observed_types: &HashSet<String>,
    ) -> bool {
        let Some(obj) = self.objects.get(qid) else {
            return false;
        };
        for child in &obj.children {
            if let Some(child_obj) = self.objects.get(child)
                && observed_types.contains(&child_obj.kind)
            {
                return true;
            }
            if self.has_observed_descendants(child, observed_types) {
                return true;
            }
        }
        false
    }

    /// Get all root-level QIDs (objects with no parent).
    #[allow(dead_code)]
    pub fn roots(&self) -> Vec<&QualifiedId> {
        self.objects
            .values()
            .filter(|o| o.parent.is_none())
            .map(|o| &o.qid)
            .collect()
    }

    /// Number of objects in the tree.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Write a `+kind qid props...` for a reduced object state.
fn write_reduced_upsert(buf: &mut Vec<u8>, obj: &ObjectState) {
    let _ = write!(buf, "\n+{} {}", obj.kind, obj.qid);
    for (key, value) in &obj.props {
        match value {
            PropValue::Str(s) => {
                let _ = write!(buf, " {key}=");
                write_quoted_value(buf, s);
            }
            PropValue::Flag => {
                let _ = write!(buf, " {key}");
            }
        }
    }
}

/// Write a value, quoting if necessary (same rules as emitter).
fn write_quoted_value(buf: &mut Vec<u8>, value: &str) {
    let needs_quoting = value.is_empty()
        || value.bytes().any(|b| {
            b.is_ascii_whitespace()
                || b == b'{'
                || b == b'}'
                || b == b'='
                || b == b'"'
                || b == b'\''
                || b == b'~'
                || b == b'\\'
        });

    if !needs_quoting {
        buf.extend_from_slice(value.as_bytes());
    } else if !value.contains('"') {
        buf.push(b'"');
        buf.extend_from_slice(value.as_bytes());
        buf.push(b'"');
    } else if !value.contains('\'') {
        buf.push(b'\'');
        buf.extend_from_slice(value.as_bytes());
        buf.push(b'\'');
    } else {
        buf.push(b'"');
        for ch in value.bytes() {
            match ch {
                b'"' => buf.extend_from_slice(b"\\\""),
                b'\\' => buf.extend_from_slice(b"\\\\"),
                _ => buf.push(ch),
            }
        }
        buf.push(b'"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;

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

    fn flags(names: &[&str]) -> IndexMap<String, PropValue> {
        let mut m = IndexMap::new();
        for name in names {
            m.insert(name.to_string(), PropValue::Flag);
        }
        m
    }

    #[test]
    fn upsert_creates_object() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        let p = props(&[("class", "w-64")]);
        tree.upsert("view", &id, &p, pid(1));

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.kind, "view");
        assert_eq!(obj.props.len(), 1);
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
    }

    #[test]
    fn upsert_replaces_props() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");

        tree.upsert(
            "view",
            &id,
            &props(&[("class", "w-64"), ("order", "0")]),
            pid(1),
        );
        tree.upsert("view", &id, &props(&[("class", "w-32")]), pid(1));

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props.len(), 1);
        assert_eq!(obj.props["class"], PropValue::Str("w-32".into()));
        assert!(!obj.props.contains_key("order"));
    }

    #[test]
    fn upsert_preserves_children() {
        let mut tree = ObjectTree::new();
        let parent = qid("app", "root");
        let child = qid("app", "child");

        tree.upsert("view", &parent, &IndexMap::new(), pid(1));
        tree.upsert("view", &child, &IndexMap::new(), pid(1));
        tree.set_parent(&child, &parent);

        // Re-upsert parent — children should be preserved.
        tree.upsert("view", &parent, &props(&[("class", "new")]), pid(1));
        let obj = tree.get(&parent).unwrap();
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0], child);
    }

    #[test]
    fn patch_merges_props() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");

        tree.upsert(
            "view",
            &id,
            &props(&[("class", "w-64"), ("order", "0")]),
            pid(1),
        );
        tree.patch(&id, &props(&[("order", "1")]), &[]);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
        assert_eq!(obj.props["order"], PropValue::Str("1".into()));
    }

    #[test]
    fn patch_removes_props() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");

        tree.upsert(
            "view",
            &id,
            &props(&[("class", "w-64"), ("tooltip", "hi")]),
            pid(1),
        );
        tree.patch(&id, &IndexMap::new(), &["tooltip"]);

        let obj = tree.get(&id).unwrap();
        assert!(!obj.props.contains_key("tooltip"));
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
    }

    #[test]
    fn patch_adds_flags() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");

        tree.upsert("view", &id, &IndexMap::new(), pid(1));
        tree.patch(&id, &flags(&["hidden"]), &[]);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props["hidden"], PropValue::Flag);
    }

    #[test]
    fn destroy_removes_object() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        tree.upsert("view", &id, &IndexMap::new(), pid(1));
        assert_eq!(tree.len(), 1);

        tree.destroy(&id);
        assert_eq!(tree.len(), 0);
        assert!(tree.get(&id).is_none());
    }

    #[test]
    fn destroy_recursive() {
        let mut tree = ObjectTree::new();
        let root = qid("app", "root");
        let child = qid("app", "child");
        let grandchild = qid("app", "grandchild");

        tree.upsert("view", &root, &IndexMap::new(), pid(1));
        tree.upsert("view", &child, &IndexMap::new(), pid(1));
        tree.upsert("view", &grandchild, &IndexMap::new(), pid(1));
        tree.set_parent(&child, &root);
        tree.set_parent(&grandchild, &child);

        tree.destroy(&root);
        assert!(tree.is_empty());
    }

    #[test]
    fn destroy_removes_from_parent() {
        let mut tree = ObjectTree::new();
        let parent = qid("app", "parent");
        let child1 = qid("app", "child1");
        let child2 = qid("app", "child2");

        tree.upsert("view", &parent, &IndexMap::new(), pid(1));
        tree.upsert("view", &child1, &IndexMap::new(), pid(1));
        tree.upsert("view", &child2, &IndexMap::new(), pid(1));
        tree.set_parent(&child1, &parent);
        tree.set_parent(&child2, &parent);

        tree.destroy(&child1);
        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children.len(), 1);
        assert_eq!(p.children[0], child2);
    }

    #[test]
    fn remove_by_owner() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "a"), &IndexMap::new(), pid(1));
        tree.upsert("view", &qid("app", "b"), &IndexMap::new(), pid(1));
        tree.upsert("view", &qid("daemon", "c"), &IndexMap::new(), pid(2));

        let removed = tree.remove_by_owner(pid(1));
        assert_eq!(removed.len(), 2);
        assert_eq!(tree.len(), 1);
        assert!(tree.get(&qid("daemon", "c")).is_some());
    }

    #[test]
    fn objects_of_type() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "a"), &IndexMap::new(), pid(1));
        tree.upsert("button", &qid("app", "b"), &IndexMap::new(), pid(1));
        tree.upsert("view", &qid("app", "c"), &IndexMap::new(), pid(1));

        let views = tree.objects_of_type("view");
        assert_eq!(views.len(), 2);

        let buttons = tree.objects_of_type("button");
        assert_eq!(buttons.len(), 1);
    }

    #[test]
    fn reduced_command_output() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        tree.upsert(
            "view",
            &id,
            &props(&[("class", "w-64"), ("order", "0")]),
            pid(1),
        );

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+view app:sidebar class=w-64 order=0");
    }

    #[test]
    fn reduced_command_with_flags() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "sidebar");
        let mut p = props(&[("class", "w-64")]);
        p.insert("hidden".into(), PropValue::Flag);
        tree.upsert("view", &id, &p, pid(1));

        let bytes = tree.reduced_command(&id).unwrap();
        assert_eq_bytes(&bytes, "+view app:sidebar class=w-64 hidden");
    }

    #[test]
    fn reduced_command_quoted_value() {
        let mut tree = ObjectTree::new();
        let id = qid("app", "label");
        tree.upsert("text", &id, &props(&[("content", "Hello, world")]), pid(1));

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+text app:label content=\"Hello, world\"");
    }

    #[test]
    fn reduced_command_nonexistent() {
        let tree = ObjectTree::new();
        assert!(tree.reduced_command(&qid("app", "nope")).is_none());
    }

    #[test]
    fn set_parent_relationship() {
        let mut tree = ObjectTree::new();
        let parent = qid("app", "root");
        let child = qid("app", "child");
        tree.upsert("view", &parent, &IndexMap::new(), pid(1));
        tree.upsert("view", &child, &IndexMap::new(), pid(1));
        tree.set_parent(&child, &parent);

        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children, vec![child.clone()]);
        let c = tree.get(&child).unwrap();
        assert_eq!(c.parent, Some(parent));
    }

    #[test]
    fn set_parent_idempotent() {
        let mut tree = ObjectTree::new();
        let parent = qid("app", "root");
        let child = qid("app", "child");
        tree.upsert("view", &parent, &IndexMap::new(), pid(1));
        tree.upsert("view", &child, &IndexMap::new(), pid(1));
        tree.set_parent(&child, &parent);
        tree.set_parent(&child, &parent);

        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children.len(), 1);
    }

    fn types(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
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
        let result = tree.project_tree(&observed);
        assert_eq_bytes(
            &result,
            "+view app:root class=w-64 { +text app:label content=Hi }",
        );
    }

    #[test]
    fn project_skip_intermediate() {
        // view → button → text
        // Observe view + text, skip button
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "root"), &IndexMap::new(), pid(1));
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "btn"), &qid("app", "root"));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view", "text"]);
        let result = tree.project_tree(&observed);
        // text should be re-parented under view (button skipped)
        assert_eq_bytes(&result, "+view app:root { +text app:label }");
    }

    #[test]
    fn project_no_observed_root() {
        // button → text (only text observed)
        let mut tree = ObjectTree::new();
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["text"]);
        let result = tree.project_tree(&observed);
        // button skipped, only text emitted
        assert_eq_bytes(&result, "+text app:label");
    }

    #[test]
    fn project_empty_filter() {
        let mut tree = ObjectTree::new();
        tree.upsert("view", &qid("app", "root"), &IndexMap::new(), pid(1));

        let observed: HashSet<String> = HashSet::new();
        let result = tree.project_tree(&observed);
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
        let ancestor = tree.nearest_observed_ancestor(&qid("app", "label"), &observed);
        assert_eq!(ancestor, Some(qid("app", "root")));
    }

    #[test]
    fn nearest_observed_ancestor_not_found() {
        let mut tree = ObjectTree::new();
        tree.upsert("button", &qid("app", "btn"), &IndexMap::new(), pid(1));
        tree.upsert("text", &qid("app", "label"), &IndexMap::new(), pid(1));
        tree.set_parent(&qid("app", "label"), &qid("app", "btn"));

        let observed = types(&["view"]);
        let ancestor = tree.nearest_observed_ancestor(&qid("app", "label"), &observed);
        assert_eq!(ancestor, None);
    }

    #[test]
    fn descendants_info_collects_all() {
        let mut tree = ObjectTree::new();
        let root = qid("app", "save");
        let child = qid("controls", "save-root");
        let grandchild = qid("controls", "save-label");

        tree.upsert("button", &root, &IndexMap::new(), pid(1));
        tree.upsert("view", &child, &IndexMap::new(), pid(2));
        tree.upsert("text", &grandchild, &IndexMap::new(), pid(2));
        tree.set_parent(&child, &root);
        tree.set_parent(&grandchild, &child);

        let info = tree.descendants_info(&root);
        assert_eq!(info.len(), 2);
        // Direct child first, then grandchild.
        assert_eq!(info[0].0, child);
        assert_eq!(info[0].1, "view");
        assert_eq!(info[0].2, pid(2));
        assert_eq!(info[1].0, grandchild);
        assert_eq!(info[1].1, "text");
        assert_eq!(info[1].2, pid(2));
    }
}
