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

/// How to handle the children block following a daemon-owned command.
enum SkipMode {
    /// Pure skip: no slot collection (claimed but no expansion, or no children).
    Skip,
    /// Collect slot contents from children, then splice into the expansion.
    CollectSlots {
        expansion_cmds: Vec<byo::Command>,
        app_client: String,
    },
}

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
    /// True if this is a replay batch (from `#claim` — daemon restart recovery).
    pub is_replay: bool,
}

impl PendingBatch {
    pub fn new(from: ProcessId, client_name: String, commands: Vec<Command>) -> Self {
        Self {
            from,
            client_name,
            commands,
            pending_expands: Vec::new(),
            expansions: HashMap::new(),
            is_replay: false,
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
            None,
        );
        (buf, claimed_destroys)
    }

    /// Recursive rewrite helper. Walks commands, qualifies IDs under `client`,
    /// and splices expansions (which may themselves contain nested expansions).
    ///
    /// When `client` is empty, IDs are assumed already qualified (expansion content).
    /// Claimed-type destroys are collected instead of emitted (handled by router).
    /// `slot_contents` carries app-provided slot children for the current expansion scope.
    fn rewrite_commands(
        &self,
        commands: &[byo::Command],
        client: &str,
        claims: &HashMap<String, ProcessId>,
        buf: &mut Vec<u8>,
        claimed_destroys: &mut Vec<QualifiedId>,
        slot_contents: Option<&HashMap<String, Vec<byo::Command>>>,
    ) {
        // Track push/pop depth for skipping children of daemon-owned objects.
        let mut skip_depth: Option<usize> = None;
        let mut skip_next_children: Option<SkipMode> = None;
        let mut depth: usize = 0;
        let mut qid_buf = String::new();
        let mut i = 0;

        while i < commands.len() {
            let cmd = &commands[i];

            // If we're inside a daemon-owned subtree, skip until we pop back.
            if let Some(skip_at) = skip_depth {
                match cmd {
                    byo::Command::Push { .. } => depth += 1,
                    byo::Command::Pop => {
                        if depth == skip_at {
                            skip_depth = None;
                        }
                        depth = depth.saturating_sub(1);
                    }
                    _ => {}
                }
                i += 1;
                continue;
            }

            // Check if previous daemon-owned command is followed by a Push.
            if let Some(mode) = skip_next_children.take() {
                if matches!(cmd, byo::Command::Push { .. }) {
                    match mode {
                        SkipMode::Skip => {
                            // Pure skip: no slot collection, just skip the block
                            depth += 1;
                            skip_depth = Some(depth);
                            i += 1;
                            continue;
                        }
                        SkipMode::CollectSlots {
                            expansion_cmds,
                            app_client,
                        } => {
                            // Collect slot contents from the children block
                            let (slot_map, end_idx) = collect_slot_contents(commands, i);
                            // Pre-qualify the slot contents with the app client
                            let qualified_slots: HashMap<String, Vec<byo::Command>> = slot_map
                                .into_iter()
                                .map(|(k, v)| (k, qualify_commands(&v, &app_client)))
                                .collect();
                            // Rewrite expansion with qualified slot contents
                            self.rewrite_commands(
                                &expansion_cmds,
                                "",
                                claims,
                                buf,
                                claimed_destroys,
                                if qualified_slots.is_empty() {
                                    None
                                } else {
                                    Some(&qualified_slots)
                                },
                            );
                            i = end_idx;
                            continue;
                        }
                    }
                }
                // Not a Push — fall through to normal processing.
                // For CollectSlots mode, emit the expansion without slot contents.
                if let SkipMode::CollectSlots { expansion_cmds, .. } = mode {
                    self.rewrite_commands(&expansion_cmds, "", claims, buf, claimed_destroys, None);
                }
            }

            match cmd {
                byo::Command::Upsert { kind, id, props } => {
                    qualify_into(&mut qid_buf, client, id);

                    if *id != "_" && self.expansions.contains_key(&*qid_buf) {
                        // Splice in the daemon expansion.
                        let (_, expansion_cmds, _) = &self.expansions[&*qid_buf];
                        // Defer rewriting until we know if there are children (for slots).
                        skip_next_children = Some(SkipMode::CollectSlots {
                            expansion_cmds: expansion_cmds.clone(),
                            app_client: client.to_string(),
                        });
                    } else if claims.contains_key(&**kind) && *id != "_" {
                        // Daemon-owned but no expansion (shouldn't happen
                        // if pending == 0, but handle gracefully).
                        skip_next_children = Some(SkipMode::Skip);
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
                byo::Command::Push { slot } => {
                    // In expansion scope, a slotted push triggers slot substitution.
                    // Slot pushes are orchestrator-internal — consumed here, never
                    // forwarded to the compositor.
                    if let Some(name) = slot {
                        let slot_key = name.to_string();
                        let has_content = slot_contents
                            .and_then(|m| m.get(&slot_key))
                            .is_some_and(|c| !c.is_empty());

                        // Collect the fallback content range (between slot Push
                        // and its matching Pop).
                        let fallback_start = i + 1;
                        let mut scan = fallback_start;
                        let mut scan_depth: usize = 1;
                        while scan < commands.len() && scan_depth > 0 {
                            match &commands[scan] {
                                byo::Command::Push { .. } => scan_depth += 1,
                                byo::Command::Pop => scan_depth -= 1,
                                _ => {}
                            }
                            scan += 1;
                        }
                        // scan now points past the matching Pop
                        let fallback_end = scan - 1; // the matching Pop

                        if has_content {
                            let content = &slot_contents.unwrap()[&slot_key];
                            // Emit substituted content inline (no wrapping braces)
                            self.rewrite_commands(content, "", claims, buf, claimed_destroys, None);
                        } else {
                            // No slot content — emit fallback content inline
                            let fallback = &commands[fallback_start..fallback_end];
                            if !fallback.is_empty() {
                                self.rewrite_commands(
                                    fallback,
                                    client,
                                    claims,
                                    buf,
                                    claimed_destroys,
                                    None,
                                );
                            }
                        }
                        // Skip past the slot Push, its content, and matching Pop
                        i = scan;
                        continue;
                    }
                    // Regular (non-slotted) push
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
                        skip_next_children = Some(SkipMode::Skip);
                    } else {
                        qualify_into(&mut qid_buf, client, id);
                        write_patch(buf, kind, &qid_buf, props);
                    }
                }
                byo::Command::Pragma {
                    kind: byo::PragmaKind::Redirect | byo::PragmaKind::Unredirect,
                    ..
                } => {
                    // Redirect/unredirect are consumed by the orchestrator —
                    // it injects its own redirect frames before passthrough.
                }
                byo::Command::Event { .. }
                | byo::Command::Ack { .. }
                | byo::Command::Request { .. }
                | byo::Command::Response { .. }
                | byo::Command::Pragma { .. } => {
                    // Events/requests/responses/pragmas in a batch are handled
                    // separately by the router.
                    let mut em = byo::emitter::Emitter::new(&mut *buf);
                    let _ = em.commands(std::slice::from_ref(cmd));
                }
            }
            i += 1;
        }

        // If the last command was an expanded type with no following Push, emit now
        if let Some(SkipMode::CollectSlots { expansion_cmds, .. }) = skip_next_children {
            self.rewrite_commands(&expansion_cmds, "", claims, buf, claimed_destroys, None);
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

/// Write props in wire format. Delegates to [`EmitProps::emit_props`].
pub(crate) fn write_props(buf: &mut Vec<u8>, props: &(impl byo::emitter::EmitProps + ?Sized)) {
    props.emit_props(&mut *buf).unwrap();
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
            byo::Command::Push { slot } => match slot {
                Some(name) => {
                    buf.extend_from_slice(b" {");
                    buf.extend_from_slice(name.as_bytes());
                }
                None => buf.extend_from_slice(b" {"),
            },
            byo::Command::Pop => {
                buf.extend_from_slice(b"\n}");
            }
            byo::Command::Patch { kind, id, props } => {
                qualify_into(&mut qid_buf, client, id);
                write_patch(&mut buf, kind, &qid_buf, props);
            }
            byo::Command::Pragma {
                kind: byo::PragmaKind::Redirect | byo::PragmaKind::Unredirect,
                ..
            } => {
                // Consumed by the orchestrator — not forwarded.
            }
            byo::Command::Event { .. }
            | byo::Command::Ack { .. }
            | byo::Command::Request { .. }
            | byo::Command::Response { .. }
            | byo::Command::Pragma { .. } => {
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

/// Collect slot contents from the children block of a daemon-owned object.
///
/// Starting from `commands[start_idx]` which must be `Push { slot: None }` (the
/// outer children block), partitions children by slot name:
/// - `Push { slot: Some(name) }` blocks → bucket `name`
/// - Bare commands (not inside a named slot) → bucket `"_"` (default)
///
/// Returns (slot_map, end_idx) where end_idx is the index after the matching Pop.
fn collect_slot_contents(
    commands: &[byo::Command],
    start_idx: usize,
) -> (HashMap<String, Vec<byo::Command>>, usize) {
    use byo::Command;

    let mut slots: HashMap<String, Vec<Command>> = HashMap::new();
    let mut default_cmds: Vec<Command> = Vec::new();
    let mut i = start_idx + 1; // skip the outer Push
    let outer_depth: usize = 1; // we're inside the outer Push

    while i < commands.len() && outer_depth > 0 {
        match &commands[i] {
            Command::Pop if outer_depth == 1 => {
                // Matching Pop for the outer block
                i += 1;
                break;
            }
            Command::Push { slot: Some(name) } if outer_depth == 1 => {
                // Named slot block at the top level of children
                let slot_name = name.to_string();
                let mut slot_cmds = Vec::new();
                let mut slot_depth: usize = 1;
                i += 1; // skip the slot Push

                while i < commands.len() && slot_depth > 0 {
                    match &commands[i] {
                        Command::Pop if slot_depth == 1 => {
                            slot_depth = 0;
                        }
                        Command::Push { .. } => {
                            slot_depth += 1;
                            slot_cmds.push(commands[i].clone());
                        }
                        Command::Pop => {
                            slot_depth -= 1;
                            slot_cmds.push(commands[i].clone());
                        }
                        _ => {
                            slot_cmds.push(commands[i].clone());
                        }
                    }
                    i += 1;
                }

                if let Some(existing) = slots.get(&slot_name)
                    && !existing.is_empty()
                {
                    tracing::warn!("duplicate slot targeting '{slot_name}' from app — ignoring");
                    // Return empty slots to trigger fallback behavior
                    return (HashMap::new(), i);
                }
                slots.insert(slot_name, slot_cmds);
            }
            Command::Push { .. } => {
                // Non-slotted push inside the children block — part of default content
                let mut push_depth: usize = 1;
                default_cmds.push(commands[i].clone());
                i += 1;
                while i < commands.len() && push_depth > 0 {
                    match &commands[i] {
                        Command::Push { .. } => push_depth += 1,
                        Command::Pop => push_depth -= 1,
                        _ => {}
                    }
                    default_cmds.push(commands[i].clone());
                    i += 1;
                }
                continue; // already advanced i
            }
            _ => {
                // Bare command at top level — goes to default slot
                default_cmds.push(commands[i].clone());
                i += 1;
                continue;
            }
        }
    }

    if !default_cmds.is_empty() {
        if let Some(existing) = slots.get("_")
            && !existing.is_empty()
        {
            tracing::warn!("duplicate default slot targeting from app — ignoring");
            return (HashMap::new(), i);
        }
        slots.insert("_".to_string(), default_cmds);
    }

    (slots, i)
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

    // -- Slot tests -----------------------------------------------------------

    #[test]
    fn collect_slot_contents_basic() {
        // +dialog d { {header +text t} {footer +view f} }
        let commands = cmds("+dialog d { {header +text t} {footer +view f} }");
        // Push{None} is at index 1
        let (slots, end_idx) = collect_slot_contents(&commands, 1);
        assert_eq!(slots.len(), 2);
        assert!(slots.contains_key("header"));
        assert!(slots.contains_key("footer"));
        // Header should contain: Upsert(text, t)
        assert_eq!(slots["header"].len(), 1);
        assert!(matches!(&slots["header"][0], byo::Command::Upsert { id, .. } if id == "t"));
        // Footer should contain: Upsert(view, f)
        assert_eq!(slots["footer"].len(), 1);
        assert!(matches!(&slots["footer"][0], byo::Command::Upsert { id, .. } if id == "f"));
        // end_idx should point past the outer Pop
        assert_eq!(end_idx, commands.len());
    }

    #[test]
    fn collect_slot_contents_default() {
        // +dialog d { +text bare1 +text bare2 }
        let commands = cmds("+dialog d { +text bare1 +text bare2 }");
        let (slots, _) = collect_slot_contents(&commands, 1);
        assert_eq!(slots.len(), 1);
        assert!(slots.contains_key("_"));
        assert_eq!(slots["_"].len(), 2);
    }

    #[test]
    fn collect_slot_contents_mixed() {
        // +dialog d { {header +text h} +view bare }
        let commands = cmds("+dialog d { {header +text h} +view bare }");
        let (slots, _) = collect_slot_contents(&commands, 1);
        assert_eq!(slots.len(), 2);
        assert!(slots.contains_key("header"));
        assert!(slots.contains_key("_"));
        assert_eq!(slots["header"].len(), 1);
        assert_eq!(slots["_"].len(), 1);
    }

    #[test]
    fn rewrite_with_default_slot() {
        // App: +dialog d { +button ok label=OK }
        // Dialog daemon expansion has a default slot: +view controls:d-root { {_ +text controls:fallback} }
        let mut batch = PendingBatch::new(
            pid(1),
            "app".into(),
            cmds("+dialog d { +button ok label=OK }"),
        );

        let mut claims = HashMap::new();
        claims.insert("dialog".to_string(), pid(2));
        claims.insert("button".to_string(), pid(3));

        // Dialog expansion with default slot
        batch.expansions.insert(
            "app:d".to_string(),
            (
                pid(2),
                cmds("+view controls:d-root { {_ +text controls:fallback} }"),
                false,
            ),
        );

        // Button expansion (nested inside slot content)
        batch.expansions.insert(
            "app:ok".to_string(),
            (pid(3), cmds("+view buttons:ok-root class=btn"), false),
        );

        let (result, _) = batch.rewrite(&claims);
        // The default slot should receive the button (which itself expands)
        assert_eq_bytes(
            &result,
            "+view controls:d-root { +view buttons:ok-root class=btn }",
        );
    }

    #[test]
    fn rewrite_with_named_slots() {
        // App: +dialog d { {header +text title content=Hi} {footer +view actions} }
        // Dialog expansion: +view controls:d-root { {header +text controls:hdr} {footer} }
        let mut batch = PendingBatch::new(
            pid(1),
            "app".into(),
            cmds("+dialog d { {header +text title content=Hi} {footer +view actions} }"),
        );

        let mut claims = HashMap::new();
        claims.insert("dialog".to_string(), pid(2));

        batch.expansions.insert(
            "app:d".to_string(),
            (
                pid(2),
                cmds("+view controls:d-root { {header +text controls:hdr} {footer} }"),
                false,
            ),
        );

        let (result, _) = batch.rewrite(&claims);
        // Header slot gets app's +text title, footer gets app's +view actions
        // Fallback content (+text controls:hdr) is skipped because app provided header
        assert_eq_bytes(
            &result,
            "+view controls:d-root { +text app:title content=Hi +view app:actions }",
        );
    }

    #[test]
    fn rewrite_with_slot_fallback() {
        // App: +dialog d (no children — no slot content)
        // Dialog expansion with fallback: +view controls:d-root { {_ +text controls:empty} }
        let mut batch = PendingBatch::new(pid(1), "app".into(), cmds("+dialog d"));

        let mut claims = HashMap::new();
        claims.insert("dialog".to_string(), pid(2));

        batch.expansions.insert(
            "app:d".to_string(),
            (
                pid(2),
                cmds("+view controls:d-root { {_ +text controls:empty} }"),
                false,
            ),
        );

        let (result, _) = batch.rewrite(&claims);
        // No slot content → fallback content used
        assert_eq_bytes(&result, "+view controls:d-root { +text controls:empty }");
    }

    #[test]
    fn rewrite_with_no_fallback_no_content() {
        // App: +dialog d (no children)
        // Dialog expansion: +view controls:d-root { {_} }
        let mut batch = PendingBatch::new(pid(1), "app".into(), cmds("+dialog d"));

        let mut claims = HashMap::new();
        claims.insert("dialog".to_string(), pid(2));

        batch.expansions.insert(
            "app:d".to_string(),
            (pid(2), cmds("+view controls:d-root { {_} }"), false),
        );

        let (result, _) = batch.rewrite(&claims);
        // Empty slot produces nothing — just the empty push/pop
        assert_eq_bytes(&result, "+view controls:d-root { }");
    }
}
