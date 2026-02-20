//! Central message router.
//!
//! All messages from all processes flow through the router. It parses BYO
//! payloads ephemerally to make routing decisions, manages subscriptions,
//! triggers daemon expansion, and maintains per-process output ordering.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexMap;

use byo::emitter::Emitter;
use byo::parser::parse;
use byo::protocol::{Command, EventKind, PragmaKind, Prop, RequestKind, ResponseKind};
use byo::tree::{PropValue, props_to_map, props_to_patch};

use crate::batch::{OutputQueue, PendingBatch, write_props};
use crate::id::QualifiedId;
use crate::process::{Process, ProcessId, WriteMsg};
use crate::state::ObjectTree;

/// Messages flowing from process reader tasks to the router.
#[derive(Debug)]
pub enum RouterMsg {
    /// Pre-parsed BYO commands from a process.
    Byo {
        from: ProcessId,
        commands: Vec<Command>,
    },
    /// A complete graphics payload from a process.
    Graphics { from: ProcessId, raw: Vec<u8> },
    /// Passthrough bytes from a process.
    Passthrough { from: ProcessId, raw: Vec<u8> },
    /// A process has disconnected.
    Disconnected { process: ProcessId },
}

/// Tracks a forwarded event awaiting an ACK from the destination.
struct PendingAck {
    /// The process that originally sent the event.
    sender: ProcessId,
    /// The sequence number used by the original sender.
    sender_seq: u64,
    /// When the event was forwarded (for future liveness timeout).
    #[allow(dead_code)]
    forwarded_at: Instant,
}

/// Tracks a forwarded custom request awaiting a response from the destination.
struct PendingResponse {
    /// The process that originally sent the request.
    sender: ProcessId,
    /// The sequence number used by the original sender.
    sender_seq: u64,
    /// When the request was forwarded (for future liveness timeout).
    #[allow(dead_code)]
    forwarded_at: Instant,
}

/// Per-client kitty graphics ID translation table.
#[derive(Default)]
struct GraphicsIdMap {
    /// Local image ID → global image ID.
    id_map: HashMap<u32, u32>,
    /// Local image number → global image number.
    number_map: HashMap<u32, u32>,
}

/// The central router that dispatches messages between processes.
pub struct Router {
    /// All managed processes, keyed by ID.
    processes: HashMap<ProcessId, Process>,
    /// Type name → ONE claiming daemon (expansion ownership).
    claims: HashMap<String, ProcessId>,
    /// Type name → MULTIPLE observer processes (final output consumers).
    observers: HashMap<String, Vec<ProcessId>>,
    /// Reverse index: observer PID → set of observed type names.
    observer_types: HashMap<ProcessId, HashSet<String>>,
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
    /// Per (destination, event_type): next outbound seq.
    event_seqs: HashMap<(ProcessId, String), u64>,
    /// Per (destination, event_type, forwarded_seq): pending ACK info.
    pending_acks: HashMap<(ProcessId, String, u64), PendingAck>,
    /// Per (destination, request_kind): next outbound seq.
    request_seqs: HashMap<(ProcessId, String), u64>,
    /// Per (destination, request_kind, forwarded_seq): pending response info.
    pending_responses: HashMap<(ProcessId, String, u64), PendingResponse>,
    /// Per-client current passthrough target. Default: "/" (root tty).
    passthrough_targets: HashMap<ProcessId, String>,
    /// The last passthrough target sent to the compositor (for dedup).
    last_forwarded_target: String,
    /// Per-client kitty graphics ID remapping tables.
    graphics_maps: HashMap<ProcessId, GraphicsIdMap>,
    /// Next global image ID to allocate.
    next_global_image_id: u32,
    /// Next global image number to allocate.
    next_global_image_number: u32,
    /// Buffered graphics payloads awaiting a `G` observer.
    /// Cleared once the first `G` observer subscribes.
    pending_graphics: Vec<(ProcessId, Vec<u8>)>,
    /// Buffered passthrough payloads awaiting a `tty` observer.
    /// Cleared once the first `tty` observer subscribes.
    pending_passthrough: Vec<(ProcessId, Vec<u8>)>,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            processes: HashMap::new(),
            claims: HashMap::new(),
            observers: HashMap::new(),
            observer_types: HashMap::new(),
            state: ObjectTree::new(),
            output_queues: HashMap::new(),
            expand_seqs: HashMap::new(),
            pending_batches: Vec::new(),
            name_to_id: HashMap::new(),
            event_seqs: HashMap::new(),
            pending_acks: HashMap::new(),
            request_seqs: HashMap::new(),
            pending_responses: HashMap::new(),
            passthrough_targets: HashMap::new(),
            last_forwarded_target: "/".to_owned(),
            graphics_maps: HashMap::new(),
            next_global_image_id: 1,
            next_global_image_number: 1,
            pending_graphics: Vec::new(),
            pending_passthrough: Vec::new(),
        }
    }
}

