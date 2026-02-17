//! Batch buffering and rewriting for daemon expansion.
//!
//! When a batch contains daemon-owned types, the orchestrator buffers it,
//! sends `!expand` events to daemons, collects their responses, and rewrites
//! the batch by splicing daemon expansions in place of the original commands.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use byo::protocol::Command;

use crate::id::{QualifiedId, qualify_into};
use crate::process::ProcessId;
use crate::router::RouterMsg;

/// A batch waiting for daemon expansion to complete.
#[derive(Debug)]
pub struct PendingBatch {
    /// The process that sent the batch.
    pub from: ProcessId,
    /// The client name of the sender.
    pub client_name: String,
    /// The pre-parsed commands from the original payload.
    pub commands: Vec<Command>,
    /// Outstanding `!expand` requests.
    pub pending_expands: Vec<PendingExpand>,
    /// QID → (daemon PID, qualified expansion commands, is_re_expand) from daemons.
    pub expansions: HashMap<String, (ProcessId, Vec<Command>, bool)>,
}

impl PendingBatch {
    pub fn new(from: ProcessId, client_name: String, commands: Vec<Command>) -> Self {
        Self {
            from,
            client_name,
            commands,
            pending_expands: Vec::new(),
            expansions: HashMap::new(),
        }
    }

    /// Returns true if all expansions have been received.
    pub fn is_ready(&self) -> bool {
        self.pending_expands.is_empty()
    }

    /// Register a pending expand request.
    pub fn add_pending_expand(
        &mut self,
        subscriber: ProcessId,
        seq: u64,
        qid: QualifiedId,
        depth: u32,
    ) {
        self.pending_expands.push(PendingExpand {
            subscriber,
            seq,
            qid,
            depth,
            is_re_expand: false,
        });
    }

    /// Register a re-expansion request (patch on claimed type).
    pub fn add_pending_re_expand(&mut self, subscriber: ProcessId, seq: u64, qid: QualifiedId) {
        self.pending_expands.push(PendingExpand {
            subscriber,
            seq,
            qid,
            depth: 0,
            is_re_expand: true,
        });
    }

    /// Complete an expand by (subscriber, seq). Returns the QID if found.
    pub fn complete_expand(&mut self, subscriber: ProcessId, seq: u64) -> Option<QualifiedId> {
        let pos = self
            .pending_expands
            .iter()
            .position(|e| e.subscriber == subscriber && e.seq == seq)?;
        let expand = self.pending_expands.remove(pos);
        Some(expand.qid)
    }

    /// Check if this batch has a pending expand from the given subscriber/seq.
    pub fn has_pending_expand(&self, subscriber: ProcessId, seq: u64) -> bool {
        self.pending_expands
            .iter()
            .any(|e| e.subscriber == subscriber && e.seq == seq)
    }

    /// Record an expansion result for a given qualified ID.
    pub fn record_expansion(
        &mut self,
        qid: &QualifiedId,
        owner: ProcessId,
        expansion_cmds: Vec<Command>,
        is_re_expand: bool,
    ) {
        self.expansions
            .insert(qid.to_string(), (owner, expansion_cmds, is_re_expand));
    }

    /// Rewrite the batch: splice expansions and qualify all IDs.
    /// Returns the rewritten payload bytes.
    ///
    /// For each command in the original:
    /// - If it's a daemon-owned type with an expansion, splice the expansion
    /// - Otherwise, re-emit with qualified IDs
    ///
    /// The `claims` map tells us which types are daemon-claimed.
    /// Rewrite result: the rewritten payload bytes plus any claimed-type
    /// destroys that need cascade handling by the router.
    pub fn rewrite(&self, claims: &HashMap<String, ProcessId>) -> (Vec<u8>, Vec<QualifiedId>) {
        let mut buf = Vec::new();
        let mut claimed_destroys = Vec::new();
        self.rewrite_commands(
            &self.commands,
            &self.client_name,
            claims,
            &mut buf,
            &mut claimed_destroys,
        );
        (buf, claimed_destroys)
    }

