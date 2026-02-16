//! Central message router.
//!
//! All messages from all processes flow through the router. It parses BYO
//! payloads ephemerally to make routing decisions, manages subscriptions,
//! triggers daemon expansion, and maintains per-process output ordering.

use std::collections::HashMap;
use std::io::Write;

use indexmap::IndexMap;

use byo::parser::parse;
use byo::protocol::{Command, Prop, RequestKind, ResponseKind};

use crate::batch::{OutputQueue, PendingBatch, write_props};
use crate::id::QualifiedId;
use crate::process::{Process, ProcessId, WriteMsg};
use crate::state::{ObjectTree, PropValue};

/// Messages flowing from process reader tasks to the router.
#[derive(Debug)]
pub enum RouterMsg {
    /// A complete BYO payload from a process.
    Byo { from: ProcessId, raw: Vec<u8> },
    /// A complete graphics payload from a process.
    Graphics { from: ProcessId, raw: Vec<u8> },
    /// Passthrough bytes from a process.
    Passthrough { from: ProcessId, raw: Vec<u8> },
    /// A process has disconnected.
    Disconnected { process: ProcessId },
}

/// What to do with the batch after the parse block finishes.
enum BatchAction {
    /// No daemon types — forward with qualified IDs.
    Forward,
    /// Has daemon types — start expansion.
    Expand {
        /// (subscriber, qid, seq, expand_buf, is_re_expand) for each daemon-owned command.
        expand_msgs: Vec<(ProcessId, QualifiedId, u64, Vec<u8>, bool)>,
    },
}