impl Router {
    pub fn new() -> Self {
        Self::default()
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
            RouterMsg::Byo { from, commands } => self.handle_byo(from, commands).await,
            RouterMsg::Graphics { from, raw } => self.handle_graphics(from, raw).await,
            RouterMsg::Passthrough { from, raw } => self.handle_passthrough(from, raw).await,
            RouterMsg::Disconnected { process } => self.handle_disconnect(process).await,
        }
    }

    /// Handle pre-parsed BYO commands from a process.
    async fn handle_byo(&mut self, from: ProcessId, mut commands: Vec<Command>) {
        let client = match self.client_name(from) {
            Some(n) => n.to_owned(),
            None => return,
        };

        // Check if output queue is blocked — buffer if so.
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            tracing::warn!(
                "handle_byo: output queue blocked for {client} (pid={}), buffering {} commands",
                from.0,
                commands.len()
            );
            queue.push(RouterMsg::Byo { from, commands });
            return;
        }

        // Remap $img(N) references in prop values to global IDs.
        self.remap_img_vars(from, &mut commands);

        // Process requests/ack/responses.
        let mut resync_pids: Vec<ProcessId> = Vec::new();
        for cmd in &commands {
            match cmd {
                Command::Pragma {
                    kind: PragmaKind::Claim,
                    targets,
                } => {
                    for target in targets {
                        self.handle_claim(from, &client, target).await;
                    }
                }
                Command::Pragma {
                    kind: PragmaKind::Unclaim,
                    targets,
                } => {
                    for target in targets {
                        self.handle_unclaim(from, &client, target).await;
                    }
                }
                Command::Pragma {
                    kind: PragmaKind::Observe,
                    targets,
                } => {
                    for target in targets {
                        self.handle_observe(from, &client, target);
                    }
                    resync_pids.push(from);
                }
                Command::Pragma {
                    kind: PragmaKind::Unobserve,
                    targets,
                } => {
                    for target in targets {
                        self.handle_unobserve(from, &client, target);
                    }
                    resync_pids.push(from);
                }
                Command::Pragma {
                    kind: PragmaKind::Redirect,
                    targets,
                } => {
                    if let Some(target) = targets.first() {
                        if target.as_ref() == "_" {
                            self.passthrough_targets.insert(from, String::new());
                        } else {
                            self.passthrough_targets.insert(from, target.to_string());
                        }
                        tracing::debug!(
                            "{client} redirect passthrough to {:?}",
                            self.passthrough_targets[&from]
                        );
                    }
                }
                Command::Pragma {
                    kind: PragmaKind::Unredirect,
                    ..
                } => {
                    self.passthrough_targets.insert(from, "/".to_owned());
                    tracing::debug!("{client} unredirect passthrough to root tty");
                }
                Command::Response {
                    kind: ResponseKind::Expand,
                    seq,
                    body,
                    ..
                } => {
                    self.handle_expand_response(from, &client, *seq, body).await;
                }
                Command::Request {
                    kind: RequestKind::Other(kind_name),
                    seq,
                    targets,
                    props,
                } => {
                    if let Some(target) = targets.first() {
                        self.handle_request(from, &client, kind_name, *seq, target, props)
                            .await;
                    }
                }
                Command::Response {
                    kind: ResponseKind::Other(kind_name),
                    seq,
                    props,
                    body,
                } => {
                    self.handle_response(from, kind_name, *seq, props, body)
                        .await;
                }
                Command::Event {
                    kind,
                    seq,
                    id,
                    props,
                } => {
                    self.handle_event(from, &client, kind, *seq, id, props)
                        .await;
                }
                Command::Ack { kind, seq, props } => {
                    self.handle_ack(from, kind, *seq, props).await;
                }
                _ => {}
            }
        }

        // Update state tree from commands.
        self.update_state(&commands, from, &client);

        // Determine routing: check for daemon-claimed types.
        let mut expand_msgs = Vec::new();
        for cmd in &commands {
            match cmd {
                Command::Upsert { kind, id, props } if *id != "_" => {
                    if let Some(&subscriber) = self.claims.get(&**kind)
                        && subscriber != from
                    {
                        let qid = QualifiedId::new(&client, id);
                        let expand_seq = self.next_expand_seq(subscriber);

                        let mut expand_buf = Vec::new();
                        let _ = write!(expand_buf, "\n?expand {expand_seq} {qid} kind={kind}");
                        write_props(&mut expand_buf, props);

                        expand_msgs.push((subscriber, qid, expand_seq, expand_buf, false));
                    }
                }
                Command::Patch { kind, id, .. } if *id != "_" => {
                    if let Some(&subscriber) = self.claims.get(&**kind)
                        && subscriber != from
                    {
                        let qid = QualifiedId::new(&client, id);

                        let expand_seq = self.next_expand_seq(subscriber);
                        let mut expand_buf = Vec::new();
                        let _ = write!(expand_buf, "\n?expand {expand_seq} {qid}");
                        if self.state.write_reduced_expand_props(&qid, &mut expand_buf) {
                            expand_msgs.push((subscriber, qid, expand_seq, expand_buf, true));
                        }
                    }
                }
                _ => {}
            }
        }

        if expand_msgs.is_empty() {
            let batch = PendingBatch::new(from, client, commands);
            let (mut rewritten, claimed_destroys) = batch.rewrite(&self.claims);
            self.handle_cascade_destroys(&claimed_destroys, &mut rewritten)
                .await;
            self.forward_to_observers(&rewritten, from).await;
        } else {
            let mut batch = PendingBatch::new(from, client, commands);

            for (subscriber, qid, expand_seq, expand_buf, is_re_expand) in expand_msgs {
                if is_re_expand {
                    batch.add_pending_re_expand(subscriber, expand_seq, qid);
                } else {
                    batch.add_pending_expand(subscriber, expand_seq, qid, 0);
                }
                self.send_to(subscriber, WriteMsg::Byo(Arc::new(expand_buf)));
            }

            if let Some(queue) = self.output_queues.get_mut(&from) {
                queue.block();
            }
            self.pending_batches.push(batch);
        }

        // Resync observers that changed their subscriptions in this batch.
        let resync_set: HashSet<ProcessId> = resync_pids.into_iter().collect();
        for pid in resync_set {
            self.resync_observer(pid).await;
        }

        // Flush pending graphics/passthrough if observers just appeared.
        self.flush_pending_graphics();
        self.flush_pending_passthrough();
    }

    /// Update the state tree from parsed commands.
    ///
    /// Destroys of claimed types are skipped here because their state must
    /// remain intact for `handle_cascade_destroys` to collect descendant
    /// info and emit proper destroy commands. Those nodes are destroyed by
    /// `handle_cascade_destroys` after cascade processing is complete.
    fn update_state(&mut self, commands: &[Command], owner: ProcessId, client: &str) {
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
                Command::Destroy { kind, id } => {
                    // Skip destroys on claimed types — their state is needed
                    // by handle_cascade_destroys for descendant cleanup.
                    if self.claims.contains_key(&**kind) && *id != "_" {
                        continue;
                    }
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

    /// Handle a graphics payload — remap image IDs and forward to `G` observers.
    async fn handle_graphics(&mut self, from: ProcessId, raw: Vec<u8>) {
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            queue.push(RouterMsg::Graphics { from, raw });
            return;
        }

        // Remap i=/I= values to globally unique IDs.
        let remapped = self.remap_graphics_ids(from, &raw);
        let payload = remapped.unwrap_or(raw);

        if let Some(observers) = self.observers.get("G") {
            let arc = Arc::new(payload);
            for &pid in observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Graphics(Arc::clone(&arc)));
                }
            }
        } else {
            // No G observer yet — buffer for replay when one subscribes.
            self.pending_graphics.push((from, payload));
        }
    }

    /// Remap `i=` and `I=` in a kitty graphics payload using per-client tables.
    fn remap_graphics_ids(&mut self, from: ProcessId, raw: &[u8]) -> Option<Vec<u8>> {
        let next_id = &mut self.next_global_image_id;
        let next_num = &mut self.next_global_image_number;
        let map = self.graphics_maps.entry(from).or_default();

        byo::kitty_gfx::rewrite_ids(
            raw,
            |local_id| {
                *map.id_map.entry(local_id).or_insert_with(|| {
                    let global = *next_id;
                    *next_id = next_id.wrapping_add(1);
                    if *next_id == 0 {
                        *next_id = 1; // skip 0
                    }
                    global
                })
            },
            |local_num| {
                *map.number_map.entry(local_num).or_insert_with(|| {
                    let global = *next_num;
                    *next_num = next_num.wrapping_add(1);
                    if *next_num == 0 {
                        *next_num = 1;
                    }
                    global
                })
            },
        )
    }

    /// Remap `$img(N)` variable references in all prop values of commands
    /// from a client, using the client's graphics ID translation table.
    fn remap_img_vars(&self, from: ProcessId, commands: &mut [Command]) {
        let Some(gfx_map) = self.graphics_maps.get(&from) else {
            return; // No images uploaded by this client — nothing to remap.
        };

        for cmd in commands.iter_mut() {
            let props = match cmd {
                Command::Upsert { props, .. }
                | Command::Patch { props, .. }
                | Command::Event { props, .. }
                | Command::Ack { props, .. } => props,
                _ => continue,
            };

            for prop in props.iter_mut() {
                if let Prop::Value { value, .. } = prop {
                    let replaced = byo::vars::replace(value, |var| {
                        if var.name == "img" {
                            if let Ok(local_id) = var.args.parse::<u32>() {
                                if let Some(&global_id) = gfx_map.id_map.get(&local_id) {
                                    return Some(format!("$img({global_id})"));
                                }
                            }
                        }
                        None
                    });
                    if let std::borrow::Cow::Owned(new_val) = replaced {
                        *value = byo::ByteStr::from(new_val);
                    }
                }
            }
        }
    }

    /// Handle passthrough bytes — forward to view observers (compositor),
    /// injecting `#redirect` frames when the target changes between clients.
    async fn handle_passthrough(&mut self, from: ProcessId, raw: Vec<u8>) {
        if let Some(queue) = self.output_queues.get_mut(&from)
            && queue.is_blocked()
        {
            queue.push(RouterMsg::Passthrough { from, raw });
            return;
        }

        let target = self
            .passthrough_targets
            .get(&from)
            .map(|s| s.as_str())
            .unwrap_or("/");

        // Discard if client set redirect to `_`.
        if target.is_empty() {
            return;
        }

        // Inject a redirect frame if the compositor's target needs switching.
        if target != self.last_forwarded_target {
            let mut redirect_buf = Vec::new();
            if target == "/" {
                let _ = write!(redirect_buf, "\n#unredirect");
            } else {
                let _ = write!(redirect_buf, "\n#redirect {target}");
            }
            self.last_forwarded_target = target.to_owned();

            let redirect_arc = Arc::new(redirect_buf);
            if let Some(observers) = self.observers.get("tty") {
                for &pid in observers {
                    if pid != from {
                        self.send_to(pid, WriteMsg::Byo(Arc::clone(&redirect_arc)));
                    }
                }
            }
        }

        if let Some(observers) = self.observers.get("tty") {
            let arc = Arc::new(raw);
            for &pid in observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Passthrough(Arc::clone(&arc)));
                }
            }
        } else {
            // No tty observer yet — buffer for replay when one subscribes.
            self.pending_passthrough.push((from, raw));
        }
    }

    /// Handle a process disconnecting.
    async fn handle_disconnect(&mut self, process: ProcessId) {
        let name = match self.client_name(process) {
            Some(n) => n.to_owned(),
            None => return,
        };
        tracing::info!("process disconnected: {name}");

        // --- Phase 1: Notify observers about destroyed objects ---
        //
        // Must happen BEFORE state tree cleanup so projection can look up
        // ancestors and descendants.

        // Find "root disconnect objects": owned by the disconnecting process,
        // whose parent (if any) is NOT owned by the same process. These are the
        // top-level objects to destroy — children cascade automatically.
        let owned_roots: Vec<(QualifiedId, String)> = self
            .state
            .values()
            .filter(|o| o.data == process)
            .filter(|o| {
                o.parent
                    .as_ref()
                    .and_then(|p| self.state.get(p))
                    .is_none_or(|parent_obj| parent_obj.data != process)
            })
            .map(|o| (o.id.clone(), o.kind.clone()))
            .collect();

        if !owned_roots.is_empty() {
            // Generate destroy commands for root objects.
            let mut destroy_buf = Vec::new();
            let mut daemon_notifications: HashMap<ProcessId, Vec<u8>> = HashMap::new();

            for (qid, kind) in &owned_roots {
                let _ = write!(destroy_buf, "\n-{kind} {qid}");

                // Collect expansion descendants owned by OTHER processes so we
                // can notify their daemons (same logic as handle_cascade_destroys).
                let descendants = self.state.descendants_info(qid);
                for (desc_qid, desc_kind, desc_owner) in &descendants {
                    if *desc_owner != process {
                        let buf = daemon_notifications.entry(*desc_owner).or_default();
                        let _ = write!(buf, "\n-{desc_kind} {desc_qid}");
                    }
                }
            }

            // Forward destroy commands to observers (projection handles type filtering).
            self.forward_to_observers(&destroy_buf, process).await;

            // Notify daemons about their expansion objects being destroyed.
            for (daemon_pid, notification) in daemon_notifications {
                if daemon_pid != process {
                    self.send_to(daemon_pid, WriteMsg::Byo(Arc::new(notification)));
                }
            }
        }

        // --- Phase 2: State tree cleanup ---

        // Destroy root objects (cascades to expansion children from other owners).
        for (qid, _) in &owned_roots {
            self.state.destroy(qid);
        }

        // Safety net: remove any remaining objects owned by this process that
        // weren't reachable as roots (shouldn't happen, but prevents leaks).
        crate::state::remove_by_owner(&mut self.state, process);

        // --- Phase 3: Subscription and routing cleanup ---

        // Remove all claims for this process.
        self.claims.retain(|_, &mut pid| pid != process);

        // Remove all observer entries for this process.
        for observers in self.observers.values_mut() {
            observers.retain(|&pid| pid != process);
        }
        self.observers.retain(|_, v| !v.is_empty());
        self.observer_types.remove(&process);

        // Remove expand seq counter.
        self.expand_seqs.remove(&process);

        // Remove event seq counters for this process.
        self.event_seqs.retain(|(pid, _), _| *pid != process);

        // Remove pending ACKs:
        // - where destination is the disconnected process (won't ACK)
        // - where sender is the disconnected process (no one to forward to)
        self.pending_acks
            .retain(|(pid, _, _), ack| *pid != process && ack.sender != process);

        // Remove request seq counters for this process.
        self.request_seqs.retain(|(pid, _), _| *pid != process);

        // Remove pending responses (same logic as pending ACKs).
        self.pending_responses
            .retain(|(pid, _, _), resp| *pid != process && resp.sender != process);

        // Clean up process state.
        self.output_queues.remove(&process);

        // Remove pending batches from this process.
        self.pending_batches.retain(|b| b.from != process);

        // Remove passthrough target.
        self.passthrough_targets.remove(&process);

        // Clean up kitty graphics — delete all images owned by this process.
        if let Some(gfx_map) = self.graphics_maps.remove(&process) {
            // Send delete commands to _gfx observers for each global image ID.
            for &global_id in gfx_map.id_map.values() {
                let delete_payload = format!("a=d,d=i,i={global_id},q=2");
                if let Some(observers) = self.observers.get("G") {
                    let arc = Arc::new(delete_payload.into_bytes());
                    for &pid in observers {
                        if pid != process {
                            self.send_to(pid, WriteMsg::Graphics(Arc::clone(&arc)));
                        }
                    }
                }
            }
        }

        // Remove from name_to_id.
        self.name_to_id.retain(|_, &mut pid| pid != process);
        self.processes.remove(&process);
    }

    /// Handle a `#claim` pragma — claim ownership of a type (fire-and-forget).
    async fn handle_claim(&mut self, from: ProcessId, client: &str, target_type: &str) {
        tracing::info!("{client} claims type '{target_type}'");
        self.claims.insert(target_type.to_owned(), from);

        // Replay reduced state for all objects of this type.
        let objects: Vec<_> = self
            .state
            .objects_of_type(target_type)
            .into_iter()
            .map(|o| (o.id.clone(), o.kind.clone()))
            .collect();

        if !objects.is_empty() {
            // Create a synthetic replay batch to track the expansion responses.
            // Empty commands signals this is a replay batch (no original app batch
            // to rewrite — the expansion results just need to be stored in the
            // state tree and forwarded to observers).
            let mut batch = PendingBatch::new(from, client.to_owned(), Vec::new());

            for (qid, _kind) in &objects {
                let expand_seq = self.next_expand_seq(from);
                let mut expand_buf = Vec::new();
                let _ = write!(expand_buf, "\n?expand {expand_seq} {qid}");
                if self.state.write_reduced_expand_props(qid, &mut expand_buf) {
                    batch.add_pending_expand(from, expand_seq, qid.clone(), 0);
                    self.send_to(from, WriteMsg::Byo(Arc::new(expand_buf)));
                }
            }

            if !batch.is_ready() {
                self.pending_batches.push(batch);
            }
        }
    }

    /// Handle a `#unclaim` pragma — release claim on a type (fire-and-forget).
    async fn handle_unclaim(&mut self, from: ProcessId, client: &str, target_type: &str) {
        tracing::info!("{client} unclaims type '{target_type}'");
        if self.claims.get(target_type) == Some(&from) {
            self.claims.remove(target_type);
        }
    }

    /// Handle a `#observe` pragma — observe final output for a type (fire-and-forget).
    fn handle_observe(&mut self, from: ProcessId, client: &str, target_type: &str) {
        tracing::info!("{client} observes type '{target_type}'");
        let observers = self.observers.entry(target_type.to_owned()).or_default();
        if !observers.contains(&from) {
            observers.push(from);
        }
        self.observer_types
            .entry(from)
            .or_default()
            .insert(target_type.to_owned());
    }

    /// Flush buffered graphics payloads to newly-registered `G` observers.
    fn flush_pending_graphics(&mut self) {
        if self.pending_graphics.is_empty() {
            return;
        }
        let Some(observers) = self.observers.get("G") else {
            return;
        };
        let observers: Vec<ProcessId> = observers.clone();
        let pending = std::mem::take(&mut self.pending_graphics);
        for (from, payload) in pending {
            let arc = Arc::new(payload);
            for &pid in &observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Graphics(Arc::clone(&arc)));
                }
            }
        }
    }

    /// Flush buffered passthrough payloads to newly-registered `tty` observers.
    fn flush_pending_passthrough(&mut self) {
        if self.pending_passthrough.is_empty() {
            return;
        }
        let Some(observers) = self.observers.get("tty") else {
            return;
        };
        let observers: Vec<ProcessId> = observers.clone();
        let pending = std::mem::take(&mut self.pending_passthrough);
        for (from, raw) in pending {
            // Inject redirect frames as needed (same logic as handle_passthrough).
            let target = self
                .passthrough_targets
                .get(&from)
                .map(|s| s.as_str())
                .unwrap_or("/");
            if target.is_empty() {
                continue; // discard
            }
            if target != self.last_forwarded_target {
                let mut redirect_buf = Vec::new();
                if target == "/" {
                    let _ = write!(redirect_buf, "\n#unredirect");
                } else {
                    let _ = write!(redirect_buf, "\n#redirect {target}");
                }
                self.last_forwarded_target = target.to_owned();
                let redirect_arc = Arc::new(redirect_buf);
                for &pid in &observers {
                    if pid != from {
                        self.send_to(pid, WriteMsg::Byo(Arc::clone(&redirect_arc)));
                    }
                }
            }
            let arc = Arc::new(raw);
            for &pid in &observers {
                if pid != from {
                    self.send_to(pid, WriteMsg::Passthrough(Arc::clone(&arc)));
                }
            }
        }
    }

    /// Handle a `#unobserve` pragma — stop observing a type (fire-and-forget).
    fn handle_unobserve(&mut self, from: ProcessId, client: &str, target_type: &str) {
        tracing::info!("{client} unobserves type '{target_type}'");
        if let Some(observers) = self.observers.get_mut(target_type) {
            observers.retain(|&pid| pid != from);
            if observers.is_empty() {
                self.observers.remove(target_type);
            }
        }
        if let Some(types) = self.observer_types.get_mut(&from) {
            types.remove(target_type);
            if types.is_empty() {
                self.observer_types.remove(&from);
            }
        }
    }

    /// Route an event to the owner of the target object.
    ///
    /// Qualifies the target ID, looks up the owner in the state tree,
    /// remaps the sequence number per-destination, and forwards with
    /// the dequalified target ID.
    async fn handle_event(
        &mut self,
        from: ProcessId,
        client: &str,
        kind: &EventKind,
        seq: u64,
        id: &str,
        props: &[Prop],
    ) {
        use crate::id::{dequalify, qualify};

        let qualified_id = qualify(client, id);
        let qid = match QualifiedId::parse(&qualified_id) {
            Some(q) => q,
            None => {
                tracing::warn!("handle_event: can't qualify {id} from {client}");
                return;
            }
        };

        // Look up the owner of this object in the state tree.
        let target_pid = match self.state.get(&qid) {
            Some(obj) => obj.data,
            None => {
                tracing::warn!(
                    "handle_event: {qid} not in state tree (event={} from={client})",
                    kind.as_str()
                );
                return;
            }
        };

        // Don't route events back to the sender.
        if target_pid == from {
            tracing::warn!("handle_event: dropping {qid} — target_pid == from ({client})");
            return;
        }

        // Get the destination's client name for dequalification.
        let dest_client = match self.client_name(target_pid) {
            Some(n) => n.to_owned(),
            None => return,
        };

        let event_type = kind.as_str();
        let remapped_seq = self.next_event_seq(target_pid, event_type);
        let dequalified_target = dequalify(&dest_client, &qualified_id);

        tracing::info!(
            "handle_event: routing {event_type}({seq}) {qid} → pid={} as {event_type}({remapped_seq}) {dequalified_target}",
            target_pid.0
        );

        // Serialize the event with remapped seq and dequalified target.
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        let _ = em.event(event_type, remapped_seq, dequalified_target, props);

        self.send_to(target_pid, WriteMsg::Byo(Arc::new(buf)));

        // Store PendingAck for the return trip.
        self.pending_acks.insert(
            (target_pid, event_type.to_owned(), remapped_seq),
            PendingAck {
                sender: from,
                sender_seq: seq,
                forwarded_at: Instant::now(),
            },
        );
    }

    /// Route an ACK back to the original event sender.
    ///
    /// Looks up the pending ACK by (from, event_type, seq), remaps back
    /// to the original sender's sequence number, and forwards.
    async fn handle_ack(&mut self, from: ProcessId, kind: &EventKind, seq: u64, props: &[Prop]) {
        let event_type = kind.as_str().to_owned();
        let pending = self.pending_acks.remove(&(from, event_type.clone(), seq));

        let Some(pending) = pending else {
            tracing::warn!(
                "handle_ack: no pending ACK for {}({seq}) from pid={}",
                event_type,
                from.0
            );
            return;
        };

        tracing::info!(
            "handle_ack: routing {}({seq}) from pid={} → pid={} as {}({})",
            event_type,
            from.0,
            pending.sender.0,
            event_type,
            pending.sender_seq
        );

        // Serialize the ACK with the original sender's seq.
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        let _ = em.ack(&event_type, pending.sender_seq, props);

        self.send_to(pending.sender, WriteMsg::Byo(Arc::new(buf)));
    }

    /// Route a custom request to the owner of the target object.
    ///
    /// Same pattern as event routing: qualify target, look up owner,
    /// remap seq per-destination, dequalify, forward, store pending response.
    async fn handle_request(
        &mut self,
        from: ProcessId,
        client: &str,
        kind_name: &str,
        seq: u64,
        target: &str,
        props: &[Prop],
    ) {
        use crate::id::{dequalify, qualify};

        let qualified_id = qualify(client, target);
        let qid = match QualifiedId::parse(&qualified_id) {
            Some(q) => q,
            None => return,
        };

        let target_pid = match self.state.get(&qid) {
            Some(obj) => obj.data,
            None => return, // Object doesn't exist — drop silently
        };

        if target_pid == from {
            return;
        }

        let dest_client = match self.client_name(target_pid) {
            Some(n) => n.to_owned(),
            None => return,
        };

        let remapped_seq = self.next_request_seq(target_pid, kind_name);
        let dequalified_target = dequalify(&dest_client, &qualified_id);

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        let _ = em.request(kind_name, remapped_seq, dequalified_target, props);

        self.send_to(target_pid, WriteMsg::Byo(Arc::new(buf)));

        self.pending_responses.insert(
            (target_pid, kind_name.to_owned(), remapped_seq),
            PendingResponse {
                sender: from,
                sender_seq: seq,
                forwarded_at: Instant::now(),
            },
        );
    }

    /// Route a custom response back to the original request sender.
    ///
    /// Same pattern as ACK routing: look up pending response, remap seq,
    /// forward to original sender.
    async fn handle_response(
        &mut self,
        from: ProcessId,
        kind_name: &str,
        seq: u64,
        props: &[Prop],
        body: &Option<Vec<Command>>,
    ) {
        let kind_owned = kind_name.to_owned();
        let pending = self
            .pending_responses
            .remove(&(from, kind_owned.clone(), seq));

        let Some(pending) = pending else {
            return; // Stale or unknown response — drop silently
        };

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        if let Some(body) = body {
            let _ = em.response_with(kind_name, pending.sender_seq, props, |em| em.commands(body));
        } else {
            let _ = em.response(kind_name, pending.sender_seq, props);
        }

        self.send_to(pending.sender, WriteMsg::Byo(Arc::new(buf)));
    }

    /// Resync an observer by replaying the full projected state tree.
    ///
    /// Called when an observer's subscription set changes (observe/unobserve).
    /// Since `+` is idempotent, a full replay is lossless.
    async fn resync_observer(&self, pid: ProcessId) {
        let types = match self.observer_types.get(&pid) {
            Some(t) => t,
            None => return,
        };
        let replay = crate::state::project_tree(&self.state, types);
        if !replay.is_empty() {
            self.send_to(pid, WriteMsg::Byo(Arc::new(replay)));
        }
    }

    /// Handle a `.expand` response from a daemon.
    async fn handle_expand_response(
        &mut self,
        from: ProcessId,
        client: &str,
        seq: u64,
        body: &Option<Vec<Command>>,
    ) {
        // Look up the current depth of this expansion before recording.
        let current_depth = self
            .pending_batches
            .iter()
            .flat_map(|b| b.pending_expands.iter())
            .find(|e| e.subscriber == from && e.seq == seq)
            .map(|e| e.depth)
            .unwrap_or(0);

        // Record qualified commands if body is present. The body from the
        // parser contains just the inner commands (no wrapping Push/Pop).
        if let Some(body_cmds) = body {
            let qualified_cmds = crate::batch::qualify_commands(body_cmds, client);

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

            self.record_expansion_output(from, seq, qualified_cmds);
        }
        self.complete_expansion(from, seq).await;
    }

    /// Scan expansion body for nested daemon-owned types and trigger
    /// additional `?expand` requests.
    async fn trigger_nested_expansions(
        &mut self,
        from: ProcessId,
        seq: u64,
        body_cmds: &[Command],
        daemon_client: &str,
        current_depth: u32,
    ) {
        use crate::batch::write_props;
        use crate::id::qualify;

        let mut nested_expands = Vec::new();

        for cmd in body_cmds {
            if let Command::Upsert { kind, id, props } = cmd
                && *id != "_"
                && let Some(&subscriber) = self.claims.get(&**kind)
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
                    self.send_to(subscriber, WriteMsg::Byo(Arc::new(expand_buf)));
                }
            }
        }
    }

    /// Update the state tree with expansion objects from a completed batch.
    ///
    /// Expansion objects become **children** of the source object in the state
    /// tree. This means cascade destroy of the source automatically cleans
    /// up all expansion objects, and the tree relationship itself encodes the
    /// expansion link (no `expanded_from` field needed).
    fn update_expansion_state(&mut self, batch: &PendingBatch) {
        for (source_qid_str, (daemon_pid, expansion_cmds, _is_re_expand)) in &batch.expansions {
            let Some(source_qid) = QualifiedId::parse(source_qid_str) else {
                continue;
            };

            let mut parent_stack: Vec<QualifiedId> = Vec::new();
            let mut last_qid: Option<QualifiedId> = None;

            for cmd in expansion_cmds {
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

                        // Parent: root-level expansion objects → source object,
                        // nested → their push parent within the expansion.
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&qid, parent);
                        } else {
                            self.state.set_parent(&qid, &source_qid);
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
        new_expansion_cmds: &[Command],
    ) -> Vec<u8> {
        use std::io::Write;

        // Collect old expansion objects (all descendants of the source object).
        let descendants = self.state.descendants_info(source_qid);
        let old_objects: HashMap<QualifiedId, IndexMap<String, PropValue>> = descendants
            .iter()
            .filter_map(|(qid, _, _)| self.state.get(qid).map(|o| (qid.clone(), o.props.clone())))
            .collect();

        let old_kinds: HashMap<QualifiedId, String> = descendants
            .iter()
            .map(|(qid, kind, _)| (qid.clone(), kind.clone()))
            .collect();

        let mut delta = Vec::new();
        let mut seen_qids: std::collections::HashSet<QualifiedId> =
            std::collections::HashSet::new();
        let mut parent_stack: Vec<QualifiedId> = Vec::new();
        let mut last_qid: Option<QualifiedId> = None;

        for cmd in new_expansion_cmds {
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

                    if let Some(old_props) = old_objects.get(&qid) {
                        // Existing object — compute delta.
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
                        // New object — emit create.
                        crate::batch::write_upsert(&mut delta, kind, &qid.to_string(), props);
                    }

                    // Update state tree.
                    self.state.upsert(kind, &qid, &new_props, daemon_pid);
                    if let Some(parent) = parent_stack.last() {
                        self.state.set_parent(&qid, parent);
                    } else {
                        self.state.set_parent(&qid, source_qid);
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

        // Emit destroys for objects that are in old but not in new.
        for qid in old_objects.keys() {
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
    /// Since expansion objects are children of the source object in the state
    /// tree, destroying the source cascades to all expansion objects
    /// automatically. We just need to:
    /// 1. Collect descendant info for compositor destroy commands and daemon notifications
    /// 2. Emit `-kind qid` for direct expansion children (compositor cascades the rest)
    /// 3. Notify owning daemons about destroyed objects
    /// 4. Destroy the source object (cascades to all children)
    async fn handle_cascade_destroys(
        &mut self,
        claimed_destroys: &[QualifiedId],
        rewritten: &mut Vec<u8>,
    ) {
        use std::io::Write;

        for source_qid in claimed_destroys {
            // Collect info about all descendants before destroying.
            let all_descendants = self.state.descendants_info(source_qid);

            // Emit destroy commands for direct children only — compositor
            // cascades the rest via its own tree semantics.
            if let Some(source) = self.state.get(source_qid) {
                for child_qid in source.children.clone() {
                    if let Some(child) = self.state.get(&child_qid) {
                        let _ = write!(rewritten, "\n-{} {child_qid}", child.kind);
                    }
                }
            }

            // Group all destroyed descendants by owner daemon and notify.
            let mut daemon_notifications: HashMap<ProcessId, Vec<u8>> = HashMap::new();
            for (qid, kind, owner) in &all_descendants {
                let buf = daemon_notifications.entry(*owner).or_default();
                let _ = write!(buf, "\n-{kind} {qid}");
            }

            for (daemon_pid, notification_buf) in daemon_notifications {
                self.send_to(daemon_pid, WriteMsg::Byo(Arc::new(notification_buf)));
            }

            // Destroy the source object — cascades to all children/expansion objects.
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

        let batch = &mut self.pending_batches[idx];
        // Complete the specific expand and get the QID back.
        // Expansion bytes are recorded separately via record_expansion_output.
        batch.complete_expand(from, seq);

        if batch.is_ready() {
            let batch = self.pending_batches.remove(idx);
            let is_replay = batch.commands.is_empty();

            // Reconcile re-expansions using the per-expansion flag.
            let mut reconciliation_buf = Vec::new();
            for (source_qid_str, (daemon_pid, expansion_cmds, is_re_expand)) in &batch.expansions {
                if *is_re_expand && let Some(source_qid) = QualifiedId::parse(source_qid_str) {
                    let delta = self.reconcile_expansion(&source_qid, *daemon_pid, expansion_cmds);
                    reconciliation_buf.extend_from_slice(&delta);
                }
            }

            let (mut rewritten, claimed_destroys) = batch.rewrite(&self.claims);

            // Handle cascade destroys BEFORE updating expansion state so that
            // old expansion children are cleaned up before new ones are added.
            self.handle_cascade_destroys(&claimed_destroys, &mut rewritten)
                .await;

            // Update state tree with expansion nodes (for initial expansions).
            self.update_expansion_state(&batch);

            // Append reconciliation output.
            rewritten.extend_from_slice(&reconciliation_buf);

            let process_id = batch.from;

            if is_replay {
                // Replay batch (from #claim): no original commands to rewrite.
                // The expansion views are now in the state tree — resync all
                // observers so they receive the expanded content.
                let observer_pids: Vec<_> = self.observer_types.keys().copied().collect();
                for pid in observer_pids {
                    self.resync_observer(pid).await;
                }
            } else {
                // Normal batch: forward rewritten output to observers.
                self.forward_to_observers(&rewritten, process_id).await;

                // Unblock the process output queue and flush.
                if let Some(queue) = self.output_queues.get_mut(&process_id) {
                    queue.unblock();
                    let queued = queue.drain();
                    for msg in queued {
                        Box::pin(self.handle(msg)).await;
                    }
                }
            }
        }
    }

    /// Record a daemon's expansion output for a pending batch.
    pub fn record_expansion_output(&mut self, from: ProcessId, seq: u64, commands: Vec<Command>) {
        let batch = self
            .pending_batches
            .iter_mut()
            .find(|b| b.has_pending_expand(from, seq));

        if let Some(batch) = batch {
            // Find the QID and is_re_expand flag associated with this (from, seq) pair.
            let expand_info = batch
                .pending_expands
                .iter()
                .find(|e| e.subscriber == from && e.seq == seq)
                .map(|e| (e.qid.clone(), e.is_re_expand));

            if let Some((ref qid, is_re_expand)) = expand_info {
                batch.record_expansion(qid, from, commands, is_re_expand);
            }
        }
    }

    /// Forward rewritten payload bytes to all observers, filtered per observer's
    /// type subscriptions.
    async fn forward_to_observers(&self, payload: &[u8], _from: ProcessId) {
        // Collect unique observer PIDs.
        let mut targets: Vec<ProcessId> = Vec::new();
        for observers in self.observers.values() {
            for &pid in observers {
                if !targets.contains(&pid) {
                    targets.push(pid);
                }
            }
        }

        if targets.is_empty() {
            return;
        }

        // Parse the rewritten payload for per-observer projection.
        let payload_str = match std::str::from_utf8(payload) {
            Ok(s) => s,
            Err(_) => return,
        };
        let commands = match parse(payload_str) {
            Ok(cmds) => cmds,
            Err(_) => return,
        };

        for target in targets {
            let projected = if let Some(types) = self.observer_types.get(&target) {
                project_commands(&commands, types, &self.state)
            } else {
                // No type filter — send everything.
                payload.to_vec()
            };
            if !projected.is_empty() {
                self.send_to(target, WriteMsg::Byo(Arc::new(projected)));
            }
        }
    }

    /// Send a message to a process.
    ///
    /// Uses an unbounded channel so this never blocks the router loop.
    /// The writer task provides natural backpressure via pipe writes.
    fn send_to(&self, target: ProcessId, msg: WriteMsg) {
        if let Some(process) = self.processes.get(&target) {
            let _ = process.tx.send(msg);
        }
    }

    /// Get the next expand sequence number for a subscriber.
    fn next_expand_seq(&mut self, subscriber: ProcessId) -> u64 {
        let seq = self.expand_seqs.entry(subscriber).or_insert(0);
        let current = *seq;
        *seq += 1;
        current
    }

    /// Get the next event sequence number for a (destination, event_type) pair.
    fn next_event_seq(&mut self, target: ProcessId, event_type: &str) -> u64 {
        let seq = self
            .event_seqs
            .entry((target, event_type.to_owned()))
            .or_insert(0);
        let current = *seq;
        *seq += 1;
        current
    }

    /// Get the next request sequence number for a (destination, request_kind) pair.
    fn next_request_seq(&mut self, target: ProcessId, request_kind: &str) -> u64 {
        let seq = self
            .request_seqs
            .entry((target, request_kind.to_owned()))
            .or_insert(0);
        let current = *seq;
        *seq += 1;
        current
    }
}

/// Project a sequence of commands through a type filter, keeping only
/// observed types and re-parenting observed children of non-observed objects.
///
/// Uses the state tree for ancestor lookup when a non-observed object at the
/// top level (outside any observed context) has observed children that need
/// to be placed under their nearest observed ancestor.
fn project_commands(
    commands: &[Command],
    observed_types: &HashSet<String>,
    state: &ObjectTree,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut i = 0;
    project_at_level(commands, observed_types, state, &mut i, &mut buf, false);
    buf
}

/// Recursive helper for `project_commands`. Processes commands at the current
/// nesting level (delimited by Push/Pop).
///
/// `in_observed_context` is true when we're inside the children block of an
/// observed object — in that case, flattening non-observed objects is safe because
/// the children naturally end up under the observed parent. When false (top
/// level), non-observed objects with observed children need state-tree lookup
/// to find the correct observed ancestor to wrap them under.
fn project_at_level(
    commands: &[Command],
    observed_types: &HashSet<String>,
    state: &ObjectTree,
    i: &mut usize,
    buf: &mut Vec<u8>,
    in_observed_context: bool,
) {
    while *i < commands.len() {
        match &commands[*i] {
            Command::Upsert { kind, id, props } => {
                let is_observed = observed_types.contains(&**kind);
                *i += 1;

                let has_children = *i < commands.len() && matches!(commands[*i], Command::Push);

                if has_children {
                    *i += 1; // consume Push

                    if is_observed {
                        let child_buf = collect_children(commands, observed_types, state, i);
                        let mut em = Emitter::new(&mut *buf);
                        if child_buf.is_empty() {
                            let _ = em.upsert(kind, id, props);
                        } else {
                            let _ = em.upsert_with(kind, id, props, |em| em.raw(&child_buf));
                        }
                    } else if in_observed_context {
                        // Inside an observed parent — flatten children here.
                        project_at_level(commands, observed_types, state, i, buf, true);
                    } else {
                        // Top level, non-observed object — wrap children under
                        // nearest observed ancestor from the state tree.
                        project_under_ancestor(commands, observed_types, state, i, buf, id);
                    }
                } else if is_observed {
                    let mut em = Emitter::new(&mut *buf);
                    let _ = em.upsert(kind, id, props);
                }
            }
            Command::Destroy { kind, id } => {
                if observed_types.contains(&**kind) {
                    let mut em = Emitter::new(&mut *buf);
                    let _ = em.destroy(kind, id);
                } else {
                    // Non-observed type destroyed — emit destroys for any
                    // observed descendants so the observer can clean up.
                    emit_observed_descendant_destroys(state, id, observed_types, buf);
                }
                *i += 1;
            }
            Command::Patch { kind, id, props } => {
                let is_observed = observed_types.contains(&**kind);
                *i += 1;

                let has_children = *i < commands.len() && matches!(commands[*i], Command::Push);

                if has_children {
                    *i += 1; // consume Push

                    if is_observed {
                        let child_buf = collect_children(commands, observed_types, state, i);
                        let mut em = Emitter::new(&mut *buf);
                        if child_buf.is_empty() {
                            let _ = em.patch(kind, id, props);
                        } else {
                            let _ = em.patch_with(kind, id, props, |em| em.raw(&child_buf));
                        }
                    } else if in_observed_context {
                        project_at_level(commands, observed_types, state, i, buf, true);
                    } else {
                        project_under_ancestor(commands, observed_types, state, i, buf, id);
                    }
                } else if is_observed {
                    let mut em = Emitter::new(&mut *buf);
                    let _ = em.patch(kind, id, props);
                }
            }
            Command::Pop => {
                *i += 1;
                return; // End of current level
            }
            _ => {
                *i += 1;
            }
        }
    }
}

/// Recurse into children, returning the projected bytes (empty if nothing observed).
fn collect_children(
    commands: &[Command],
    observed_types: &HashSet<String>,
    state: &ObjectTree,
    i: &mut usize,
) -> Vec<u8> {
    let mut child_buf = Vec::new();
    project_at_level(commands, observed_types, state, i, &mut child_buf, true);
    child_buf
}

/// Project children of a non-observed object, wrapping them under the object's
/// nearest observed ancestor from the state tree.
///
/// If no observed ancestor exists, children are emitted at the top level.
fn project_under_ancestor(
    commands: &[Command],
    observed_types: &HashSet<String>,
    state: &ObjectTree,
    i: &mut usize,
    buf: &mut Vec<u8>,
    id: &str,
) {
    // Look up the nearest observed ancestor in the state tree.
    let ancestor = QualifiedId::parse(id)
        .and_then(|qid| crate::state::nearest_observed_ancestor(state, &qid, observed_types));

    if let Some(ref ancestor_qid) = ancestor
        && let Some(ancestor_obj) = state.get(ancestor_qid)
    {
        // Wrap children under `@ancestorKind ancestorQid { ... }`.
        let child_buf = collect_children(commands, observed_types, state, i);

        if !child_buf.is_empty() {
            let ancestor_str = ancestor_qid.to_string();
            let mut em = Emitter::new(&mut *buf);
            let _ = em.patch_with(&ancestor_obj.kind, &ancestor_str, &[], |em| {
                em.raw(&child_buf)
            });
        }
    } else {
        // No observed ancestor — emit children at top level.
        project_at_level(commands, observed_types, state, i, buf, false);
    }
}

/// Emit `-kind qid` for each observed descendant of a non-observed destroyed
/// object, deepest-first so the observer can cascade correctly.
fn emit_observed_descendant_destroys(
    state: &ObjectTree,
    id: &str,
    observed_types: &HashSet<String>,
    buf: &mut Vec<u8>,
) {
    let Some(qid) = QualifiedId::parse(id) else {
        return;
    };
    let descendants = state.descendants_info(&qid);
    // descendants_info returns pre-order (parent before child).
    // Reverse to get deepest-first for destroy ordering.
    for (desc_qid, kind, _owner) in descendants.iter().rev() {
        if observed_types.contains(kind) {
            let desc_str = desc_qid.to_string();
            let mut em = Emitter::new(&mut *buf);
            let _ = em.destroy(kind, &desc_str);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;
    use byo::protocol::Prop;

    #[test]
    fn project_all_types_observed() {
        let state = ObjectTree::new();
        let commands = parse("+view app:root { +text app:label content=Hello }").unwrap();
        let types: HashSet<String> = ["view", "text"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(
            &result,
            r#"+view app:root { +text app:label content=Hello }"#,
        );
    }

    #[test]
    fn project_skip_unobserved_leaf() {
        let state = ObjectTree::new();
        let commands =
            parse("+view app:root { +text app:label content=Hello +image app:bg src=bg.png }")
                .unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "+view app:root");
    }

    #[test]
    fn project_flatten_children() {
        let state = ObjectTree::new();
        // root (view) → container (panel, not observed) → child (view, observed)
        let commands =
            parse("+view app:root { +panel app:container { +view app:child class=inner } }")
                .unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "+view app:root { +view app:child class=inner }");
    }

    #[test]
    fn project_destroy_observed() {
        let state = ObjectTree::new();
        let commands = parse("-view app:sidebar -text app:label").unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "-view app:sidebar");
    }

    #[test]
    fn project_patch_observed() {
        let state = ObjectTree::new();
        let commands = parse("@view app:sidebar hidden @text app:label content=New").unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "@view app:sidebar hidden");
    }

    #[test]
    fn project_empty_filter() {
        let state = ObjectTree::new();
        let commands = parse("+view app:root { +text app:label content=Hello }").unwrap();
        let types: HashSet<String> = HashSet::new();
        let result = project_commands(&commands, &types, &state);
        assert!(
            result.is_empty(),
            "expected empty, got: {:?}",
            String::from_utf8(result)
        );
    }

    #[test]
    fn project_deeply_nested_flatten() {
        let state = ObjectTree::new();
        // a (view) → b (panel) → c (panel) → d (view)
        // With only view observed, d should be re-parented under a.
        let commands = parse("+view a { +panel b { +panel c { +view d } } }").unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "+view a { +view d }");
    }

    #[test]
    fn project_patch_with_children() {
        let state = ObjectTree::new();
        let commands =
            parse("@view app:root { +text app:label content=Hello +view app:child }").unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "@view app:root { +view app:child }");
    }

    #[test]
    fn project_top_level_unobserved_wraps_under_ancestor() {
        // State tree: app:root (view) → app:container (panel)
        // Batch: @panel app:container { +view app:child class=inner }
        // Observer observes [view]. Panel is not observed and is at the
        // top level of the batch. The projection should wrap children
        // under the nearest observed ancestor from the state tree.
        use crate::process::ProcessId;

        let mut state = ObjectTree::new();
        let root_qid = QualifiedId::new("app", "root");
        let container_qid = QualifiedId::new("app", "container");
        state.upsert("view", &root_qid, &IndexMap::new(), ProcessId(1));
        state.upsert("panel", &container_qid, &IndexMap::new(), ProcessId(1));
        state.set_parent(&container_qid, &root_qid);

        let commands = parse("@panel app:container { +view app:child class=inner }").unwrap();
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "@view app:root { +view app:child class=inner }");
    }

    #[test]
    fn project_destroy_unobserved_emits_descendant_destroys() {
        // State tree: app:container (panel) → app:child (view) → app:label (text)
        // Batch: -panel app:container
        // Observer observes [view, text]. Should emit destroys for observed descendants.
        use crate::process::ProcessId;

        let mut state = ObjectTree::new();
        let container = QualifiedId::new("app", "container");
        let child = QualifiedId::new("app", "child");
        let label = QualifiedId::new("app", "label");
        state.upsert("panel", &container, &IndexMap::new(), ProcessId(1));
        state.upsert("view", &child, &IndexMap::new(), ProcessId(1));
        state.upsert("text", &label, &IndexMap::new(), ProcessId(1));
        state.set_parent(&child, &container);
        state.set_parent(&label, &child);

        let commands = parse("-panel app:container").unwrap();
        let types: HashSet<String> = ["view", "text"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&result, "-text app:label -view app:child");
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
}