    /// Recursive rewrite helper. Walks commands, qualifies IDs under `client`,
    /// and splices expansions (which may themselves contain nested expansions).
    ///
    /// When `client` is empty, IDs are assumed already qualified (expansion content).
    /// Claimed-type destroys are collected instead of emitted (handled by router).
    fn rewrite_commands(
        &self,
        commands: &[byo::Command],
        client: &str,
        claims: &HashMap<String, ProcessId>,
        buf: &mut Vec<u8>,
        claimed_destroys: &mut Vec<QualifiedId>,
    ) {
        // Track push/pop depth for skipping children of daemon-owned objects.
        let mut skip_depth: Option<usize> = None;
        let mut skip_next_children = false;
        let mut depth: usize = 0;
        let mut qid_buf = String::new();

        for cmd in commands {
            // If we're inside a daemon-owned subtree, skip until we pop back.
            if let Some(skip_at) = skip_depth {
                match cmd {
                    byo::Command::Push => depth += 1,
                    byo::Command::Pop => {
                        if depth == skip_at {
                            skip_depth = None;
                        }
                        depth = depth.saturating_sub(1);
                    }
                    _ => {}
                }
                continue;
            }

            // Check if previous daemon-owned command is followed by a Push.
            if skip_next_children {
                skip_next_children = false;
                if matches!(cmd, byo::Command::Push) {
                    depth += 1;
                    skip_depth = Some(depth);
                    continue;
                }
                // Not a Push — fall through to normal processing.
            }

            match cmd {
                byo::Command::Upsert { kind, id, props } => {
                    qualify_into(&mut qid_buf, client, id);

                    if *id != "_" && self.expansions.contains_key(&*qid_buf) {
                        // Splice in the daemon expansion — recursively rewrite it.
                        let (_, expansion_cmds, _) = &self.expansions[&*qid_buf];
                        // Empty client: expansion IDs are already qualified.
                        self.rewrite_commands(expansion_cmds, "", claims, buf, claimed_destroys);
                        // If next command is Push, skip the children block.
                        skip_next_children = true;
                    } else if claims.contains_key(&**kind) && *id != "_" {
                        // Daemon-owned but no expansion (shouldn't happen
                        // if pending == 0, but handle gracefully).
                        skip_next_children = true;
                    } else {
                        write_upsert(buf, kind, &qid_buf, props);
                    }
                }
                byo::Command::Destroy { kind, id } => {
                    qualify_into(&mut qid_buf, client, id);
                    if claims.contains_key(&**kind) && *id != "_" {
                        // Claimed type: collect for cascade destroy by the router.
                        // Don't emit — compositor doesn't have this object.
                        if let Some(qid) = QualifiedId::parse(&qid_buf) {
                            claimed_destroys.push(qid);
                        }
                    } else {
                        let _ = write!(buf, "\n-{kind} {qid_buf}");
                    }
                }
                byo::Command::Push => {
                    depth += 1;
                    buf.extend_from_slice(b" {");
                }
                byo::Command::Pop => {
                    depth = depth.saturating_sub(1);
                    buf.extend_from_slice(b"\n}");
                }
                byo::Command::Patch { kind, id, props } => {
                    if claims.contains_key(&**kind) && *id != "_" {
                        // Claimed type: don't emit patch to compositor.
                        // Re-expansion is handled by the router.
                        // If next command is Push (children), skip them.
                        skip_next_children = true;
                    } else {
                        qualify_into(&mut qid_buf, client, id);
                        write_patch(buf, kind, &qid_buf, props);
                    }
                }
                byo::Command::Event { .. }
                | byo::Command::Ack { .. }
                | byo::Command::Request { .. }
                | byo::Command::Response { .. } => {
                    // Events/requests/responses in a batch are handled
                    // separately by the router.
                    let mut em = byo::emitter::Emitter::new(&mut *buf);
                    let _ = em.commands(std::slice::from_ref(cmd));
                }
            }
        }
    }
}