/// The central router that dispatches messages between processes.
pub struct Router {
    /// All managed processes, keyed by ID.
    processes: HashMap<ProcessId, Process>,
    /// Type name → ONE claiming daemon (expansion ownership).
    claims: HashMap<String, ProcessId>,
    /// Type name → MULTIPLE observer processes (final output consumers).
    observers: HashMap<String, Vec<ProcessId>>,
    /// The reduced object tree for crash recovery.
    state: ObjectTree,
    /// Per-process output ordering queues.
    output_queues: HashMap<ProcessId, OutputQueue>,
    /// Per-subscriber expand sequence counters.
    expand_seqs: HashMap<ProcessId, u64>,
    /// In-flight pending batches (by batch index).
    pending_batches: Vec<PendingBatch>,
    /// Process name → ProcessId for lookup.
    name_to_id: HashMap<String, ProcessId>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            claims: HashMap::new(),
            observers: HashMap::new(),
            state: ObjectTree::new(),
            output_queues: HashMap::new(),
            expand_seqs: HashMap::new(),
            pending_batches: Vec::new(),
            name_to_id: HashMap::new(),
        }
    }

    /// Register a process with the router.
    pub fn add_process(&mut self, process: Process) {
        let id = process.id;
        let name = process.name.clone();
        self.output_queues.insert(id, OutputQueue::new());
        self.name_to_id.insert(name, id);
        self.processes.insert(id, process);
    }

    /// Returns true if the router has any connected processes.
    pub fn has_processes(&self) -> bool {
        !self.processes.is_empty()
    }

    /// Get the client name for a process ID.
    fn client_name(&self, id: ProcessId) -> Option<&str> {
        self.processes.get(&id).map(|p| p.name.as_str())
    }

    /// Handle an incoming router message.
    pub async fn handle(&mut self, msg: RouterMsg) {
        match msg {
            RouterMsg::Byo { from, raw } => self.handle_byo(from, raw).await,
            RouterMsg::Graphics { from, raw } => self.handle_graphics(from, raw).await,
            RouterMsg::Passthrough { from, raw } => self.handle_passthrough(from, raw).await,
            RouterMsg::Disconnected { process } => self.handle_disconnect(process).await,
        }
    }

    /// Handle a BYO payload from a process.
    ///
    /// Parses the payload within a block scope so borrows on `raw` end
    /// naturally before `raw` is moved into a `PendingBatch`.
    async fn handle_byo(&mut self, from: ProcessId, raw: Vec<u8>) {
        let client = match self.client_name(from) {
            Some(n) => n.to_owned(),
            None => return,
        };

        // Check if output queue is blocked — buffer if so.
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            queue.push(RouterMsg::Byo { from, raw });
            return;
        }

        // Parse within a block scope. All borrows of `raw` end at the
        // closing brace, so `raw` can be moved afterward.
        let action = {
            let payload_str = match std::str::from_utf8(&raw) {
                Ok(s) => s,
                Err(_) => return,
            };

            // TODO: move parsing to per-process reader tasks so the router
            // only receives pre-parsed commands and doesn't pay the parse cost
            // on the single-threaded hot path.
            let commands = match parse(payload_str) {
                Ok(cmds) => cmds,
                Err(e) => {
                    tracing::warn!("parse error from {client}: {e:?}");
                    return;
                }
            };

            // Process requests/ack/responses directly while commands are borrowed.
            for cmd in &commands {
                match cmd {
                    Command::Request {
                        kind: RequestKind::Claim,
                        seq,
                        target,
                        ..
                    } => {
                        self.handle_claim(from, &client, *seq, target).await;
                    }
                    Command::Request {
                        kind: RequestKind::Unclaim,
                        seq,
                        target,
                        ..
                    } => {
                        self.handle_unclaim(from, &client, *seq, target).await;
                    }
                    Command::Request {
                        kind: RequestKind::Observe,
                        seq,
                        target,
                        ..
                    } => {
                        self.handle_observe(from, &client, *seq, target);
                    }
                    Command::Request {
                        kind: RequestKind::Unobserve,
                        seq,
                        target,
                        ..
                    } => {
                        self.handle_unobserve(from, &client, *seq, target);
                    }
                    Command::Response {
                        kind: ResponseKind::Expand,
                        seq,
                        body,
                        ..
                    } => {
                        self.handle_expand_response(from, &client, *seq, body)
                            .await;
                    }
                    Command::Ack { seq, .. } => {
                        // Other ACKs (click, keydown, etc.) — forwarded to event source.
                        // For now, liveness tracking is not implemented.
                        let _ = (from, &client, *seq);
                    }
                    _ => {}
                }
            }

            // Update state tree directly from borrowed commands.
            self.update_state(&commands, from, &client);

            // Determine routing: check for daemon-claimed types.
            let mut expand_msgs = Vec::new();

            for cmd in &commands {
                match cmd {
                    Command::Upsert { kind, id, props }
                        if *id != "_"
                            && self.claims.get(*kind).is_some_and(|&s| s != from) =>
                    {
                        let subscriber = self.claims[*kind];
                        let qid = QualifiedId::new(&client, id);
                        let expand_seq = self.next_expand_seq(subscriber);

                        let mut expand_buf = Vec::new();
                        let _ =
                            write!(expand_buf, "\n?expand {expand_seq} {qid} kind={kind}");
                        write_props(&mut expand_buf, props);

                        expand_msgs.push((
                            subscriber,
                            qid,
                            expand_seq,
                            expand_buf,
                            false,
                        ));
                    }
                    Command::Patch { kind, id, .. }
                        if *id != "_"
                            && self.claims.get(*kind).is_some_and(|&s| s != from) =>
                    {
                        // Patch on a claimed type: re-expand with full reduced state.
                        let subscriber = self.claims[*kind];
                        let qid = QualifiedId::new(&client, id);

                        if let Some(reduced) = self.state.reduced_command(&qid) {
                            let expand_seq = self.next_expand_seq(subscriber);
                            let mut expand_buf = Vec::new();
                            let _ = write!(
                                expand_buf,
                                "\n?expand {expand_seq} {}",
                                String::from_utf8_lossy(&reduced)
                                    .trim_start_matches('+')
                            );
                            expand_msgs.push((
                                subscriber,
                                qid,
                                expand_seq,
                                expand_buf,
                                true, // re-expand
                            ));
                        }
                    }
                    _ => {}
                }
            }

            if expand_msgs.is_empty() {
                BatchAction::Forward
            } else {
                BatchAction::Expand { expand_msgs }
            }
        };
        // --- block scope ends, `raw` is no longer borrowed ---

        match action {
            BatchAction::Forward => {
                let batch = PendingBatch::new(from, client, raw);
                let (mut rewritten, claimed_destroys) = batch.rewrite(&self.claims);
                self.handle_cascade_destroys(&claimed_destroys, &mut rewritten)
                    .await;
                self.forward_to_observers(&rewritten, from).await;
            }
            BatchAction::Expand { expand_msgs } => {
                let mut batch = PendingBatch::new(from, client, raw);

                for (subscriber, qid, expand_seq, expand_buf, is_re_expand) in expand_msgs {
                    if is_re_expand {
                        batch.add_pending_re_expand(subscriber, expand_seq, qid);
                    } else {
                        batch.add_pending_expand(subscriber, expand_seq, qid, 0);
                    }
                    self.send_to(subscriber, WriteMsg::Byo(expand_buf)).await;
                }

                if let Some(queue) = self.output_queues.get_mut(&from) {
                    queue.block();
                }
                self.pending_batches.push(batch);
            }
        }
    }

    /// Update the state tree from parsed commands.
    fn update_state(&mut self, commands: &[Command<'_>], owner: ProcessId, client: &str) {
        let mut parent_stack: Vec<QualifiedId> = Vec::new();
        let mut last_qid: Option<QualifiedId> = None;

        for cmd in commands {
            match cmd {
                Command::Upsert { kind, id, props } => {
                    if *id == "_" {
                        continue;
                    }
                    let qid = QualifiedId::new(client, id);
                    let map = props_to_map(props);
                    self.state.upsert(kind, &qid, &map, owner);
                    if let Some(parent) = parent_stack.last() {
                        self.state.set_parent(&qid, parent);
                    }
                    last_qid = Some(qid);
                }
                Command::Destroy { id, .. } => {
                    let qid = QualifiedId::new(client, id);
                    self.state.destroy(&qid);
                }
                Command::Push => {
                    if let Some(ref qid) = last_qid {
                        parent_stack.push(qid.clone());
                    }
                }
                Command::Pop => {
                    parent_stack.pop();
                    last_qid = parent_stack.last().cloned();
                }
                Command::Patch { id, props, .. } => {
                    let qid = QualifiedId::new(client, id);
                    let (set, remove) = props_to_patch(props);
                    let remove_refs: Vec<&str> = remove.iter().map(|s| s.as_str()).collect();
                    self.state.patch(&qid, &set, &remove_refs);
                    last_qid = Some(qid);
                }
                _ => {}
            }
        }
    }

    /// Handle a graphics payload — forward to all subscribers of graphics-related types.
    /// In practice, graphics commands go to the compositor.
    async fn handle_graphics(&mut self, from: ProcessId, raw: Vec<u8>) {
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            queue.push(RouterMsg::Graphics { from, raw });
            return;
        }

        // Forward graphics to all observers of "view" (typically the compositor).
        if let Some(observers) = self.observers.get("view") {
            for &pid in observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Graphics(raw.clone())).await;
                }
            }
        }
    }

    /// Handle passthrough bytes — forward to view observers (compositor).
    async fn handle_passthrough(&mut self, from: ProcessId, raw: Vec<u8>) {
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            queue.push(RouterMsg::Passthrough { from, raw });
            return;
        }

        if let Some(observers) = self.observers.get("view") {
            for &pid in observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Passthrough(raw.clone())).await;
                }
            }
        }
    }

    /// Handle a process disconnecting.
    async fn handle_disconnect(&mut self, process: ProcessId) {
        let name = match self.client_name(process) {
            Some(n) => n.to_owned(),
            None => return,
        };
        tracing::info!("process disconnected: {name}");

        // Remove all claims for this process.
        self.claims.retain(|_, &mut pid| pid != process);

        // Remove all observer entries for this process.
        for observers in self.observers.values_mut() {
            observers.retain(|&pid| pid != process);
        }
        self.observers.retain(|_, v| !v.is_empty());

        // Remove expand seq counter.
        self.expand_seqs.remove(&process);

        // Remove objects owned by this process from the state tree.
        let _removed = self.state.remove_by_owner(process);

        // Clean up process state.
        self.output_queues.remove(&process);

        // Remove pending batches from this process.
        self.pending_batches.retain(|b| b.from != process);

        // Remove from name_to_id.
        self.name_to_id.retain(|_, &mut pid| pid != process);
        self.processes.remove(&process);
    }

    /// Handle a `?claim` request — claim ownership of a type (fire-and-forget).
    async fn handle_claim(&mut self, from: ProcessId, client: &str, _seq: u64, target_type: &str) {
        tracing::info!("{client} claims type '{target_type}'");
        self.claims.insert(target_type.to_owned(), from);

        // Replay reduced state for all objects of this type.
        let objects: Vec<_> = self
            .state
            .objects_of_type(target_type)
            .into_iter()
            .map(|o| (o.qid.clone(), o.kind.clone()))
            .collect();

        for (qid, _kind) in &objects {
            if let Some(reduced) = self.state.reduced_command(qid) {
                let expand_seq = self.next_expand_seq(from);
                let mut expand_buf = Vec::new();
                let _ = write!(
                    expand_buf,
                    "\n?expand {expand_seq} {}",
                    String::from_utf8_lossy(&reduced).trim_start_matches('+')
                );
                self.send_to(from, WriteMsg::Byo(expand_buf)).await;
            }
        }
    }

    /// Handle a `?unclaim` request — release claim on a type (fire-and-forget).
    async fn handle_unclaim(
        &mut self,
        from: ProcessId,
        client: &str,
        _seq: u64,
        target_type: &str,
    ) {
        tracing::info!("{client} unclaims type '{target_type}'");
        if self.claims.get(target_type) == Some(&from) {
            self.claims.remove(target_type);
        }
    }

    /// Handle a `?observe` request — observe final output for a type (fire-and-forget).
    fn handle_observe(&mut self, from: ProcessId, client: &str, _seq: u64, target_type: &str) {
        tracing::info!("{client} observes type '{target_type}'");
        let observers = self.observers.entry(target_type.to_owned()).or_default();
        if !observers.contains(&from) {
            observers.push(from);
        }
    }

    /// Handle a `?unobserve` request — stop observing a type (fire-and-forget).
    fn handle_unobserve(&mut self, from: ProcessId, client: &str, _seq: u64, target_type: &str) {
        tracing::info!("{client} unobserves type '{target_type}'");
        if let Some(observers) = self.observers.get_mut(target_type) {
            observers.retain(|&pid| pid != from);
            if observers.is_empty() {
                self.observers.remove(target_type);
            }
        }
    }

    /// Handle a `.expand` response from a daemon.
    async fn handle_expand_response(
        &mut self,
        from: ProcessId,
        client: &str,
        seq: u64,
        body: &Option<Vec<Command<'_>>>,
    ) {
        // Look up the current depth of this expansion before recording.
        let current_depth = self
            .pending_batches
            .iter()
            .flat_map(|b| b.pending_expands.iter())
            .find(|e| e.subscriber == from && e.seq == seq)
            .map(|e| e.depth)
            .unwrap_or(0);

        // Record the expansion body bytes if present, with IDs qualified
        // under the daemon's client name.
        if let Some(body_cmds) = body {
            let expansion_buf = crate::batch::qualify_and_serialize(body_cmds, client);

            // Scan for nested daemon-owned types within the expansion.
            if current_depth < crate::batch::MAX_EXPANSION_DEPTH {
                self.trigger_nested_expansions(from, seq, body_cmds, client, current_depth)
                    .await;
            } else {
                tracing::warn!(
                    "expansion depth limit reached ({}) for {client}",
                    crate::batch::MAX_EXPANSION_DEPTH
                );
            }

            self.record_expansion_output(from, seq, expansion_buf);
        }
        self.complete_expansion(from, seq).await;
    }

    /// Scan expansion body for nested daemon-owned types and trigger
    /// additional `?expand` requests.
    async fn trigger_nested_expansions(
        &mut self,
        from: ProcessId,
        seq: u64,
        body_cmds: &[Command<'_>],
        daemon_client: &str,
        current_depth: u32,
    ) {
        use crate::batch::write_props;
        use crate::id::qualify;

        let mut nested_expands = Vec::new();

        for cmd in body_cmds {
            if let Command::Upsert { kind, id, props } = cmd
                && *id != "_"
                && let Some(&subscriber) = self.claims.get(*kind)
            {
                let qid = QualifiedId::new(daemon_client, id);
                let expand_seq = self.next_expand_seq(subscriber);

                let mut expand_buf = Vec::new();
                let _ = write!(
                    expand_buf,
                    "\n?expand {expand_seq} {} kind={kind}",
                    qualify(daemon_client, id)
                );
                write_props(&mut expand_buf, props);

                nested_expands.push((subscriber, qid, expand_seq, expand_buf));
            }
        }

        if !nested_expands.is_empty() {
            // Find the batch that owns this expansion.
            let batch_idx = self
                .pending_batches
                .iter()
                .position(|b| b.has_pending_expand(from, seq));

            if let Some(idx) = batch_idx {
                for (subscriber, qid, expand_seq, expand_buf) in nested_expands {
                    self.pending_batches[idx].add_pending_expand(
                        subscriber,
                        expand_seq,
                        qid,
                        current_depth + 1,
                    );
                    self.send_to(subscriber, WriteMsg::Byo(expand_buf)).await;
                }
            }
        }
    }

    /// Update the state tree with expansion nodes from a completed batch.
    ///
    /// For each expansion, parse the bytes back to commands and walk them
    /// to add nodes to the state tree with `expanded_from` and the correct owner.
    fn update_expansion_state(&mut self, batch: &PendingBatch) {
        for (source_qid_str, (daemon_pid, expansion_bytes)) in &batch.expansions {
            let Some(source_qid) = QualifiedId::parse(source_qid_str) else {
                continue;
            };

            let exp_str = match std::str::from_utf8(expansion_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let commands = match byo::parser::parse(exp_str) {
                Ok(cmds) => cmds,
                Err(_) => continue,
            };

            // Find the source node's parent to use as parent for root-level
            // expansion nodes.
            let source_parent = self
                .state
                .get(&source_qid)
                .and_then(|o| o.parent.clone());

            let mut parent_stack: Vec<QualifiedId> = Vec::new();
            let mut last_qid: Option<QualifiedId> = None;

            for cmd in &commands {
                match cmd {
                    Command::Upsert { kind, id, props } => {
                        if *id == "_" {
                            continue;
                        }
                        // IDs are already qualified (from qualify_and_serialize).
                        let qid = if id.contains(':') {
                            QualifiedId::parse(id).unwrap_or_else(|| QualifiedId::new("", id))
                        } else {
                            QualifiedId::new("", id)
                        };
                        let map = props_to_map(props);
                        self.state.upsert(kind, &qid, &map, *daemon_pid);

                        // Set expanded_from.
                        self.state.set_expanded_from(&qid, &source_qid);

                        // Set parent: root-level → source's parent, nested → push parent.
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&qid, parent);
                        } else if let Some(ref parent) = source_parent {
                            self.state.set_parent(&qid, parent);
                        }

                        last_qid = Some(qid);
                    }
                    Command::Push => {
                        if let Some(ref qid) = last_qid {
                            parent_stack.push(qid.clone());
                        }
                    }
                    Command::Pop => {
                        parent_stack.pop();
                        last_qid = parent_stack.last().cloned();
                    }
                    Command::Patch { id, props, .. } => {
                        if let Some(qid) = QualifiedId::parse(id) {
                            let (set, remove) = props_to_patch(props);
                            let remove_refs: Vec<&str> =
                                remove.iter().map(|s| s.as_str()).collect();
                            self.state.patch(&qid, &set, &remove_refs);
                        }
                        last_qid = QualifiedId::parse(id);
                    }
                    Command::Destroy { id, .. } => {
                        if let Some(qid) = QualifiedId::parse(id) {
                            self.state.destroy(&qid);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Reconcile a re-expansion: diff old expansion nodes against new expansion
    /// and produce minimal delta commands (patches, creates, destroys).
    ///
    /// Returns the delta bytes to send to observers.
    fn reconcile_expansion(
        &mut self,
        source_qid: &QualifiedId,
        daemon_pid: ProcessId,
        new_expansion_bytes: &[u8],
    ) -> Vec<u8> {
        use std::io::Write;

        // Collect old expansion nodes.
        let old_nodes: HashMap<QualifiedId, IndexMap<String, PropValue>> = self
            .state
            .expansions_of(source_qid)
            .iter()
            .map(|o| (o.qid.clone(), o.props.clone()))
            .collect();

        let old_kinds: HashMap<QualifiedId, String> = self
            .state
            .expansions_of(source_qid)
            .iter()
            .map(|o| (o.qid.clone(), o.kind.clone()))
            .collect();

        // Parse new expansion.
        let exp_str = match std::str::from_utf8(new_expansion_bytes) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let new_cmds = match byo::parser::parse(exp_str) {
            Ok(cmds) => cmds,
            Err(_) => return Vec::new(),
        };

        let mut delta = Vec::new();
        let mut seen_qids: std::collections::HashSet<QualifiedId> = std::collections::HashSet::new();
        let mut parent_stack: Vec<QualifiedId> = Vec::new();
        let mut last_qid: Option<QualifiedId> = None;

        for cmd in &new_cmds {
            match cmd {
                Command::Upsert { kind, id, props } => {
                    if *id == "_" {
                        continue;
                    }
                    let qid = if id.contains(':') {
                        QualifiedId::parse(id).unwrap_or_else(|| QualifiedId::new("", id))
                    } else {
                        QualifiedId::new("", id)
                    };

                    seen_qids.insert(qid.clone());
                    let new_props = props_to_map(props);

                    if let Some(old_props) = old_nodes.get(&qid) {
                        // Existing node — compute delta.
                        let mut delta_props: Vec<u8> = Vec::new();
                        let mut has_changes = false;

                        // Check for changed/added props.
                        for (key, new_val) in &new_props {
                            if old_props.get(key) != Some(new_val) {
                                has_changes = true;
                                match new_val {
                                    PropValue::Str(s) => {
                                        let _ = write!(delta_props, " {key}=");
                                        crate::batch::write_value(&mut delta_props, s);
                                    }
                                    PropValue::Flag => {
                                        let _ = write!(delta_props, " {key}");
                                    }
                                }
                            }
                        }

                        // Check for removed props.
                        for key in old_props.keys() {
                            if !new_props.contains_key(key) {
                                has_changes = true;
                                let _ = write!(delta_props, " ~{key}");
                            }
                        }

                        if has_changes {
                            let _ = write!(delta, "\n@{kind} {qid}");
                            delta.extend_from_slice(&delta_props);
                        }
                    } else {
                        // New node — emit create.
                        crate::batch::write_upsert(
                            &mut delta,
                            kind,
                            &qid.to_string(),
                            props,
                        );
                    }

                    // Update state tree.
                    self.state.upsert(kind, &qid, &new_props, daemon_pid);
                    self.state.set_expanded_from(&qid, source_qid);
                    if let Some(parent) = parent_stack.last() {
                        self.state.set_parent(&qid, parent);
                    }

                    last_qid = Some(qid);
                }
                Command::Push => {
                    if let Some(ref qid) = last_qid {
                        parent_stack.push(qid.clone());
                    }
                }
                Command::Pop => {
                    parent_stack.pop();
                    last_qid = parent_stack.last().cloned();
                }
                _ => {}
            }
        }

        // Emit destroys for nodes that are in old but not in new.
        for qid in old_nodes.keys() {
            if !seen_qids.contains(qid) {
                if let Some(kind) = old_kinds.get(qid) {
                    let _ = write!(delta, "\n-{kind} {qid}");
                }
                self.state.destroy(qid);
            }
        }

        delta
    }

    /// Handle cascade destroys for claimed-type objects.
    ///
    /// For each source QID:
    /// 1. Cascade destroy expansion nodes in state tree
    /// 2. Emit `-kind qid` for all destroyed expansion nodes into the output
    /// 3. Notify owning daemons about destroyed nodes
    /// 4. Remove the source node from state
    async fn handle_cascade_destroys(
        &mut self,
        claimed_destroys: &[QualifiedId],
        rewritten: &mut Vec<u8>,
    ) {
        use std::io::Write;

        for source_qid in claimed_destroys {
            // Cascade destroy: removes expansion nodes and returns what was removed.
            let destroyed = self.state.cascade_destroy_expansions(source_qid);

            // Emit destroy commands for all expansion nodes (compositor needs these).
            for (qid, kind, _owner) in &destroyed {
                let _ = write!(rewritten, "\n-{kind} {qid}");
            }

            // Group destroyed nodes by owner daemon and notify.
            let mut daemon_notifications: HashMap<ProcessId, Vec<u8>> = HashMap::new();
            for (qid, kind, owner) in &destroyed {
                let buf = daemon_notifications.entry(*owner).or_default();
                let _ = write!(buf, "\n-{kind} {qid}");
            }

            for (daemon_pid, notification_buf) in daemon_notifications {
                self.send_to(daemon_pid, WriteMsg::Byo(notification_buf))
                    .await;
            }

            // Remove the source node itself from state.
            self.state.destroy(source_qid);
        }
    }

    /// Complete an expansion when a `.expand` response is received from a daemon.
    ///
    /// Matches the response to a specific pending batch by (subscriber, seq).
    /// For re-expansions (patch on claimed type), performs reconciliation
    /// to emit minimal diffs.
    async fn complete_expansion(&mut self, from: ProcessId, seq: u64) {
        let batch_idx = self
            .pending_batches
            .iter()
            .position(|b| b.has_pending_expand(from, seq));

        let Some(idx) = batch_idx else {
            return;
        };

        // Check if this specific expand is a re-expand before completing it.
        let is_re_expand = self.pending_batches[idx]
            .pending_expands
            .iter()
            .find(|e| e.subscriber == from && e.seq == seq)
            .map(|e| e.is_re_expand)
            .unwrap_or(false);

        let batch = &mut self.pending_batches[idx];
        // Complete the specific expand and get the QID back.
        // Expansion bytes are recorded separately via record_expansion_output.
        batch.complete_expand(from, seq);

        if batch.is_ready() {
            let batch = self.pending_batches.remove(idx);

            // Reconcile re-expansions: emit minimal diffs for each.
            let mut reconciliation_buf = Vec::new();
            for (source_qid_str, (daemon_pid, expansion_bytes)) in &batch.expansions {
                if is_re_expand
                    && let Some(source_qid) = QualifiedId::parse(source_qid_str)
                {
                    let delta = self.reconcile_expansion(
                        &source_qid,
                        *daemon_pid,
                        expansion_bytes,
                    );
                    reconciliation_buf.extend_from_slice(&delta);
                }
            }

            let (mut rewritten, claimed_destroys) = batch.rewrite(&self.claims);

            // Update state tree with expansion nodes (for initial expansions).
            self.update_expansion_state(&batch);

            // Handle cascade destroys (appends destroy commands to rewritten).
            self.handle_cascade_destroys(&claimed_destroys, &mut rewritten)
                .await;

            // Append reconciliation output.
            rewritten.extend_from_slice(&reconciliation_buf);

            // Unblock the process output queue and flush.
            let process_id = batch.from;
            self.forward_to_observers(&rewritten, process_id).await;

            if let Some(queue) = self.output_queues.get_mut(&process_id) {
                queue.unblock();
                let queued = queue.drain();
                for msg in queued {
                    Box::pin(self.handle(msg)).await;
                }
            }
        }
    }

    /// Record a daemon's expansion output for a pending batch.
    #[allow(dead_code)]
    pub fn record_expansion_output(&mut self, from: ProcessId, seq: u64, raw: Vec<u8>) {
        let batch = self
            .pending_batches
            .iter_mut()
            .find(|b| b.has_pending_expand(from, seq));

        if let Some(batch) = batch {
            // Find the QID associated with this (from, seq) pair.
            let qid = batch
                .pending_expands
                .iter()
                .find(|e| e.subscriber == from && e.seq == seq)
                .map(|e| e.qid.clone());

            if let Some(ref qid) = qid {
                batch.record_expansion(qid, from, raw);
            }
        }
    }

    /// Forward rewritten payload bytes to all observers.
    async fn forward_to_observers(&self, payload: &[u8], _from: ProcessId) {
        let mut targets: Vec<ProcessId> = Vec::new();
        for observers in self.observers.values() {
            for &pid in observers {
                if !targets.contains(&pid) {
                    targets.push(pid);
                }
            }
        }

        for target in targets {
            self.send_to(target, WriteMsg::Byo(payload.to_vec())).await;
        }
    }

    /// Forward an event to a target process (by object ID namespace).
    #[allow(dead_code)]
    pub async fn forward_event_to_owner(&self, qid_str: &str, event_payload: &[u8]) {
        if let Some(qid) = QualifiedId::parse(qid_str)
            && let Some(&pid) = self.name_to_id.get(qid.client())
        {
            self.send_to(pid, WriteMsg::Byo(event_payload.to_vec()))
                .await;
        }
    }

    /// Send a message to a process.
    async fn send_to(&self, target: ProcessId, msg: WriteMsg) {
        if let Some(process) = self.processes.get(&target) {
            let _ = process.tx.send(msg).await;
        }
    }

    /// Get the next expand sequence number for a subscriber.
    fn next_expand_seq(&mut self, subscriber: ProcessId) -> u64 {
        let seq = self.expand_seqs.entry(subscriber).or_insert(0);
        let current = *seq;
        *seq += 1;
        current
    }
}

/// Convert parsed props to a state-storage IndexMap.
fn props_to_map(props: &[Prop<'_>]) -> IndexMap<String, PropValue> {
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
fn props_to_patch(props: &[Prop<'_>]) -> (IndexMap<String, PropValue>, Vec<String>) {
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
}
