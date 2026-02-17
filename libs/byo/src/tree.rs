//! Generic object tree for maintaining BYO/OS state.
//!
//! Provides [`ObjectTree`], a generic tree of named objects with parent-child
//! relationships and arbitrary key-value props. Used by the orchestrator for
//! state reduction (crash recovery, late-daemon replay) and can be used by
//! clients (apps, compositor, debug tools) to maintain local state trees.

use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::io::Write;

use indexmap::IndexMap;

use crate::emitter::write_value;
use crate::protocol::{Command, Prop};

/// A stored property value (reduced from [`Prop`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropValue {
    Str(String),
    Flag,
}

/// A single object in the tree.
#[derive(Debug, Clone)]
pub struct ObjectState<K = String, D = ()> {
    pub kind: String,
    pub id: K,
    pub props: IndexMap<String, PropValue>,
    pub children: Vec<K>,
    pub parent: Option<K>,
    pub data: D,
}

/// A tree of objects with parent-child relationships.
#[derive(Debug)]
pub struct ObjectTree<K = String, D = ()> {
    objects: HashMap<K, ObjectState<K, D>>,
}

impl<K: Eq + Hash + Clone + Display, D: Clone> Default for ObjectTree<K, D> {
    fn default() -> Self {
        Self {
            objects: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone + Display, D: Clone> ObjectTree<K, D> {
    /// Create an empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert (full replace) an object. Replaces all props with the given set.
    /// Preserves children and parent relationships.
    pub fn upsert(&mut self, kind: &str, id: &K, props: &IndexMap<String, PropValue>, data: D) {
        if let Some(existing) = self.objects.get_mut(id) {
            existing.kind = kind.to_owned();
            existing.props = props.clone();
            existing.data = data;
        } else {
            self.objects.insert(
                id.clone(),
                ObjectState {
                    kind: kind.to_owned(),
                    id: id.clone(),
                    props: props.clone(),
                    children: Vec::new(),
                    parent: None,
                    data,
                },
            );
        }
    }

    /// Patch (merge) props onto an existing object.
    /// `set` adds/updates props, `remove` removes props.
    pub fn patch(&mut self, id: &K, set: &IndexMap<String, PropValue>, remove: &[&str]) {
        if let Some(obj) = self.objects.get_mut(id) {
            for (k, v) in set {
                obj.props.insert(k.clone(), v.clone());
            }
            for k in remove {
                obj.props.shift_remove(*k);
            }
        }
    }

    /// Set a parent-child relationship.
    pub fn set_parent(&mut self, child: &K, parent: &K) {
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
    pub fn destroy(&mut self, id: &K) {
        // Collect all descendants first to avoid borrow issues.
        let descendants = self.collect_descendants(id);

        // Remove from parent's children list.
        if let Some(obj) = self.objects.get(id)
            && let Some(ref parent_id) = obj.parent.clone()
            && let Some(parent) = self.objects.get_mut(parent_id)
        {
            parent.children.retain(|c| c != id);
        }

        // Remove the object and all descendants.
        for desc in &descendants {
            self.objects.remove(desc);
        }
        self.objects.remove(id);
    }

    /// Get an object by ID.
    pub fn get(&self, id: &K) -> Option<&ObjectState<K, D>> {
        self.objects.get(id)
    }

    /// Get all objects of a given type.
    pub fn objects_of_type(&self, kind: &str) -> Vec<&ObjectState<K, D>> {
        self.objects.values().filter(|o| o.kind == kind).collect()
    }

    /// Get all root-level IDs (objects with no parent).
    pub fn roots(&self) -> Vec<&K> {
        self.objects
            .values()
            .filter(|o| o.parent.is_none())
            .map(|o| &o.id)
            .collect()
    }

    /// Number of objects in the tree.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Returns true if the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Emit the reduced state of an object as wire-format bytes.
    ///
    /// Produces `+kind id props...` as a byte string suitable for
    /// re-emission via the emitter.
    pub fn reduced_command(&self, id: &K) -> Option<Vec<u8>> {
        let obj = self.objects.get(id)?;
        let mut buf = Vec::new();
        write!(buf, "+{} {}", obj.kind, obj.id).unwrap();
        for (key, value) in &obj.props {
            match value {
                PropValue::Str(s) => {
                    write!(buf, " {key}=").unwrap();
                    write_value(&mut buf, s).unwrap();
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
    /// Returns `(id, kind, data)` tuples for each descendant,
    /// useful for notification before a destroy cascades.
    pub fn descendants_info(&self, id: &K) -> Vec<(K, String, D)> {
        let mut result = Vec::new();
        if let Some(obj) = self.objects.get(id) {
            for child in &obj.children {
                if let Some(child_obj) = self.objects.get(child) {
                    result.push((
                        child.clone(),
                        child_obj.kind.clone(),
                        child_obj.data.clone(),
                    ));
                }
                result.extend(self.descendants_info(child));
            }
        }
        result
    }

    /// Remove all objects matching a predicate. Returns the removed IDs.
    ///
    /// Removes from parent children lists. Does NOT cascade to children
    /// of removed objects (use `destroy` for that).
    pub fn remove_where(&mut self, predicate: impl Fn(&ObjectState<K, D>) -> bool) -> Vec<K> {
        let matching: Vec<K> = self
            .objects
            .values()
            .filter(|o| predicate(o))
            .map(|o| o.id.clone())
            .collect();

        for id in &matching {
            // Remove from parent's children list.
            if let Some(obj) = self.objects.get(id)
                && let Some(ref parent_id) = obj.parent.clone()
                && let Some(parent) = self.objects.get_mut(parent_id)
            {
                parent.children.retain(|c| c != id);
            }
            self.objects.remove(id);
        }
        matching
    }

    /// Walk up parent links to find the nearest ancestor whose type matches
    /// a predicate.
    pub fn nearest_ancestor_where(&self, id: &K, predicate: impl Fn(&str) -> bool) -> Option<K> {
        let obj = self.objects.get(id)?;
        let mut current = obj.parent.clone()?;
        loop {
            let ancestor = self.objects.get(&current)?;
            if predicate(&ancestor.kind) {
                return Some(current);
            }
            current = ancestor.parent.clone()?;
        }
    }

    /// Apply parsed commands to the tree.
    ///
    /// Walks a `&[Command]` slice and calls upsert/patch/destroy/set_parent.
    /// Tracks parent stack for Push/Pop. Skips anonymous (`_`) objects.
    ///
    /// `make_key` converts bare `&str` IDs to `K`.
    /// `make_data` receives `(kind, id)` and returns per-object data.
    pub fn apply(
        &mut self,
        commands: &[Command<'_>],
        make_key: impl Fn(&str) -> K,
        mut make_data: impl FnMut(&str, &str) -> D,
    ) {
        let mut parent_stack: Vec<K> = Vec::new();
        let mut last_key: Option<K> = None;

        for cmd in commands {
            match cmd {
                Command::Upsert { kind, id, props } => {
                    if *id == "_" {
                        continue;
                    }
                    let key = make_key(id);
                    let map = props_to_map(props);
                    let data = make_data(kind, id);
                    self.upsert(kind, &key, &map, data);
                    if let Some(parent) = parent_stack.last() {
                        self.set_parent(&key, parent);
                    }
                    last_key = Some(key);
                }
                Command::Destroy { id, .. } => {
                    if *id == "_" {
                        continue;
                    }
                    let key = make_key(id);
                    self.destroy(&key);
                }
                Command::Push => {
                    if let Some(ref key) = last_key {
                        parent_stack.push(key.clone());
                    }
                }
                Command::Pop => {
                    parent_stack.pop();
                    last_key = parent_stack.last().cloned();
                }
                Command::Patch { id, props, .. } => {
                    if *id == "_" {
                        continue;
                    }
                    let key = make_key(id);
                    let (set, remove) = props_to_patch(props);
                    let remove_refs: Vec<&str> = remove.iter().map(|s| s.as_str()).collect();
                    self.patch(&key, &set, &remove_refs);
                    last_key = Some(key);
                }
                _ => {}
            }
        }
    }

    /// Collect all descendant IDs (depth-first).
    fn collect_descendants(&self, id: &K) -> Vec<K> {
        let mut result = Vec::new();
        if let Some(obj) = self.objects.get(id) {
            for child in &obj.children {
                result.push(child.clone());
                result.extend(self.collect_descendants(child));
            }
        }
        result
    }
}

/// Convert parsed props to a storage map.
///
/// [`Prop::Remove`] is a no-op in upsert context.
pub fn props_to_map(props: &[Prop<'_>]) -> IndexMap<String, PropValue> {
    let mut map = IndexMap::new();
    for prop in props {
        match prop {
            Prop::Value { key, value } => {
                map.insert(key.to_string(), PropValue::Str(value.to_string()));
            }
            Prop::Boolean { key } => {
                map.insert(key.to_string(), PropValue::Flag);
            }
            Prop::Remove { .. } => {} // Remove is a no-op in upsert context.
        }
    }
    map
}

/// Convert parsed props to patch operations (set + remove).
pub fn props_to_patch(props: &[Prop<'_>]) -> (IndexMap<String, PropValue>, Vec<String>) {
    let mut set = IndexMap::new();
    let mut remove = Vec::new();
    for prop in props {
        match prop {
            Prop::Value { key, value } => {
                set.insert(key.to_string(), PropValue::Str(value.to_string()));
            }
            Prop::Boolean { key } => {
                set.insert(key.to_string(), PropValue::Flag);
            }
            Prop::Remove { key } => {
                remove.push(key.to_string());
            }
        }
    }
    (set, remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert::assert_eq_bytes;

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

    // Use String keys and u32 data for simple generic tests.
    type TestTree = ObjectTree<String, u32>;

    fn key(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn upsert_creates_object() {
        let mut tree = TestTree::new();
        let id = key("sidebar");
        let p = props(&[("class", "w-64")]);
        tree.upsert("view", &id, &p, 1);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.kind, "view");
        assert_eq!(obj.props.len(), 1);
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
    }

    #[test]
    fn upsert_replaces_props() {
        let mut tree = TestTree::new();
        let id = key("sidebar");

        tree.upsert("view", &id, &props(&[("class", "w-64"), ("order", "0")]), 1);
        tree.upsert("view", &id, &props(&[("class", "w-32")]), 1);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props.len(), 1);
        assert_eq!(obj.props["class"], PropValue::Str("w-32".into()));
        assert!(!obj.props.contains_key("order"));
    }

    #[test]
    fn upsert_preserves_children() {
        let mut tree = TestTree::new();
        let parent = key("root");
        let child = key("child");

        tree.upsert("view", &parent, &IndexMap::new(), 1);
        tree.upsert("view", &child, &IndexMap::new(), 1);
        tree.set_parent(&child, &parent);

        // Re-upsert parent — children should be preserved.
        tree.upsert("view", &parent, &props(&[("class", "new")]), 1);
        let obj = tree.get(&parent).unwrap();
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0], child);
    }

    #[test]
    fn patch_merges_props() {
        let mut tree = TestTree::new();
        let id = key("sidebar");

        tree.upsert("view", &id, &props(&[("class", "w-64"), ("order", "0")]), 1);
        tree.patch(&id, &props(&[("order", "1")]), &[]);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
        assert_eq!(obj.props["order"], PropValue::Str("1".into()));
    }

    #[test]
    fn patch_removes_props() {
        let mut tree = TestTree::new();
        let id = key("sidebar");

        tree.upsert(
            "view",
            &id,
            &props(&[("class", "w-64"), ("tooltip", "hi")]),
            1,
        );
        tree.patch(&id, &IndexMap::new(), &["tooltip"]);

        let obj = tree.get(&id).unwrap();
        assert!(!obj.props.contains_key("tooltip"));
        assert_eq!(obj.props["class"], PropValue::Str("w-64".into()));
    }

    #[test]
    fn patch_adds_flags() {
        let mut tree = TestTree::new();
        let id = key("sidebar");

        tree.upsert("view", &id, &IndexMap::new(), 1);
        tree.patch(&id, &flags(&["hidden"]), &[]);

        let obj = tree.get(&id).unwrap();
        assert_eq!(obj.props["hidden"], PropValue::Flag);
    }

    #[test]
    fn destroy_removes_object() {
        let mut tree = TestTree::new();
        let id = key("sidebar");
        tree.upsert("view", &id, &IndexMap::new(), 1);
        assert_eq!(tree.len(), 1);

        tree.destroy(&id);
        assert_eq!(tree.len(), 0);
        assert!(tree.get(&id).is_none());
    }

    #[test]
    fn destroy_recursive() {
        let mut tree = TestTree::new();
        let root = key("root");
        let child = key("child");
        let grandchild = key("grandchild");

        tree.upsert("view", &root, &IndexMap::new(), 1);
        tree.upsert("view", &child, &IndexMap::new(), 1);
        tree.upsert("view", &grandchild, &IndexMap::new(), 1);
        tree.set_parent(&child, &root);
        tree.set_parent(&grandchild, &child);

        tree.destroy(&root);
        assert!(tree.is_empty());
    }

    #[test]
    fn destroy_removes_from_parent() {
        let mut tree = TestTree::new();
        let parent = key("parent");
        let child1 = key("child1");
        let child2 = key("child2");

        tree.upsert("view", &parent, &IndexMap::new(), 1);
        tree.upsert("view", &child1, &IndexMap::new(), 1);
        tree.upsert("view", &child2, &IndexMap::new(), 1);
        tree.set_parent(&child1, &parent);
        tree.set_parent(&child2, &parent);

        tree.destroy(&child1);
        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children.len(), 1);
        assert_eq!(p.children[0], child2);
    }

    #[test]
    fn remove_where_filters_by_data() {
        let mut tree = TestTree::new();
        tree.upsert("view", &key("a"), &IndexMap::new(), 1);
        tree.upsert("view", &key("b"), &IndexMap::new(), 1);
        tree.upsert("view", &key("c"), &IndexMap::new(), 2);

        let removed = tree.remove_where(|obj| obj.data == 1);
        assert_eq!(removed.len(), 2);
        assert_eq!(tree.len(), 1);
        assert!(tree.get(&key("c")).is_some());
    }

    #[test]
    fn objects_of_type() {
        let mut tree = TestTree::new();
        tree.upsert("view", &key("a"), &IndexMap::new(), 1);
        tree.upsert("button", &key("b"), &IndexMap::new(), 1);
        tree.upsert("view", &key("c"), &IndexMap::new(), 1);

        let views = tree.objects_of_type("view");
        assert_eq!(views.len(), 2);

        let buttons = tree.objects_of_type("button");
        assert_eq!(buttons.len(), 1);
    }

    #[test]
    fn set_parent_relationship() {
        let mut tree = TestTree::new();
        let parent = key("root");
        let child = key("child");
        tree.upsert("view", &parent, &IndexMap::new(), 1);
        tree.upsert("view", &child, &IndexMap::new(), 1);
        tree.set_parent(&child, &parent);

        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children, vec![child.clone()]);
        let c = tree.get(&child).unwrap();
        assert_eq!(c.parent, Some(parent));
    }

    #[test]
    fn set_parent_idempotent() {
        let mut tree = TestTree::new();
        let parent = key("root");
        let child = key("child");
        tree.upsert("view", &parent, &IndexMap::new(), 1);
        tree.upsert("view", &child, &IndexMap::new(), 1);
        tree.set_parent(&child, &parent);
        tree.set_parent(&child, &parent);

        let p = tree.get(&parent).unwrap();
        assert_eq!(p.children.len(), 1);
    }

    #[test]
    fn reduced_command_output() {
        let mut tree = TestTree::new();
        let id = key("app:sidebar");
        tree.upsert("view", &id, &props(&[("class", "w-64"), ("order", "0")]), 1);

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+view app:sidebar class=w-64 order=0");
    }

    #[test]
    fn reduced_command_with_flags() {
        let mut tree = TestTree::new();
        let id = key("app:sidebar");
        let mut p = props(&[("class", "w-64")]);
        p.insert("hidden".into(), PropValue::Flag);
        tree.upsert("view", &id, &p, 1);

        let bytes = tree.reduced_command(&id).unwrap();
        assert_eq_bytes(&bytes, "+view app:sidebar class=w-64 hidden");
    }

    #[test]
    fn reduced_command_quoted_value() {
        let mut tree = TestTree::new();
        let id = key("app:label");
        tree.upsert("text", &id, &props(&[("content", "Hello, world")]), 1);

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+text app:label content=\"Hello, world\"");
    }

    #[test]
    fn reduced_command_nonexistent() {
        let tree = TestTree::new();
        assert!(tree.reduced_command(&key("nope")).is_none());
    }

    #[test]
    fn descendants_info_collects_all() {
        let mut tree = TestTree::new();
        let root = key("save");
        let child = key("save-root");
        let grandchild = key("save-label");

        tree.upsert("button", &root, &IndexMap::new(), 1);
        tree.upsert("view", &child, &IndexMap::new(), 2);
        tree.upsert("text", &grandchild, &IndexMap::new(), 2);
        tree.set_parent(&child, &root);
        tree.set_parent(&grandchild, &child);

        let info = tree.descendants_info(&root);
        assert_eq!(info.len(), 2);
        assert_eq!(info[0].0, child);
        assert_eq!(info[0].1, "view");
        assert_eq!(info[0].2, 2);
        assert_eq!(info[1].0, grandchild);
        assert_eq!(info[1].1, "text");
        assert_eq!(info[1].2, 2);
    }

    #[test]
    fn nearest_ancestor_where_found() {
        let mut tree = TestTree::new();
        tree.upsert("view", &key("root"), &IndexMap::new(), 1);
        tree.upsert("button", &key("btn"), &IndexMap::new(), 1);
        tree.upsert("text", &key("label"), &IndexMap::new(), 1);
        tree.set_parent(&key("btn"), &key("root"));
        tree.set_parent(&key("label"), &key("btn"));

        let ancestor = tree.nearest_ancestor_where(&key("label"), |kind| kind == "view");
        assert_eq!(ancestor, Some(key("root")));
    }

    #[test]
    fn nearest_ancestor_where_not_found() {
        let mut tree = TestTree::new();
        tree.upsert("button", &key("btn"), &IndexMap::new(), 1);
        tree.upsert("text", &key("label"), &IndexMap::new(), 1);
        tree.set_parent(&key("label"), &key("btn"));

        let ancestor = tree.nearest_ancestor_where(&key("label"), |kind| kind == "view");
        assert_eq!(ancestor, None);
    }

    #[test]
    fn roots_returns_parentless() {
        let mut tree = TestTree::new();
        tree.upsert("view", &key("a"), &IndexMap::new(), 1);
        tree.upsert("view", &key("b"), &IndexMap::new(), 1);
        tree.upsert("view", &key("c"), &IndexMap::new(), 1);
        tree.set_parent(&key("c"), &key("a"));

        let roots = tree.roots();
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn apply_basic() {
        let mut tree = TestTree::new();
        let commands =
            crate::parser::parse("+view root class=w-64 { +text label content=Hello }").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);

        assert_eq!(tree.len(), 2);
        let root = tree.get(&key("root")).unwrap();
        assert_eq!(root.kind, "view");
        assert_eq!(root.props["class"], PropValue::Str("w-64".into()));
        assert_eq!(root.children, vec![key("label")]);

        let label = tree.get(&key("label")).unwrap();
        assert_eq!(label.kind, "text");
        assert_eq!(label.parent, Some(key("root")));
    }

    #[test]
    fn apply_destroy() {
        let mut tree = TestTree::new();
        let commands = crate::parser::parse("+view root +view child").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);
        assert_eq!(tree.len(), 2);

        let commands = crate::parser::parse("-view root").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn apply_patch() {
        let mut tree = TestTree::new();
        let commands = crate::parser::parse("+view sidebar class=w-64").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);

        let commands = crate::parser::parse("@view sidebar hidden ~class").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);

        let obj = tree.get(&key("sidebar")).unwrap();
        assert_eq!(obj.props["hidden"], PropValue::Flag);
        assert!(!obj.props.contains_key("class"));
    }

    #[test]
    fn apply_skips_anonymous() {
        let mut tree = TestTree::new();
        let commands = crate::parser::parse("+view _ +view named").unwrap();
        tree.apply(&commands, |id| id.to_string(), |_, _| 0);

        assert_eq!(tree.len(), 1);
        assert!(tree.get(&key("named")).is_some());
    }

    #[test]
    fn props_to_map_basic() {
        let props = vec![
            Prop::val("class", "w-64"),
            Prop::val("order", "0"),
            Prop::flag("hidden"),
        ];
        let map = props_to_map(&props);
        assert_eq!(map.len(), 3);
        assert_eq!(map["class"], PropValue::Str("w-64".into()));
        assert_eq!(map["order"], PropValue::Str("0".into()));
        assert_eq!(map["hidden"], PropValue::Flag);
    }

    #[test]
    fn props_to_patch_basic() {
        let props = vec![
            Prop::val("order", "1"),
            Prop::flag("hidden"),
            Prop::remove("tooltip"),
        ];
        let (set, remove) = props_to_patch(&props);
        assert_eq!(set.len(), 2);
        assert_eq!(remove, vec!["tooltip"]);
    }

    #[test]
    fn remove_where_removes_from_parent_children() {
        let mut tree = TestTree::new();
        tree.upsert("view", &key("parent"), &IndexMap::new(), 1);
        tree.upsert("view", &key("child"), &IndexMap::new(), 2);
        tree.set_parent(&key("child"), &key("parent"));

        tree.remove_where(|obj| obj.data == 2);
        let p = tree.get(&key("parent")).unwrap();
        assert!(p.children.is_empty());
    }

    #[test]
    fn reduced_command_with_comma_value() {
        let mut tree = TestTree::new();
        let id = key("x");
        tree.upsert("view", &id, &props(&[("data", "a,b")]), 1);

        let bytes = tree.reduced_command(&id).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s, "+view x data=\"a,b\"");
    }
}