/// Write `+kind qid props...` directly to a buffer.
pub(crate) fn write_upsert(buf: &mut Vec<u8>, kind: &str, qid: &str, props: &[byo::Prop]) {
    let _ = write!(buf, "\n+{kind} {qid}");
    write_props(buf, props);
}

/// Write `@kind qid props...` directly to a buffer.
pub(crate) fn write_patch(buf: &mut Vec<u8>, kind: &str, qid: &str, props: &[byo::Prop]) {
    let _ = write!(buf, "\n@{kind} {qid}");
    write_props(buf, props);
}

/// Write props in wire format. Delegates to the canonical implementation
/// in [`byo::emitter::write_props`].
pub(crate) fn write_props(buf: &mut Vec<u8>, props: &[byo::Prop]) {
    byo::emitter::write_props(&mut *buf, props).unwrap();
}

/// Write a value, auto-quoting as needed. Delegates to the canonical
/// implementation in [`byo::emitter::write_value`].
pub(crate) fn write_value(buf: &mut Vec<u8>, value: &str) {
    byo::emitter::write_value(&mut *buf, value).unwrap();
}

/// Re-serialize commands with all IDs qualified under `client`.
///
/// Walks each command and qualifies bare IDs. Already-qualified IDs
/// (containing `:`) are left unchanged.
pub fn qualify_and_serialize(commands: &[byo::Command], client: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut qid_buf = String::new();

    for cmd in commands {
        match cmd {
            byo::Command::Upsert { kind, id, props } => {
                qualify_into(&mut qid_buf, client, id);
                write_upsert(&mut buf, kind, &qid_buf, props);
            }
            byo::Command::Destroy { kind, id } => {
                qualify_into(&mut qid_buf, client, id);
                let _ = write!(buf, "\n-{kind} {qid_buf}");
            }
            byo::Command::Push => {
                buf.extend_from_slice(b" {");
            }
            byo::Command::Pop => {
                buf.extend_from_slice(b"\n}");
            }
            byo::Command::Patch { kind, id, props } => {
                qualify_into(&mut qid_buf, client, id);
                write_patch(&mut buf, kind, &qid_buf, props);
            }
            byo::Command::Event { .. }
            | byo::Command::Ack { .. }
            | byo::Command::Request { .. }
            | byo::Command::Response { .. } => {
                let mut em = byo::emitter::Emitter::new(&mut buf);
                let _ = em.commands(std::slice::from_ref(cmd));
            }
        }
    }

    buf
}

/// Qualify bare IDs in commands under a client namespace.
///
/// Returns new commands with qualified IDs. ByteStr clone is cheap
/// (atomic refcount bump — no allocation for the common case).
pub fn qualify_commands(commands: &[Command], client: &str) -> Vec<Command> {
    use byo::ByteStr;

    fn qualify_bytestr(client: &str, id: &ByteStr) -> ByteStr {
        if id.contains(':') || client.is_empty() || **id == *"_" {
            id.clone()
        } else {
            ByteStr::from(format!("{client}:{id}"))
        }
    }

    commands
        .iter()
        .map(|cmd| match cmd {
            Command::Upsert { kind, id, props } => Command::Upsert {
                kind: kind.clone(),
                id: qualify_bytestr(client, id),
                props: props.clone(),
            },
            Command::Destroy { kind, id } => Command::Destroy {
                kind: kind.clone(),
                id: qualify_bytestr(client, id),
            },
            Command::Patch { kind, id, props } => Command::Patch {
                kind: kind.clone(),
                id: qualify_bytestr(client, id),
                props: props.clone(),
            },
            other => other.clone(),
        })
        .collect()
}

/// Maximum nesting depth for recursive daemon expansion.
pub const MAX_EXPANSION_DEPTH: u32 = 64;

/// A single outstanding `!expand` request within a pending batch.
#[derive(Debug)]
pub struct PendingExpand {
    /// The daemon that received the `!expand`.
    pub subscriber: ProcessId,
    /// The sequence number sent to that daemon.
    pub seq: u64,
    /// The qualified ID of the object being expanded.
    pub qid: QualifiedId,
    /// Current nesting depth (0 = top-level from app batch).
    pub depth: u32,
    /// True if this is a re-expansion (patch on claimed type), requiring reconciliation.
    pub is_re_expand: bool,
}

/// Per-process output queue for ordering guarantees.
///
/// While a batch is pending expansion, all subsequent output from that
/// process is held until the expansion completes.
#[derive(Debug, Default)]
pub struct OutputQueue {
    blocked: bool,
    queue: VecDeque<RouterMsg>,
}

impl OutputQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    pub fn block(&mut self) {
        self.blocked = true;
    }

    pub fn unblock(&mut self) {
        self.blocked = false;
    }

    pub fn push(&mut self, msg: RouterMsg) {
        self.queue.push_back(msg);
    }

    pub fn drain(&mut self) -> Vec<RouterMsg> {
        self.queue.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;

    fn pid(n: u32) -> ProcessId {
        ProcessId(n)
    }

    fn cmds(s: &str) -> Vec<Command> {
        byo::parser::parse(s).unwrap()
    }

    #[test]
    fn pending_batch_starts_ready() {
        let batch = PendingBatch::new(pid(1), "app".into(), cmds("+view sidebar"));
        assert!(batch.is_ready());
    }

    fn qid(client: &str, id: &str) -> QualifiedId {
        QualifiedId::new(client, id)
    }

    #[test]
    fn pending_batch_tracks_expansions() {
        let mut batch = PendingBatch::new(pid(1), "app".into(), cmds("+button save label=Save"));
        batch.add_pending_expand(pid(2), 0, qid("app", "save"), 0);
        assert!(!batch.is_ready());

        let id = batch.complete_expand(pid(2), 0).unwrap();
        batch.record_expansion(&id, pid(2), cmds("+view save-root class=btn"), false);
        assert!(batch.is_ready());
    }

    #[test]
    fn pending_batch_matches_by_subscriber_seq() {
        let mut batch = PendingBatch::new(pid(1), "app".into(), cmds("+button a +slider b"));
        batch.add_pending_expand(pid(2), 0, qid("app", "a"), 0);
        batch.add_pending_expand(pid(3), 0, qid("app", "b"), 0);
        assert!(!batch.is_ready());

        // Complete in reverse order — should work because we match by (subscriber, seq).
        let id = batch.complete_expand(pid(3), 0).unwrap();
        assert_eq!(id, qid("app", "b"));
        assert!(!batch.is_ready());

        let id = batch.complete_expand(pid(2), 0).unwrap();
        assert_eq!(id, qid("app", "a"));
        assert!(batch.is_ready());
    }

    #[test]
    fn complete_expand_wrong_seq_returns_none() {
        let mut batch = PendingBatch::new(pid(1), "app".into(), cmds("+button save"));
        batch.add_pending_expand(pid(2), 5, qid("app", "save"), 0);
        assert!(batch.complete_expand(pid(2), 99).is_none());
        assert!(!batch.is_ready());
    }

    #[test]
    fn rewrite_native_only() {
        let batch = PendingBatch::new(pid(1), "app".into(), cmds("+view sidebar class=w-64"));
        let subs = HashMap::new();
        let (result, _) = batch.rewrite(&subs);
        assert_eq_bytes(&result, "+view app:sidebar class=w-64");
    }

    #[test]
    fn rewrite_with_expansion() {
        let mut batch = PendingBatch::new(
            pid(1),
            "app".into(),
            cmds("+view root { +button save label=Save +view footer }"),
        );

        let mut subs = HashMap::new();
        subs.insert("button".to_string(), pid(2));

        batch.expansions.insert(
            "app:save".to_string(),
            (pid(2), cmds("+view controls:save-root class=btn"), false),
        );

        let (result, _) = batch.rewrite(&subs);
        assert_eq_bytes(
            &result,
            "+view app:root { +view controls:save-root class=btn +view app:footer }",
        );
    }

    #[test]
    fn rewrite_preserves_children_block() {
        let batch = PendingBatch::new(
            pid(1),
            "app".into(),
            cmds("+view root { +view child1 +view child2 }"),
        );
        let subs = HashMap::new();
        let (result, _) = batch.rewrite(&subs);
        assert_eq_bytes(
            &result,
            "+view app:root { +view app:child1 +view app:child2 }",
        );
    }

    #[test]
    fn output_queue_default() {
        let mut q = OutputQueue::new();
        assert!(!q.is_blocked());
        assert!(q.drain().is_empty());
    }

    #[test]
    fn qualify_and_serialize_basic() {
        let commands = byo::parser::parse("+view root class=w-64").unwrap();
        let result = qualify_and_serialize(&commands, "controls");
        assert_eq_bytes(&result, "+view controls:root class=w-64");
    }

    #[test]
    fn qualify_and_serialize_nested() {
        let commands = byo::parser::parse("+view root { +text label content=Hello }").unwrap();
        let result = qualify_and_serialize(&commands, "controls");
        assert_eq_bytes(
            &result,
            "+view controls:root { +text controls:label content=Hello }",
        );
    }

    #[test]
    fn qualify_and_serialize_already_qualified() {
        let commands = byo::parser::parse("+view app:sidebar").unwrap();
        let result = qualify_and_serialize(&commands, "controls");
        // Already-qualified IDs are left unchanged.
        assert_eq_bytes(&result, "+view app:sidebar");
    }

    #[test]
    fn qualify_and_serialize_destroy_and_patch() {
        let commands = byo::parser::parse("-view old @view sidebar hidden").unwrap();
        let result = qualify_and_serialize(&commands, "controls");
        assert_eq_bytes(&result, "-view controls:old @view controls:sidebar hidden");
    }

    #[test]
    fn rewrite_nested_expansion() {
        // App: +view root { +button save label=Save }
        // Controls expands button → +view controls:save-root { +icon controls:save-icon name=check }
        // Icon daemon expands icon → +image icons:check-img src=check.png
        let mut batch = PendingBatch::new(
            pid(1),
            "app".into(),
            cmds("+view root { +button save label=Save }"),
        );

        let mut claims = HashMap::new();
        claims.insert("button".to_string(), pid(2));
        claims.insert("icon".to_string(), pid(3));

        // Controls daemon expansion for app:save (already qualified).
        batch.expansions.insert(
            "app:save".to_string(),
            (
                pid(2),
                cmds("+view controls:save-root { +icon controls:save-icon name=check }"),
                false,
            ),
        );

        // Icon daemon expansion for controls:save-icon (already qualified).
        batch.expansions.insert(
            "controls:save-icon".to_string(),
            (pid(3), cmds("+image icons:check-img src=check.png"), false),
        );

        let (result, _) = batch.rewrite(&claims);
        assert_eq_bytes(
            &result,
            "+view app:root { +view controls:save-root { +image icons:check-img src=check.png } }",
        );
    }

    #[test]
    fn rewrite_claimed_destroy_collected() {
        // App sends `-button save` — should be collected as claimed destroy.
        let batch = PendingBatch::new(pid(1), "app".into(), cmds("-button save"));

        let mut claims = HashMap::new();
        claims.insert("button".to_string(), pid(2));

        let (result, claimed_destroys) = batch.rewrite(&claims);
        // Should produce empty output (compositor doesn't have this object).
        assert!(result.is_empty());
        // Should have collected the QID.
        assert_eq!(claimed_destroys.len(), 1);
        assert_eq!(claimed_destroys[0], qid("app", "save"));
    }

    #[test]
    fn rewrite_native_destroy_emitted() {
        // App sends `-view sidebar` — native type, should be emitted.
        let batch = PendingBatch::new(pid(1), "app".into(), cmds("-view sidebar"));

        let claims = HashMap::new();
        let (result, claimed_destroys) = batch.rewrite(&claims);
        assert_eq_bytes(&result, "-view app:sidebar");
        assert!(claimed_destroys.is_empty());
    }
}
