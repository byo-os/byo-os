//! Central message router.
//!
//! All messages from all processes flow through the router. It parses BYO
//! payloads ephemerally to make routing decisions, manages subscriptions,
//! triggers daemon expansion, and maintains per-process output ordering.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use indexmap::IndexMap;

use byo::ByteStr;
use byo::byo_vec;
use byo::protocol::{Command, EventKind, MessageKind, PragmaKind, Prop, RequestKind, ResponseKind};
use byo::tree::{ObjectKind, PropValue, props_to_map, props_to_patch};

use crate::batch::{OutputQueue, PendingBatch};
use crate::id::QualifiedId;
use crate::process::{Process, ProcessId, ProcessKind, WriteMsg};
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
    /// (type_name, request_kind) → ONE handler process.
    handlers: HashMap<(String, String), ProcessId>,
    /// (type_name, event_kind) → MULTIPLE tapping processes (event eavesdroppers).
    taps: HashMap<(String, String), Vec<ProcessId>>,
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
    /// (from, qualified_target, raw) — target captured at buffering time.
    pending_passthrough: Vec<(ProcessId, String, Vec<u8>)>,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            processes: HashMap::new(),
            claims: HashMap::new(),
            handlers: HashMap::new(),
            taps: HashMap::new(),
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

    /// Returns true if the router has any connected non-virtual processes.
    pub fn has_processes(&self) -> bool {
        self.processes
            .values()
            .any(|p| p.kind == ProcessKind::Subprocess)
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
                Command::Pragma(PragmaKind::Claim(types)) => {
                    for target in types {
                        self.handle_claim(from, &client, target).await;
                    }
                }
                Command::Pragma(PragmaKind::Unclaim(types)) => {
                    for target in types {
                        self.handle_unclaim(from, &client, target).await;
                    }
                }
                Command::Pragma(PragmaKind::Observe(types)) => {
                    for target in types {
                        self.handle_observe(from, &client, target);
                    }
                    resync_pids.push(from);
                }
                Command::Pragma(PragmaKind::Unobserve(types)) => {
                    for target in types {
                        self.handle_unobserve(from, &client, target);
                    }
                    resync_pids.push(from);
                }
                Command::Pragma(PragmaKind::Redirect(target)) => {
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
                Command::Pragma(PragmaKind::Unredirect) => {
                    self.passthrough_targets.insert(from, "/".to_owned());
                    tracing::debug!("{client} unredirect passthrough to root tty");
                }
                Command::Pragma(PragmaKind::Handle(targets)) => {
                    for (type_name, request_kind) in targets {
                        self.register_handler(from, &client, type_name, request_kind);
                    }
                }
                Command::Pragma(PragmaKind::Unhandle(targets)) => {
                    for (type_name, request_kind) in targets {
                        self.unregister_handler(from, &client, type_name, request_kind);
                    }
                }
                Command::Pragma(PragmaKind::Tap(targets)) => {
                    for (type_name, event_kind) in targets {
                        self.register_tap(from, &client, type_name, event_kind);
                    }
                }
                Command::Pragma(PragmaKind::Untap(targets)) => {
                    for (type_name, event_kind) in targets {
                        self.unregister_tap(from, &client, type_name, event_kind);
                    }
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
                Command::Message {
                    kind,
                    target,
                    props,
                    body,
                } => {
                    self.handle_message(from, &client, kind, target, props, body)
                        .await;
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
                        let qid_str = qid.to_string();
                        let kind_val = &**kind;
                        let expand_cmds = byo_vec! {
                            ?expand {expand_seq} {qid_str} kind={kind_val} {..props}
                        };

                        expand_msgs.push((subscriber, qid, expand_seq, expand_cmds, false));
                    }
                }
                Command::Patch { kind, id, .. } if *id != "_" => {
                    if let Some(&subscriber) = self.claims.get(&**kind)
                        && subscriber != from
                    {
                        let qid = QualifiedId::new(&client, id);

                        let expand_seq = self.next_expand_seq(subscriber);
                        if let Some((_kind_str, props)) =
                            crate::state::expand_props(&self.state, &qid)
                        {
                            let qid_str = qid.to_string();
                            let expand_cmds = byo_vec! {
                                ?expand {expand_seq} {qid_str} {..props}
                            };
                            expand_msgs.push((subscriber, qid, expand_seq, expand_cmds, true));
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

            for (subscriber, qid, expand_seq, expand_cmds, is_re_expand) in expand_msgs {
                if is_re_expand {
                    batch.add_pending_re_expand(subscriber, expand_seq, qid);
                } else {
                    batch.add_pending_expand(subscriber, expand_seq, qid, 0);
                }
                self.send_to(subscriber, WriteMsg::Byo(Arc::new(expand_cmds)));
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
        let mut last_was_claimed = false;

        for cmd in commands {
            match cmd {
                Command::Upsert { kind, id, props } => {
                    if *id == "_" {
                        continue;
                    }
                    let qid = QualifiedId::new(client, id);
                    let map = props_to_map(props);
                    self.state.upsert(kind.as_ref().into(), &qid, &map, owner);
                    if let Some(parent) = parent_stack.last() {
                        self.state.set_parent(&qid, parent);
                    }
                    last_was_claimed = self.claims.contains_key(&**kind) && *id != "_";
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
                Command::Push { slot } => {
                    if let Some(name) = slot {
                        // Slot push — create a SlotContent node in the state tree.
                        // QID format: client:parent::name (e.g. app:d::header).
                        let parent_local = last_qid.as_ref().map(|q| q.local_id()).unwrap_or("");
                        let slot_qid = QualifiedId::new(client, &format!("{parent_local}::{name}"));
                        self.state.upsert(
                            ObjectKind::SlotContent,
                            &slot_qid,
                            &IndexMap::new(),
                            owner,
                        );
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&slot_qid, parent);
                        }
                        parent_stack.push(slot_qid.clone());
                        last_qid = Some(slot_qid);
                    } else if last_was_claimed {
                        // Bare children of a claimed type → synthetic default
                        // SlotContent node with name "_".
                        let parent_local = last_qid.as_ref().map(|q| q.local_id()).unwrap_or("");
                        let slot_qid = QualifiedId::new(client, &format!("{parent_local}::_"));
                        self.state.upsert(
                            ObjectKind::SlotContent,
                            &slot_qid,
                            &IndexMap::new(),
                            owner,
                        );
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&slot_qid, parent);
                        }
                        parent_stack.push(slot_qid.clone());
                        last_qid = Some(slot_qid);
                    } else if let Some(ref qid) = last_qid {
                        parent_stack.push(qid.clone());
                    }
                    last_was_claimed = false;
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
                        if var.name != "img" {
                            return None;
                        }
                        let entries = byo::vars::parse_img_args(var.args)?;
                        let mut any_changed = false;
                        let remapped: Vec<_> = entries
                            .into_iter()
                            .map(|mut entry| {
                                if let Some(&global_id) = gfx_map.id_map.get(&entry.id) {
                                    entry.id = global_id;
                                    any_changed = true;
                                }
                                entry
                            })
                            .collect();
                        if any_changed {
                            Some(format!("$img({})", byo::vars::format_img_args(&remapped)))
                        } else {
                            None
                        }
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

        // Qualify the target with the client name so the compositor can
        // look it up in its IdMap (which stores qualified IDs).
        let qualified_target = if target == "/" {
            "/".to_owned()
        } else {
            let client = self.client_name(from).unwrap_or("");
            crate::id::qualify(client, target)
        };

        if let Some(observers) = self.observers.get("tty") {
            let observers: Vec<ProcessId> = observers.clone();

            // Inject a redirect frame if the compositor's target needs switching.
            if qualified_target != self.last_forwarded_target {
                let qt = qualified_target.as_str();
                let redirect_cmds = if qualified_target == "/" {
                    byo_vec! { #unredirect }
                } else {
                    byo_vec! { #redirect {qt} }
                };
                self.last_forwarded_target.clone_from(&qualified_target);

                let redirect_arc = Arc::new(redirect_cmds);
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
        } else {
            // No tty observer yet — buffer target + data for replay.
            self.pending_passthrough.push((from, qualified_target, raw));
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
        let owned_roots: Vec<(QualifiedId, ObjectKind)> = self
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
            let mut destroy_cmds = Vec::new();
            let mut daemon_notifications: HashMap<ProcessId, Vec<Command>> = HashMap::new();

            for (qid, kind) in &owned_roots {
                destroy_cmds.push(Command::Destroy {
                    kind: ByteStr::from(kind.to_string()),
                    id: ByteStr::from(qid.to_string()),
                });

                // Collect expansion descendants owned by OTHER processes so we
                // can notify their daemons (same logic as handle_cascade_destroys).
                let descendants = self.state.descendants_info(qid);
                for (desc_qid, desc_kind, desc_owner) in &descendants {
                    if *desc_owner != process {
                        let cmds = daemon_notifications.entry(*desc_owner).or_default();
                        cmds.push(Command::Destroy {
                            kind: ByteStr::from(desc_kind.to_string()),
                            id: ByteStr::from(desc_qid.to_string()),
                        });
                    }
                }
            }

            // Forward destroy commands to observers (projection handles type filtering).
            self.forward_to_observers(&destroy_cmds, process).await;

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

        // Remove all handler registrations for this process.
        self.handlers.retain(|_, &mut pid| pid != process);

        // Remove all tap registrations for this process.
        for tappers in self.taps.values_mut() {
            tappers.retain(|&pid| pid != process);
        }
        self.taps.retain(|_, v| !v.is_empty());

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

        // Retroactively wrap bare children of claimed objects in synthetic
        // SlotContent nodes. When the app batch arrived before this daemon
        // claimed the type, `update_state` didn't know it was claimed, so
        // bare `{ ... }` children were stored as direct children instead of
        // under a `::_` SlotContent node. Fix that now so slot substitution
        // works during replay expansion.
        {
            let objs: Vec<_> = self
                .state
                .objects_of_type(target_type)
                .into_iter()
                .map(|o| (o.id.clone(), o.data))
                .collect();

            for (qid, owner) in &objs {
                let Some(obj) = self.state.get(qid) else {
                    continue;
                };
                // Check if there are any non-slot children (bare children
                // that should be wrapped in a default SlotContent).
                let bare_children: Vec<_> = obj
                    .children
                    .iter()
                    .filter(|c| {
                        self.state
                            .get(c)
                            .is_some_and(|o| matches!(o.kind, ObjectKind::Type(_)))
                    })
                    .cloned()
                    .collect();

                if bare_children.is_empty() {
                    continue;
                }

                // Check that a `::_` SlotContent doesn't already exist.
                let has_default_slot = obj.children.iter().any(|c| {
                    self.state
                        .get(c)
                        .is_some_and(|o| matches!(o.kind, ObjectKind::SlotContent))
                        && c.local_id().ends_with("::_")
                });
                if has_default_slot {
                    continue;
                }

                // Create synthetic SlotContent node for `::_`.
                let slot_qid = QualifiedId::new(qid.client(), &format!("{}::_", qid.local_id()));
                self.state
                    .upsert(ObjectKind::SlotContent, &slot_qid, &IndexMap::new(), *owner);
                self.state.set_parent(&slot_qid, qid);

                // Reparent bare children under the SlotContent node.
                for child_qid in &bare_children {
                    self.state.set_parent(child_qid, &slot_qid);
                }
            }
        }

        // Replay reduced state for all objects of this type.
        let objects: Vec<_> = self
            .state
            .objects_of_type(target_type)
            .into_iter()
            .map(|o| (o.id.clone(), o.kind.clone()))
            .collect();

        if !objects.is_empty() {
            // Synthesize replay commands from the state tree. These include
            // the original app objects with their slot children, so that the
            // batch rewriter can perform slot substitution during expansion.
            let mut replay_cmds = Vec::new();
            for (qid, _kind) in &objects {
                if let Some(upsert) = crate::state::reduced_upsert(&self.state, qid) {
                    replay_cmds.push(upsert);

                    // Include any children (with slot nodes) so the rewriter
                    // can reconstruct slot contents for substitution.
                    if self.state.has_children(qid) {
                        replay_cmds.push(Command::Push { slot: None });
                        replay_cmds.extend(crate::state::children_commands(&self.state, qid));
                        replay_cmds.push(Command::Pop);
                    }
                }
            }

            let mut batch = PendingBatch::new(from, client.to_owned(), replay_cmds);
            batch.is_replay = true;

            for (qid, _kind) in &objects {
                let expand_seq = self.next_expand_seq(from);
                if let Some((_kind_str, props)) = crate::state::expand_props(&self.state, qid) {
                    let qid_str = qid.to_string();
                    let expand_cmds = byo_vec! {
                        ?expand {expand_seq} {qid_str} {..props}
                    };
                    batch.add_pending_expand(from, expand_seq, qid.clone(), 0);
                    self.send_to(from, WriteMsg::Byo(Arc::new(expand_cmds)));
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

    /// Register a handler for a (type, request) pair.
    fn register_handler(
        &mut self,
        from: ProcessId,
        client: &str,
        type_name: &str,
        request_kind: &str,
    ) {
        if RequestKind::is_reserved(request_kind) {
            tracing::warn!(
                "{client} #handle: rejecting reserved request kind '{request_kind}' — use #claim instead"
            );
            return;
        }
        tracing::info!("{client} registers handler for {type_name}?{request_kind}");
        self.handlers
            .insert((type_name.to_owned(), request_kind.to_owned()), from);
    }

    /// Unregister a handler for a (type, request) pair.
    fn unregister_handler(
        &mut self,
        from: ProcessId,
        client: &str,
        type_name: &str,
        request_kind: &str,
    ) {
        if self.lookup_handler(type_name, request_kind) == Some(from) {
            tracing::info!("{client} unregisters handler for {type_name}?{request_kind}");
            self.handlers
                .remove(&(type_name.to_owned(), request_kind.to_owned()));
        }
    }

    /// Look up a handler by borrowed key, avoiding allocation for HashMap lookup.
    fn lookup_handler(&self, type_name: &str, request_kind: &str) -> Option<ProcessId> {
        // Linear scan avoids allocating owned Strings for HashMap::get.
        // The handlers map is expected to be small (tens of entries).
        self.handlers
            .iter()
            .find(|((t, r), _)| t == type_name && r == request_kind)
            .map(|(_, &pid)| pid)
    }

    /// Register a tap for a (type, event) pair. Plural — multiple processes can tap.
    fn register_tap(&mut self, from: ProcessId, client: &str, type_name: &str, event_kind: &str) {
        tracing::info!("{client} taps {type_name}!{event_kind}");
        let entry = self
            .taps
            .entry((type_name.to_owned(), event_kind.to_owned()))
            .or_default();
        if !entry.contains(&from) {
            entry.push(from);
        }
    }

    /// Unregister a tap for a (type, event) pair.
    fn unregister_tap(&mut self, from: ProcessId, client: &str, type_name: &str, event_kind: &str) {
        tracing::info!("{client} untaps {type_name}!{event_kind}");
        // Linear scan avoids allocating owned Strings (consistent with lookup_taps).
        if let Some((_, entry)) = self
            .taps
            .iter_mut()
            .find(|((t, e), _)| t == type_name && e == event_kind)
        {
            entry.retain(|&pid| pid != from);
        }
        self.taps.retain(|_, v| !v.is_empty());
    }

    /// Look up all tappers for a (type_name, event_kind) pair.
    fn lookup_taps(&self, type_name: &str, event_kind: &str) -> &[ProcessId] {
        // Linear scan avoids allocating owned Strings for HashMap::get.
        // The taps map is expected to be small.
        self.taps
            .iter()
            .find(|((t, e), _)| t == type_name && e == event_kind)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    /// BFS through expansion children to find a handler for the given request kind.
    ///
    /// Walks the expansion subtree of `qid` breadth-first, checking if any
    /// child's type has an explicit handler registered for `request_kind`.
    fn resolve_handler(&self, qid: &QualifiedId, request_kind: &str) -> Option<ProcessId> {
        use std::collections::VecDeque;

        let obj = self.state.get(qid)?;
        let mut queue: VecDeque<&QualifiedId> = obj.children.iter().collect();
        let mut visited: HashSet<&QualifiedId> = HashSet::new();

        while let Some(child_qid) = queue.pop_front() {
            if !visited.insert(child_qid) {
                continue;
            }

            let Some(child_obj) = self.state.get(child_qid) else {
                continue;
            };

            let child_type = match &child_obj.kind {
                ObjectKind::Type(t) => t.as_str(),
                ObjectKind::SlotContent | ObjectKind::Slot => {
                    // Follow slot_ref links for slots.
                    if let Some(ref slot_ref) = child_obj.slot_ref
                        && let Some(linked) = self.state.get(slot_ref)
                    {
                        queue.extend(linked.children.iter());
                    }
                    // Also traverse direct children of slot nodes.
                    queue.extend(child_obj.children.iter());
                    continue;
                }
            };

            if let Some(pid) = self.lookup_handler(child_type, request_kind) {
                return Some(pid);
            }

            queue.extend(child_obj.children.iter());
        }

        None
    }

    /// Resolve the target process for a custom request using 3-tier strategy.
    ///
    /// Returns `None` if no suitable handler is found or all candidates are
    /// the sender (to prevent routing back to self).
    fn resolve_request_handler(
        &self,
        from: ProcessId,
        obj_type: &str,
        owner_pid: ProcessId,
        qid: &QualifiedId,
        request_kind: &str,
    ) -> Option<ProcessId> {
        // Tier 1: Explicit handler for (obj_type, request_kind).
        if let Some(pid) = self.lookup_handler(obj_type, request_kind)
            && pid != from
        {
            return Some(pid);
        }

        // Tier 2: Owner of the target object.
        if owner_pid != from {
            return Some(owner_pid);
        }

        // Tier 3: BFS fallback through expansion children.
        self.resolve_handler(qid, request_kind)
            .filter(|&pid| pid != from)
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
    ///
    /// Each pending entry carries the qualified target that was computed at
    /// buffering time, so redirect frames are injected correctly even when
    /// the app has changed targets multiple times before any observer existed.
    fn flush_pending_passthrough(&mut self) {
        if self.pending_passthrough.is_empty() {
            return;
        }
        let Some(observers) = self.observers.get("tty") else {
            return;
        };
        let observers: Vec<ProcessId> = observers.clone();
        let pending = std::mem::take(&mut self.pending_passthrough);
        for (from, qualified_target, raw) in pending {
            if qualified_target.is_empty() {
                continue; // discard target
            }
            if qualified_target != self.last_forwarded_target {
                let qt = qualified_target.as_str();
                let redirect_cmds = if qualified_target == "/" {
                    byo_vec! { #unredirect }
                } else {
                    byo_vec! { #redirect {qt} }
                };
                self.last_forwarded_target.clone_from(&qualified_target);
                let redirect_arc = Arc::new(redirect_cmds);
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

        // Look up the owner and type of this object in the state tree.
        let (target_pid, obj_type) = match self.state.get(&qid) {
            Some(obj) => (obj.data, obj.kind.as_type().map(|s| s.to_owned())),
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

        // Build event command with remapped seq and dequalified target.
        self.send_to(
            target_pid,
            WriteMsg::Byo(Arc::new(byo_vec! {
                !{event_type} {remapped_seq} {dequalified_target} {..props}
            })),
        );

        // Store PendingAck for the return trip.
        self.pending_acks.insert(
            (target_pid, event_type.to_owned(), remapped_seq),
            PendingAck {
                sender: from,
                sender_seq: seq,
                forwarded_at: Instant::now(),
            },
        );

        // --- Tap fan-out: send read-only copies to tapping processes ---
        if let Some(ref obj_type) = obj_type {
            let tappers: Vec<ProcessId> = self
                .lookup_taps(obj_type, event_type)
                .iter()
                .copied()
                .filter(|&pid| pid != from && pid != target_pid)
                .collect();
            if !tappers.is_empty() {
                let tap_msg = WriteMsg::Byo(Arc::new(byo_vec! {
                    !{event_type} {seq} {&qualified_id} tap {..props}
                }));
                for tapper_pid in tappers {
                    self.send_to(tapper_pid, tap_msg.clone());
                }
            }
        }
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

        // Build ACK command with the original sender's seq.
        let ack_seq = pending.sender_seq;
        self.send_to(
            pending.sender,
            WriteMsg::Byo(Arc::new(byo_vec! {
                !ack {event_type} {ack_seq} {..props}
            })),
        );
    }

    /// Route a custom request using 3-tier strategy:
    /// 1. Explicit handler: `handlers[(obj_type, request_kind)]`
    /// 2. Owner: the process that created the target object
    /// 3. BFS fallback: walk expansion children for a handler match
    ///
    /// Resolve the handler for a dot-command (request or message) targeting an object.
    ///
    /// Qualifies the target ID, looks up the object in the state tree,
    /// and uses the 3-tier handler resolution to find the destination process.
    /// Returns `(target_pid, dest_client, dequalified_target)` on success.
    fn resolve_dot_target(
        &self,
        from: ProcessId,
        client: &str,
        kind: &str,
        target: &str,
    ) -> Option<(ProcessId, String, String)> {
        use crate::id::{dequalify, qualify};

        let qualified_id = qualify(client, target);
        let qid = QualifiedId::parse(&qualified_id)?;

        let obj = self.state.get(&qid)?;

        let obj_type = match &obj.kind {
            ObjectKind::Type(t) => t.as_str(),
            _ => return None,
        };
        let owner_pid = obj.data;

        let target_pid = self.resolve_request_handler(from, obj_type, owner_pid, &qid, kind)?;

        let dest_client = self.client_name(target_pid)?.to_owned();
        let dequalified_target = dequalify(&dest_client, &qualified_id).to_owned();

        Some((target_pid, dest_client, dequalified_target))
    }

    async fn handle_request(
        &mut self,
        from: ProcessId,
        client: &str,
        kind_name: &str,
        seq: u64,
        target: &str,
        props: &[Prop],
    ) {
        let Some((target_pid, _, dequalified_target)) =
            self.resolve_dot_target(from, client, kind_name, target)
        else {
            return;
        };

        let remapped_seq = self.next_request_seq(target_pid, kind_name);

        self.send_to(
            target_pid,
            WriteMsg::Byo(Arc::new(byo_vec! {
                ?{kind_name} {remapped_seq} {&dequalified_target} {..props}
            })),
        );

        self.pending_responses.insert(
            (target_pid, kind_name.to_owned(), remapped_seq),
            PendingResponse {
                sender: from,
                sender_seq: seq,
                forwarded_at: Instant::now(),
            },
        );
    }

    /// Route a standalone message to the appropriate handler.
    ///
    /// Like request routing but fire-and-forget: no seq remapping, no
    /// pending response tracking. Uses the same 3-tier handler resolution.
    async fn handle_message(
        &mut self,
        from: ProcessId,
        client: &str,
        kind: &MessageKind,
        target: &str,
        props: &[Prop],
        body: &Option<Vec<Command>>,
    ) {
        let Some((target_pid, _, dequalified_target)) =
            self.resolve_dot_target(from, client, kind.as_str(), target)
        else {
            return;
        };

        self.send_to(
            target_pid,
            WriteMsg::Byo(Arc::new(vec![Command::Message {
                kind: kind.clone(),
                target: dequalified_target.into(),
                props: props.to_vec(),
                body: body.clone(),
            }])),
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

        let response_cmds = vec![Command::Response {
            kind: ResponseKind::Other(ByteStr::from(kind_name)),
            seq: pending.sender_seq,
            props: props.to_vec(),
            body: body.clone(),
        }];

        self.send_to(pending.sender, WriteMsg::Byo(Arc::new(response_cmds)));
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
        let replay_cmds = crate::state::project_tree(&self.state, types);
        if !replay_cmds.is_empty() {
            self.send_to(pid, WriteMsg::Byo(Arc::new(replay_cmds)));
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
        use crate::id::qualify;

        let mut nested_expands = Vec::new();

        for cmd in body_cmds {
            if let Command::Upsert { kind, id, props } = cmd
                && *id != "_"
                && let Some(&subscriber) = self.claims.get(&**kind)
            {
                let qid = QualifiedId::new(daemon_client, id);
                let expand_seq = self.next_expand_seq(subscriber);
                let qualified = qualify(daemon_client, id);
                let kind_val = &**kind;
                let expand_cmds = byo_vec! {
                    ?expand {expand_seq} {qualified} kind={kind_val} {..props}
                };

                nested_expands.push((subscriber, qid, expand_seq, expand_cmds));
            }
        }

        if !nested_expands.is_empty() {
            // Find the batch that owns this expansion.
            let batch_idx = self
                .pending_batches
                .iter()
                .position(|b| b.has_pending_expand(from, seq));

            if let Some(idx) = batch_idx {
                for (subscriber, qid, expand_seq, expand_cmds) in nested_expands {
                    tracing::trace!(
                        "nested expansion: {qid} (seq={expand_seq}, depth={})",
                        current_depth + 1
                    );
                    self.pending_batches[idx].add_pending_expand(
                        subscriber,
                        expand_seq,
                        qid,
                        current_depth + 1,
                    );
                    self.send_to(subscriber, WriteMsg::Byo(Arc::new(expand_cmds)));
                }
            } else {
                tracing::warn!("nested expansion: batch not found for ({from:?}, seq={seq})");
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
        // Sort expansions by depth so parent expansions are processed before
        // nested child expansions. This ensures parent nodes exist in the state
        // tree before set_parent is called for child expansion objects.
        let mut sorted: Vec<_> = batch.expansions.iter().collect();
        sorted.sort_by_key(|(_, (_, _, _, depth))| *depth);

        for (source_qid_str, (daemon_pid, expansion_cmds, _is_re_expand, _depth)) in sorted {
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
                        self.state
                            .upsert(kind.as_ref().into(), &qid, &map, *daemon_pid);

                        // Parent: root-level expansion objects → source object,
                        // nested → their push parent within the expansion.
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&qid, parent);
                        } else {
                            self.state.set_parent(&qid, &source_qid);
                        }

                        last_qid = Some(qid);
                    }
                    Command::Push { slot } => {
                        if let Some(name) = slot {
                            // Expansion-side Slot node in the state tree.
                            // QID format: client:parent_local::name.
                            let (parent_client, parent_local) = last_qid
                                .as_ref()
                                .map(|q| (q.client().to_owned(), q.local_id().to_owned()))
                                .unwrap_or_default();
                            let slot_local = format!("{parent_local}::{name}");
                            let slot_qid = QualifiedId::new(&parent_client, &slot_local);
                            self.state.upsert(
                                ObjectKind::Slot,
                                &slot_qid,
                                &IndexMap::new(),
                                *daemon_pid,
                            );
                            if let Some(parent) = parent_stack.last() {
                                self.state.set_parent(&slot_qid, parent);
                            } else {
                                self.state.set_parent(&slot_qid, &source_qid);
                            }
                            parent_stack.push(slot_qid.clone());
                            last_qid = Some(slot_qid);
                        } else if let Some(ref qid) = last_qid {
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
    ) -> Vec<Command> {
        // Collect old expansion objects (all descendants of the source object).
        let descendants = self.state.descendants_info(source_qid);
        let old_objects: HashMap<QualifiedId, IndexMap<String, PropValue>> = descendants
            .iter()
            .filter_map(|(qid, _, _)| self.state.get(qid).map(|o| (qid.clone(), o.props.clone())))
            .collect();

        let old_kinds: HashMap<QualifiedId, ObjectKind> = descendants
            .iter()
            .map(|(qid, kind, _)| (qid.clone(), kind.clone()))
            .collect();

        let old_parents: HashMap<QualifiedId, Option<QualifiedId>> = descendants
            .iter()
            .filter_map(|(qid, _, _)| self.state.get(qid).map(|o| (qid.clone(), o.parent.clone())))
            .collect();

        let mut delta: Vec<Command> = Vec::new();
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

                    // If this object is a claimed type with a separate expansion,
                    // protect all its state-tree descendants from being destroyed
                    // by this reconciliation — they belong to a nested expansion.
                    if self.claims.contains_key(kind.as_ref()) {
                        for (desc_qid, _, _) in self.state.descendants_info(&qid) {
                            seen_qids.insert(desc_qid);
                        }
                    }

                    let new_props = props_to_map(props);
                    let mut slot_inlined = false;

                    if let Some(old_props) = old_objects.get(&qid) {
                        // Determine the new parent for this object.
                        let new_parent = parent_stack
                            .last()
                            .cloned()
                            .unwrap_or_else(|| source_qid.clone());
                        let old_parent = old_parents.get(&qid).and_then(|p| p.as_ref());
                        let raw_parent_changed = old_parent != Some(&new_parent);

                        // Follow slot_ref links when computing parent change.
                        // If the old parent is a SlotContent node linked to a Slot
                        // whose parent matches the new parent, the object didn't
                        // actually move — the difference is just an artifact of slot
                        // inlining in effective_cmds vs. the SlotContent parent in
                        // the state tree. We must also preserve the state tree parent
                        // (keep it under SlotContent) so that collect_prior_slots
                        // can find it on subsequent re-expansions.
                        if raw_parent_changed
                            && let Some(old_p) = old_parent
                            && let Some(old_parent_obj) = self.state.get(old_p)
                            && old_parent_obj.kind == ObjectKind::SlotContent
                            && let Some(slot_qid) = old_parent_obj.slot_ref.as_ref()
                            && let Some(slot_obj) = self.state.get(slot_qid)
                            && slot_obj.kind == ObjectKind::Slot
                            && slot_obj.parent.as_ref() == Some(&new_parent)
                        {
                            slot_inlined = true;
                        }

                        let parent_changed = raw_parent_changed && !slot_inlined;

                        if parent_changed {
                            // Parent changed — emit destroy+create (compositor
                            // can't reparent via patch).
                            delta.push(Command::Destroy {
                                kind: kind.clone(),
                                id: ByteStr::from(qid.to_string()),
                            });
                            delta.push(Command::Upsert {
                                kind: kind.clone(),
                                id: ByteStr::from(qid.to_string()),
                                props: props.clone(),
                            });
                        } else {
                            // Same parent — compute prop delta.
                            let mut delta_props: Vec<Prop> = Vec::new();

                            for (key, new_val) in &new_props {
                                if old_props.get(key) != Some(new_val) {
                                    match new_val {
                                        PropValue::Str(s) => {
                                            delta_props.push(Prop::val(key.as_str(), s.as_str()));
                                        }
                                        PropValue::Flag => {
                                            delta_props.push(Prop::flag(key.as_str()));
                                        }
                                    }
                                }
                            }

                            for key in old_props.keys() {
                                if !new_props.contains_key(key) {
                                    delta_props.push(Prop::remove(key.as_str()));
                                }
                            }

                            if !delta_props.is_empty() {
                                delta.push(Command::Patch {
                                    kind: kind.clone(),
                                    id: ByteStr::from(qid.to_string()),
                                    props: delta_props,
                                });
                            }
                        }
                    } else {
                        // New object — emit create.
                        delta.push(Command::Upsert {
                            kind: kind.clone(),
                            id: ByteStr::from(qid.to_string()),
                            props: props.clone(),
                        });
                    }

                    // Update state tree.
                    self.state
                        .upsert(kind.as_ref().into(), &qid, &new_props, daemon_pid);
                    // When parent difference is due to slot inlining, keep the
                    // state tree parent under the SlotContent node so that
                    // collect_prior_slots can find it on subsequent re-expansions.
                    if !slot_inlined {
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&qid, parent);
                        } else {
                            self.state.set_parent(&qid, source_qid);
                        }
                    }

                    last_qid = Some(qid);
                }
                Command::Push { slot } => {
                    if let Some(name) = slot {
                        // Expansion-side Slot node in state tree.
                        // QID format: client:parent_local::name.
                        let (parent_client, parent_local) = last_qid
                            .as_ref()
                            .map(|q| (q.client().to_owned(), q.local_id().to_owned()))
                            .unwrap_or_default();
                        let slot_local = format!("{parent_local}::{name}");
                        let slot_qid = QualifiedId::new(&parent_client, &slot_local);
                        seen_qids.insert(slot_qid.clone());
                        self.state.upsert(
                            ObjectKind::Slot,
                            &slot_qid,
                            &IndexMap::new(),
                            daemon_pid,
                        );
                        if let Some(parent) = parent_stack.last() {
                            self.state.set_parent(&slot_qid, parent);
                        } else {
                            self.state.set_parent(&slot_qid, source_qid);
                        }
                        parent_stack.push(slot_qid.clone());
                        last_qid = Some(slot_qid);
                    } else if let Some(ref qid) = last_qid {
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

        // Preserve SlotContent and Slot nodes — these are not present in
        // effective_cmds (they get inlined by slot substitution) but must
        // survive reconciliation so that future re-expansions and
        // link_slot_refs can find them.
        for (qid, kind) in &old_kinds {
            if matches!(kind, ObjectKind::SlotContent | ObjectKind::Slot) {
                seen_qids.insert(qid.clone());
            }
        }

        // Emit destroys for objects that are in old but not in new.
        for qid in old_objects.keys() {
            if !seen_qids.contains(qid) {
                if let Some(kind) = old_kinds.get(qid) {
                    delta.push(Command::Destroy {
                        kind: ByteStr::from(kind.to_string()),
                        id: ByteStr::from(qid.to_string()),
                    });
                }
                self.state.destroy(qid);
            }
        }

        delta
    }

    /// Link `SlotContent` ↔ `Slot` cross-references for a source object.
    ///
    /// After expansion state is written, this finds `SlotContent` children
    /// (app-side) and `Slot` descendants (expansion-side) of the source
    /// object and sets `slot_ref` on each to point to its counterpart.
    /// Matching is by bare slot name (the part after `::` in the local ID).
    fn link_slot_refs(&mut self, source_qid: &QualifiedId) {
        let descendants = self.state.descendants_info(source_qid);

        // Collect SlotContent and Slot QIDs, extracting slot names.
        let mut content_by_name: HashMap<&str, QualifiedId> = HashMap::new();
        let mut slot_by_name: HashMap<&str, QualifiedId> = HashMap::new();

        // We need stable references to the QIDs, so collect into a vec first.
        let slot_qids: Vec<(QualifiedId, ObjectKind)> = descendants
            .iter()
            .filter(|(_, kind, _)| matches!(kind, ObjectKind::SlotContent | ObjectKind::Slot))
            .map(|(qid, kind, _)| (qid.clone(), kind.clone()))
            .collect();

        for (qid, kind) in &slot_qids {
            let local = qid.local_id();
            let Some(sep_pos) = local.rfind("::") else {
                continue;
            };
            let name = &local[sep_pos + 2..];
            match kind {
                ObjectKind::SlotContent => {
                    content_by_name.insert(name, qid.clone());
                }
                ObjectKind::Slot => {
                    slot_by_name.insert(name, qid.clone());
                }
                _ => {}
            }
        }

        // Set bidirectional slot_ref for matching pairs.
        for (name, content_qid) in &content_by_name {
            if let Some(slot_qid) = slot_by_name.get(name) {
                self.state.set_slot_ref(content_qid, slot_qid);
                self.state.set_slot_ref(slot_qid, content_qid);
            }
        }
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
        rewritten: &mut Vec<Command>,
    ) {
        for source_qid in claimed_destroys {
            // Collect info about all descendants before destroying.
            let all_descendants = self.state.descendants_info(source_qid);

            // Emit destroy commands for direct children only — compositor
            // cascades the rest via its own tree semantics.
            if let Some(source) = self.state.get(source_qid) {
                for child_qid in source.children.clone() {
                    if let Some(child) = self.state.get(&child_qid) {
                        rewritten.push(Command::Destroy {
                            kind: ByteStr::from(child.kind.to_string()),
                            id: ByteStr::from(child_qid.to_string()),
                        });
                    }
                }
            }

            // Group all destroyed descendants by owner daemon and notify.
            let mut daemon_notifications: HashMap<ProcessId, Vec<Command>> = HashMap::new();
            for (qid, kind, owner) in &all_descendants {
                let cmds = daemon_notifications.entry(*owner).or_default();
                cmds.push(Command::Destroy {
                    kind: ByteStr::from(kind.to_string()),
                    id: ByteStr::from(qid.to_string()),
                });
            }

            for (daemon_pid, notification_cmds) in daemon_notifications {
                self.send_to(daemon_pid, WriteMsg::Byo(Arc::new(notification_cmds)));
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
            let is_replay = batch.is_replay;

            // Debug: log expansion keys before rewrite
            tracing::trace!(
                "batch ready — {} expansions: [{}]",
                batch.expansions.len(),
                batch
                    .expansions
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            // Reconcile re-expansions using the per-expansion flag.
            // For re-expansions, substitute slot contents (batch + prior state)
            // into the daemon's expansion before reconciling.
            let mut reconciliation_cmds: Vec<Command> = Vec::new();
            for (source_qid_str, (daemon_pid, expansion_cmds, is_re_expand, _depth)) in
                &batch.expansions
            {
                if *is_re_expand && let Some(source_qid) = QualifiedId::parse(source_qid_str) {
                    // Collect slot contents from the batch's @ children
                    let batch_slots = batch.collect_patch_slots(source_qid_str);
                    // Collect prior slot contents from the state tree
                    let prior_slots = crate::state::collect_prior_slots(&self.state, &source_qid);
                    // Merge: batch overrides prior, unmentioned fall back to prior
                    let merged = crate::batch::merge_slots(batch_slots, Some(prior_slots));
                    // Substitute slots into the expansion
                    let effective_cmds = if merged.is_empty() {
                        expansion_cmds.clone()
                    } else {
                        crate::batch::substitute_slots(expansion_cmds, &merged)
                    };
                    let delta = self.reconcile_expansion(&source_qid, *daemon_pid, &effective_cmds);
                    reconciliation_cmds.extend(delta);
                }
            }

            let (mut rewritten, claimed_destroys) = batch.rewrite(&self.claims);

            // Handle cascade destroys BEFORE updating expansion state so that
            // old expansion children are cleaned up before new ones are added.
            self.handle_cascade_destroys(&claimed_destroys, &mut rewritten)
                .await;

            // Update state tree with expansion nodes (for initial expansions).
            self.update_expansion_state(&batch);

            // Link SlotContent ↔ Slot cross-references for all expansions.
            for source_qid_str in batch.expansions.keys() {
                if let Some(source_qid) = QualifiedId::parse(source_qid_str) {
                    self.link_slot_refs(&source_qid);
                }
            }

            // Append reconciliation output.
            rewritten.extend(reconciliation_cmds);

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
            // Find the QID, is_re_expand flag, and depth associated with this (from, seq) pair.
            let expand_info = batch
                .pending_expands
                .iter()
                .find(|e| e.subscriber == from && e.seq == seq)
                .map(|e| (e.qid.clone(), e.is_re_expand, e.depth));

            if let Some((ref qid, is_re_expand, depth)) = expand_info {
                tracing::trace!(
                    "recording expansion for {qid} (re_expand={is_re_expand}, depth={depth})"
                );
                batch.record_expansion(qid, from, commands, is_re_expand, depth);
            }
        }
    }

    /// Forward rewritten commands to all observers, filtered per observer's
    /// type subscriptions.
    async fn forward_to_observers(&self, commands: &[Command], _from: ProcessId) {
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

        for target in targets {
            let projected = if let Some(types) = self.observer_types.get(&target) {
                project_commands(commands, types, &self.state)
            } else {
                // No type filter — send everything.
                commands.to_vec()
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
) -> Vec<Command> {
    let mut out = Vec::new();
    let mut i = 0;
    project_at_level(commands, observed_types, state, &mut i, &mut out, false);
    out
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
    out: &mut Vec<Command>,
    in_observed_context: bool,
) {
    while *i < commands.len() {
        match &commands[*i] {
            Command::Upsert { kind, id, props } => {
                let is_observed = observed_types.contains(&**kind);
                *i += 1;

                let has_children =
                    *i < commands.len() && matches!(commands[*i], Command::Push { .. });

                if has_children {
                    *i += 1; // consume Push

                    if is_observed {
                        let child_cmds = collect_children(commands, observed_types, state, i);
                        out.push(Command::Upsert {
                            kind: kind.clone(),
                            id: id.clone(),
                            props: props.clone(),
                        });
                        if !child_cmds.is_empty() {
                            out.push(Command::Push { slot: None });
                            out.extend(child_cmds);
                            out.push(Command::Pop);
                        }
                    } else if in_observed_context {
                        // Inside an observed parent — flatten children here.
                        project_at_level(commands, observed_types, state, i, out, true);
                    } else {
                        // Top level, non-observed object — wrap children under
                        // nearest observed ancestor from the state tree.
                        project_under_ancestor(commands, observed_types, state, i, out, id);
                    }
                } else if is_observed {
                    out.push(Command::Upsert {
                        kind: kind.clone(),
                        id: id.clone(),
                        props: props.clone(),
                    });
                }
            }
            Command::Destroy { kind, id } => {
                if observed_types.contains(&**kind) {
                    out.push(Command::Destroy {
                        kind: kind.clone(),
                        id: id.clone(),
                    });
                } else {
                    // Non-observed type destroyed — emit destroys for any
                    // observed descendants so the observer can clean up.
                    emit_observed_descendant_destroys(state, id, observed_types, out);
                }
                *i += 1;
            }
            Command::Patch { kind, id, props } => {
                let is_observed = observed_types.contains(&**kind);
                *i += 1;

                let has_children =
                    *i < commands.len() && matches!(commands[*i], Command::Push { .. });

                if has_children {
                    *i += 1; // consume Push

                    if is_observed {
                        let child_cmds = collect_children(commands, observed_types, state, i);
                        out.push(Command::Patch {
                            kind: kind.clone(),
                            id: id.clone(),
                            props: props.clone(),
                        });
                        if !child_cmds.is_empty() {
                            out.push(Command::Push { slot: None });
                            out.extend(child_cmds);
                            out.push(Command::Pop);
                        }
                    } else if in_observed_context {
                        project_at_level(commands, observed_types, state, i, out, true);
                    } else {
                        project_under_ancestor(commands, observed_types, state, i, out, id);
                    }
                } else if is_observed {
                    out.push(Command::Patch {
                        kind: kind.clone(),
                        id: id.clone(),
                        props: props.clone(),
                    });
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

/// Recurse into children, returning the projected commands (empty if nothing observed).
fn collect_children(
    commands: &[Command],
    observed_types: &HashSet<String>,
    state: &ObjectTree,
    i: &mut usize,
) -> Vec<Command> {
    let mut child_cmds = Vec::new();
    project_at_level(commands, observed_types, state, i, &mut child_cmds, true);
    child_cmds
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
    out: &mut Vec<Command>,
    id: &str,
) {
    // Look up the nearest observed ancestor in the state tree.
    let ancestor = QualifiedId::parse(id)
        .and_then(|qid| crate::state::nearest_observed_ancestor(state, &qid, observed_types));

    if let Some(ref ancestor_qid) = ancestor
        && let Some(ancestor_obj) = state.get(ancestor_qid)
    {
        // Wrap children under `@ancestorKind ancestorQid { ... }`.
        let child_cmds = collect_children(commands, observed_types, state, i);

        if !child_cmds.is_empty() {
            let ancestor_kind = ancestor_obj.kind.as_type().unwrap_or("_slot");
            out.push(Command::Patch {
                kind: ByteStr::from(ancestor_kind),
                id: ByteStr::from(ancestor_qid.to_string()),
                props: vec![],
            });
            out.push(Command::Push { slot: None });
            out.extend(child_cmds);
            out.push(Command::Pop);
        }
    } else {
        // No observed ancestor — emit children at top level.
        project_at_level(commands, observed_types, state, i, out, false);
    }
}

/// Emit `-kind qid` for each observed descendant of a non-observed destroyed
/// object, deepest-first so the observer can cascade correctly.
fn emit_observed_descendant_destroys(
    state: &ObjectTree,
    id: &str,
    observed_types: &HashSet<String>,
    out: &mut Vec<Command>,
) {
    let Some(qid) = QualifiedId::parse(id) else {
        return;
    };
    let descendants = state.descendants_info(&qid);
    // descendants_info returns pre-order (parent before child).
    // Reverse to get deepest-first for destroy ordering.
    for (desc_qid, kind, _owner) in descendants.iter().rev() {
        if let Some(kind_str) = kind.as_type()
            && observed_types.contains(kind_str)
        {
            out.push(Command::Destroy {
                kind: ByteStr::from(kind_str),
                id: ByteStr::from(desc_qid.to_string()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byo::assert::assert_eq_bytes;
    use byo::protocol::Prop;

    fn cmds_to_bytes(cmds: &[Command]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut em = byo::emitter::Emitter::new(&mut buf);
        let _ = em.commands(cmds);
        buf
    }

    fn parse(input: &str) -> Vec<Command> {
        byo::parser::parse(input).unwrap()
    }

    #[test]
    fn project_all_types_observed() {
        let state = ObjectTree::new();
        let commands = parse("+view app:root { +text app:label content=Hello }");
        let types: HashSet<String> = ["view", "text"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            r#"+view app:root { +text app:label content=Hello }"#,
        );
    }

    #[test]
    fn project_skip_unobserved_leaf() {
        let state = ObjectTree::new();
        let commands =
            parse("+view app:root { +text app:label content=Hello +image app:bg src=bg.png }");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&cmds_to_bytes(&result), "+view app:root");
    }

    #[test]
    fn project_flatten_children() {
        let state = ObjectTree::new();
        // root (view) → container (panel, not observed) → child (view, observed)
        let commands =
            parse("+view app:root { +panel app:container { +view app:child class=inner } }");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            "+view app:root { +view app:child class=inner }",
        );
    }

    #[test]
    fn project_destroy_observed() {
        let state = ObjectTree::new();
        let commands = parse("-view app:sidebar -text app:label");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&cmds_to_bytes(&result), "-view app:sidebar");
    }

    #[test]
    fn project_patch_observed() {
        let state = ObjectTree::new();
        let commands = parse("@view app:sidebar hidden @text app:label content=New");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&cmds_to_bytes(&result), "@view app:sidebar hidden");
    }

    #[test]
    fn project_empty_filter() {
        let state = ObjectTree::new();
        let commands = parse("+view app:root { +text app:label content=Hello }");
        let types: HashSet<String> = HashSet::new();
        let result = project_commands(&commands, &types, &state);
        assert!(result.is_empty(), "expected empty, got: {result:?}");
    }

    #[test]
    fn project_deeply_nested_flatten() {
        let state = ObjectTree::new();
        // a (view) → b (panel) → c (panel) → d (view)
        // With only view observed, d should be re-parented under a.
        let commands = parse("+view a { +panel b { +panel c { +view d } } }");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&cmds_to_bytes(&result), "+view a { +view d }");
    }

    #[test]
    fn project_patch_with_children() {
        let state = ObjectTree::new();
        let commands = parse("@view app:root { +text app:label content=Hello +view app:child }");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            "@view app:root { +view app:child }",
        );
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
        state.upsert("view".into(), &root_qid, &IndexMap::new(), ProcessId(1));
        state.upsert(
            "panel".into(),
            &container_qid,
            &IndexMap::new(),
            ProcessId(1),
        );
        state.set_parent(&container_qid, &root_qid);

        let commands = parse("@panel app:container { +view app:child class=inner }");
        let types: HashSet<String> = ["view"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(
            &cmds_to_bytes(&result),
            "@view app:root { +view app:child class=inner }",
        );
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
        state.upsert("panel".into(), &container, &IndexMap::new(), ProcessId(1));
        state.upsert("view".into(), &child, &IndexMap::new(), ProcessId(1));
        state.upsert("text".into(), &label, &IndexMap::new(), ProcessId(1));
        state.set_parent(&child, &container);
        state.set_parent(&label, &child);

        let commands = parse("-panel app:container");
        let types: HashSet<String> = ["view", "text"].iter().map(|s| s.to_string()).collect();
        let result = project_commands(&commands, &types, &state);
        assert_eq_bytes(&cmds_to_bytes(&result), "-text app:label -view app:child");
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

    // ── remap_img_vars tests ─────────────────────────────────────

    /// Helper that applies the same remapping logic as `remap_img_vars`
    /// on a single prop value string, given a local→global ID map.
    fn remap_value(value: &str, id_map: &HashMap<u32, u32>) -> String {
        let result = byo::vars::replace(value, |var| {
            if var.name != "img" {
                return None;
            }
            let entries = byo::vars::parse_img_args(var.args)?;
            let mut any_changed = false;
            let remapped: Vec<_> = entries
                .into_iter()
                .map(|mut entry| {
                    if let Some(&global_id) = id_map.get(&entry.id) {
                        entry.id = global_id;
                        any_changed = true;
                    }
                    entry
                })
                .collect();
            if any_changed {
                Some(format!("$img({})", byo::vars::format_img_args(&remapped)))
            } else {
                None
            }
        });
        result.into_owned()
    }

    #[test]
    fn remap_single_img() {
        let map = HashMap::from([(1, 100)]);
        assert_eq!(remap_value("$img(1)", &map), "$img(100)");
    }

    #[test]
    fn remap_multi_density_img() {
        let map = HashMap::from([(1, 100), (2, 200)]);
        assert_eq!(
            remap_value("$img(1 @1x, 2 @2x)", &map),
            "$img(100 @1x, 200 @2x)"
        );
    }

    #[test]
    fn remap_partial_match() {
        // Only image 1 is in the map, image 2 is not
        let map = HashMap::from([(1, 100)]);
        assert_eq!(
            remap_value("$img(1 @1x, 2 @2x)", &map),
            "$img(100 @1x, 2 @2x)"
        );
    }

    #[test]
    fn remap_no_match_unchanged() {
        let map = HashMap::from([(99, 999)]);
        // No mapping for image 1 — should stay unchanged
        assert_eq!(remap_value("$img(1)", &map), "$img(1)");
    }

    #[test]
    fn remap_non_img_var_unchanged() {
        let map = HashMap::from([(1, 100)]);
        assert_eq!(remap_value("$env(HOME)", &map), "$env(HOME)");
    }

    #[test]
    fn remap_multi_density_with_modifiers() {
        let map = HashMap::from([(1, 100), (2, 200)]);
        assert_eq!(
            remap_value("$img(1@2x dark, 2@1x light)", &map),
            "$img(100 @2x dark, 200 @1x light)"
        );
    }

    #[test]
    fn remap_embedded_in_prop_value() {
        let map = HashMap::from([(5, 500)]);
        assert_eq!(
            remap_value("url($img(5))/path", &map),
            "url($img(500))/path"
        );
    }

    #[test]
    fn register_handler() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_handler(pid, "compositor", "view", "measure");
        assert_eq!(
            router
                .handlers
                .get(&("view".to_owned(), "measure".to_owned())),
            Some(&pid)
        );
    }

    #[test]
    fn register_handler_rejects_expand() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_handler(pid, "compositor", "button", "expand");
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn unregister_handler() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_handler(pid, "compositor", "view", "measure");
        router.unregister_handler(pid, "compositor", "view", "measure");
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn unregister_handler_wrong_owner() {
        let mut router = Router::new();
        let pid1 = ProcessId(1);
        let pid2 = ProcessId(2);
        router.register_handler(pid1, "compositor", "view", "measure");
        router.unregister_handler(pid2, "other", "view", "measure");
        // Should not remove since pid2 doesn't own it.
        assert_eq!(
            router
                .handlers
                .get(&("view".to_owned(), "measure".to_owned())),
            Some(&pid1)
        );
    }

    #[test]
    fn resolve_handler_direct_match() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        let qid = QualifiedId::parse("app:root").unwrap();
        // Insert a view object with a child that has handler.
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            ProcessId(2),
        );
        let child_qid = QualifiedId::parse("controls:child").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &child_qid,
            &IndexMap::new(),
            pid,
        );
        router.state.set_parent(&child_qid, &qid);
        router
            .handlers
            .insert(("view".to_owned(), "measure".to_owned()), pid);

        assert_eq!(router.resolve_handler(&qid, "measure"), Some(pid));
    }

    #[test]
    fn resolve_handler_no_match() {
        let mut router = Router::new();
        let qid = QualifiedId::parse("app:root").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            ProcessId(1),
        );
        assert_eq!(router.resolve_handler(&qid, "measure"), None);
    }

    #[test]
    fn handler_cleanup_on_disconnect() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_handler(pid, "compositor", "view", "measure");
        router.register_handler(pid, "compositor", "text", "measure");
        // Simulate disconnect cleanup.
        router.handlers.retain(|_, &mut p| p != pid);
        assert!(router.handlers.is_empty());
    }

    // ── reconcile_expansion slot parent tests ───────────────────

    /// Build a state tree mimicking a scroll-view with app content in a slot:
    ///
    /// ```text
    /// app:sv                          ← app-owned scroll-view
    ///   app:sv::_                     ← SlotContent (default slot) [slot_ref → controls:vp::_]
    ///     app:child                   ← app content inside the slot
    ///
    /// controls:sv-root                ← daemon expansion root
    ///   controls:vp                   ← daemon viewport
    ///     controls:vp::_              ← Slot placeholder [slot_ref → app:sv::_]
    /// ```
    fn setup_scroll_view_state(router: &mut Router) {
        let app = ProcessId(1);
        let daemon = ProcessId(2);

        let sv = QualifiedId::parse("app:sv").unwrap();
        let sv_slot = QualifiedId::parse("app:sv::_").unwrap();
        let child = QualifiedId::parse("app:child").unwrap();
        let root = QualifiedId::parse("controls:sv-root").unwrap();
        let vp = QualifiedId::parse("controls:vp").unwrap();
        let vp_slot = QualifiedId::parse("controls:vp::_").unwrap();

        // App-side
        router.state.upsert(
            ObjectKind::Type("scroll-view".into()),
            &sv,
            &IndexMap::new(),
            app,
        );
        router
            .state
            .upsert(ObjectKind::SlotContent, &sv_slot, &IndexMap::new(), app);
        router.state.set_parent(&sv_slot, &sv);
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &child,
            &IndexMap::new(),
            app,
        );
        router.state.set_parent(&child, &sv_slot);

        // Expansion-side
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &root,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&root, &sv);
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &vp,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&vp, &root);
        router
            .state
            .upsert(ObjectKind::Slot, &vp_slot, &IndexMap::new(), daemon);
        router.state.set_parent(&vp_slot, &vp);

        // Bidirectional slot_ref links
        router.state.set_slot_ref(&sv_slot, &vp_slot);
        router.state.set_slot_ref(&vp_slot, &sv_slot);
    }

    #[test]
    fn reconcile_expansion_slot_parent_no_false_destroy() {
        // When reconciling a re-expansion with slot content inlined,
        // app content whose old parent is a SlotContent node should NOT
        // be treated as having changed parents — the SlotContent→Slot
        // link means it's logically in the same position.
        let mut router = Router::new();
        setup_scroll_view_state(&mut router);

        let sv = QualifiedId::parse("app:sv").unwrap();
        let daemon = ProcessId(2);

        // Effective commands after slot substitution: app:child is now
        // directly under controls:vp (the slot was inlined).
        let effective_cmds = parse(
            "+view controls:sv-root { \
               +view controls:vp { \
                 +view app:child \
               } \
             }",
        );

        let delta = router.reconcile_expansion(&sv, daemon, &effective_cmds);

        // The delta should NOT contain a Destroy for app:child.
        let has_destroy = delta
            .iter()
            .any(|cmd| matches!(cmd, Command::Destroy { id, .. } if id.as_ref() == "app:child"));
        assert!(
            !has_destroy,
            "reconcile_expansion should not destroy app:child when its parent \
             only changed due to slot inlining. Delta: {delta:?}"
        );
    }

    #[test]
    fn reconcile_expansion_slot_parent_preserves_slot_content_node() {
        // The SlotContent node (app:sv::_) should survive reconciliation
        // even though it doesn't appear in the effective_cmds (it's been
        // inlined). It must remain in seen_qids or otherwise be preserved.
        let mut router = Router::new();
        setup_scroll_view_state(&mut router);

        let sv = QualifiedId::parse("app:sv").unwrap();
        let sv_slot = QualifiedId::parse("app:sv::_").unwrap();
        let daemon = ProcessId(2);

        let effective_cmds = parse(
            "+view controls:sv-root { \
               +view controls:vp { \
                 +view app:child \
               } \
             }",
        );

        let _delta = router.reconcile_expansion(&sv, daemon, &effective_cmds);

        // The SlotContent node should still exist in the state tree.
        assert!(
            router.state.get(&sv_slot).is_some(),
            "SlotContent node app:sv::_ should not be destroyed during reconciliation"
        );
    }

    #[test]
    fn reconcile_expansion_slot_content_parent_stable_across_reexpansions() {
        // After reconciliation, slot-inlined content must remain parented
        // under the SlotContent node in the state tree, so that
        // collect_prior_slots can find it on subsequent re-expansions.
        let mut router = Router::new();
        setup_scroll_view_state(&mut router);

        let sv = QualifiedId::parse("app:sv").unwrap();
        let sv_slot = QualifiedId::parse("app:sv::_").unwrap();
        let child = QualifiedId::parse("app:child").unwrap();
        let daemon = ProcessId(2);

        let effective_cmds = parse(
            "+view controls:sv-root { \
               +view controls:vp { \
                 +view app:child \
               } \
             }",
        );

        // First re-expansion.
        let delta1 = router.reconcile_expansion(&sv, daemon, &effective_cmds);
        assert!(
            !delta1
                .iter()
                .any(|c| matches!(c, Command::Destroy { id, .. } if id.as_ref() == "app:child")),
            "first reconciliation should not destroy app:child"
        );

        // Verify app:child is still parented under SlotContent.
        let child_obj = router.state.get(&child).expect("app:child should exist");
        assert_eq!(
            child_obj.parent.as_ref(),
            Some(&sv_slot),
            "app:child should remain parented under SlotContent after reconciliation"
        );

        // Second re-expansion (simulates another @scroll-view patch).
        let delta2 = router.reconcile_expansion(&sv, daemon, &effective_cmds);
        assert!(
            !delta2
                .iter()
                .any(|c| matches!(c, Command::Destroy { id, .. } if id.as_ref() == "app:child")),
            "second reconciliation should not destroy app:child either. Delta: {delta2:?}"
        );
    }

    #[test]
    fn reconcile_expansion_nested_claimed_type_preserved() {
        // When a re-expansion contains a claimed type (e.g. +scrollbar inside
        // scroll-view), the claimed type's expansion children should not be
        // destroyed — they belong to a separate nested expansion.
        let mut router = Router::new();
        setup_scroll_view_state(&mut router);

        let daemon = ProcessId(2);
        let sv = QualifiedId::parse("app:sv").unwrap();

        // Register "scrollbar" as a claimed type.
        router.claims.insert("scrollbar".to_owned(), daemon);

        // Add a scrollbar and its expansion children to the state tree
        // (simulating a previous expansion that created them).
        let sb = QualifiedId::parse("controls:sv-scrollbar-y").unwrap();
        let sb_root = QualifiedId::parse("controls:sv-scrollbar-y-root").unwrap();
        let sb_track = QualifiedId::parse("controls:sv-scrollbar-y-track").unwrap();
        let sb_thumb = QualifiedId::parse("controls:sv-scrollbar-y-thumb").unwrap();

        router.state.upsert(
            ObjectKind::Type("scrollbar".into()),
            &sb,
            &IndexMap::new(),
            daemon,
        );
        router
            .state
            .set_parent(&sb, &QualifiedId::parse("controls:sv-root").unwrap());
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &sb_root,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&sb_root, &sb);
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &sb_track,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&sb_track, &sb_root);
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &sb_thumb,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&sb_thumb, &sb_root);

        // Effective commands after slot substitution: scrollbar appears as
        // a raw claimed type (its expansion is handled separately).
        let effective_cmds = parse(
            "+view controls:sv-root { \
               +view controls:vp { \
                 +view app:child \
               } \
               +scrollbar controls:sv-scrollbar-y direction=vertical \
             }",
        );

        let delta = router.reconcile_expansion(&sv, daemon, &effective_cmds);

        // Scrollbar expansion children should NOT be destroyed.
        let destroyed_ids: Vec<_> = delta
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Destroy { id, .. } => Some(id.to_string()),
                _ => None,
            })
            .collect();
        assert!(
            !destroyed_ids.contains(&"controls:sv-scrollbar-y-root".to_owned()),
            "scrollbar expansion root should not be destroyed. Destroyed: {destroyed_ids:?}"
        );
        assert!(
            !destroyed_ids.contains(&"controls:sv-scrollbar-y-track".to_owned()),
            "scrollbar expansion track should not be destroyed. Destroyed: {destroyed_ids:?}"
        );
        assert!(
            !destroyed_ids.contains(&"controls:sv-scrollbar-y-thumb".to_owned()),
            "scrollbar expansion thumb should not be destroyed. Destroyed: {destroyed_ids:?}"
        );
    }

    #[test]
    fn reconcile_expansion_real_parent_change_still_detected() {
        // When a daemon object genuinely moves to a different parent
        // (not due to slot inlining), the parent change should still
        // be detected and emit destroy+create.
        let mut router = Router::new();
        let daemon = ProcessId(2);

        let sv = QualifiedId::parse("app:sv").unwrap();
        let root = QualifiedId::parse("controls:root").unwrap();
        let container = QualifiedId::parse("controls:container").unwrap();
        let item = QualifiedId::parse("controls:item").unwrap();

        router.state.upsert(
            ObjectKind::Type("scroll-view".into()),
            &sv,
            &IndexMap::new(),
            ProcessId(1),
        );
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &root,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&root, &sv);
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &container,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&container, &root);
        // item is currently under container
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &item,
            &IndexMap::new(),
            daemon,
        );
        router.state.set_parent(&item, &container);

        // New expansion moves item from container to root.
        let effective_cmds = parse(
            "+view controls:root { \
               +view controls:container \
               +view controls:item \
             }",
        );

        let delta = router.reconcile_expansion(&sv, daemon, &effective_cmds);

        // Should detect the real parent change.
        let has_destroy = delta.iter().any(
            |cmd| matches!(cmd, Command::Destroy { id, .. } if id.as_ref() == "controls:item"),
        );
        assert!(
            has_destroy,
            "reconcile_expansion should detect real parent change for controls:item. Delta: {delta:?}"
        );
    }

    // ── #tap / #untap registration tests ─────────────────────────

    #[test]
    fn register_tap() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_tap(pid, "compositor", "view", "scroll");
        assert_eq!(
            router.taps.get(&("view".to_owned(), "scroll".to_owned())),
            Some(&vec![pid])
        );
    }

    #[test]
    fn register_tap_plural() {
        let mut router = Router::new();
        let pid1 = ProcessId(1);
        let pid2 = ProcessId(2);
        router.register_tap(pid1, "compositor", "view", "scroll");
        router.register_tap(pid2, "compositor2", "view", "scroll");
        let tappers = router
            .taps
            .get(&("view".to_owned(), "scroll".to_owned()))
            .unwrap();
        assert_eq!(tappers.len(), 2);
        assert!(tappers.contains(&pid1));
        assert!(tappers.contains(&pid2));
    }

    #[test]
    fn register_tap_idempotent() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_tap(pid, "compositor", "view", "scroll");
        router.register_tap(pid, "compositor", "view", "scroll");
        assert_eq!(
            router
                .taps
                .get(&("view".to_owned(), "scroll".to_owned()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn unregister_tap() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_tap(pid, "compositor", "view", "scroll");
        router.unregister_tap(pid, "compositor", "view", "scroll");
        assert!(router.taps.is_empty());
    }

    #[test]
    fn unregister_tap_preserves_others() {
        let mut router = Router::new();
        let pid1 = ProcessId(1);
        let pid2 = ProcessId(2);
        router.register_tap(pid1, "compositor", "view", "scroll");
        router.register_tap(pid2, "compositor2", "view", "scroll");
        router.unregister_tap(pid1, "compositor", "view", "scroll");
        assert_eq!(
            router.taps.get(&("view".to_owned(), "scroll".to_owned())),
            Some(&vec![pid2])
        );
    }

    #[test]
    fn tap_cleanup_on_disconnect() {
        let mut router = Router::new();
        let pid = ProcessId(1);
        router.register_tap(pid, "compositor", "view", "scroll");
        router.register_tap(pid, "compositor", "text", "click");
        // Simulate disconnect cleanup (same pattern as handle_disconnect).
        for tappers in router.taps.values_mut() {
            tappers.retain(|&p| p != pid);
        }
        router.taps.retain(|_, v| !v.is_empty());
        assert!(router.taps.is_empty());
    }

    // ── #tap fan-out behavior tests ──────────────────────────────

    use crate::channel::tracked_unbounded_channel;

    /// Create a test Process with a channel pair for verifying sends.
    fn make_test_process(
        id: ProcessId,
        name: &str,
    ) -> (Process, crate::channel::TrackedUnboundedReceiver<WriteMsg>) {
        let (tx, rx) = tracked_unbounded_channel(format!("test-{name}"), 512, 256);
        let process = Process {
            id,
            name: name.to_owned(),
            kind: ProcessKind::InProcess,
            tx,
        };
        (process, rx)
    }

    /// Drain all WriteMsg::Byo commands from a receiver.
    fn drain_byo(rx: &mut crate::channel::TrackedUnboundedReceiver<WriteMsg>) -> Vec<Vec<Command>> {
        let mut all = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            if let WriteMsg::Byo(cmds) = msg {
                all.push((*cmds).clone());
            }
        }
        all
    }

    #[tokio::test]
    async fn tap_fan_out_basic() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(
                sender,
                "sender",
                &EventKind::Scroll,
                0,
                "btn",
                &[Prop::val("delta-y", "-20")],
            )
            .await;

        let msgs = drain_byo(&mut rx3);
        assert_eq!(msgs.len(), 1, "tapper should receive exactly one tap copy");
    }

    #[tokio::test]
    async fn tap_fan_out_no_self_send() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);

        let (p1, mut rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        router.add_process(p1);
        router.add_process(p2);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        // Sender also taps — should NOT receive its own event.
        router.register_tap(sender, "sender", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        let msgs = drain_byo(&mut rx1);
        assert!(
            msgs.is_empty(),
            "sender should not receive tap copy of its own event"
        );
    }

    #[tokio::test]
    async fn tap_fan_out_no_recipient_send() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, mut rx2) = make_test_process(owner, "owner");
        router.add_process(p1);
        router.add_process(p2);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        // Owner also taps — should NOT get duplicate.
        router.register_tap(owner, "owner", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        // Owner gets exactly 1 message (the primary routed event), not 2.
        let msgs = drain_byo(&mut rx2);
        assert_eq!(
            msgs.len(),
            1,
            "owner should get primary event only, no tap duplicate"
        );
    }

    #[tokio::test]
    async fn tap_fan_out_multiple_tappers() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper_a = ProcessId(3);
        let tapper_b = ProcessId(4);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper_a, "tapper-a");
        let (p4, mut rx4) = make_test_process(tapper_b, "tapper-b");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);
        router.add_process(p4);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper_a, "tapper-a", "view", "scroll");
        router.register_tap(tapper_b, "tapper-b", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        assert_eq!(drain_byo(&mut rx3).len(), 1, "tapper_a should receive tap");
        assert_eq!(drain_byo(&mut rx4).len(), 1, "tapper_b should receive tap");
    }

    #[tokio::test]
    async fn tap_fan_out_qualified_id() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        let msgs = drain_byo(&mut rx3);
        let cmds = &msgs[0];
        // The tap copy should use the qualified ID "sender:btn".
        match &cmds[0] {
            Command::Event { id, .. } => {
                assert_eq!(
                    id.as_ref(),
                    "sender:btn",
                    "tap copy should use qualified ID"
                );
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tap_fan_out_has_tap_flag() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        let msgs = drain_byo(&mut rx3);
        let cmds = &msgs[0];
        match &cmds[0] {
            Command::Event { props, .. } => {
                assert!(
                    props
                        .iter()
                        .any(|p| matches!(p, Prop::Boolean { key } if key == "tap")),
                    "tap copy should contain 'tap' flag prop"
                );
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tap_fan_out_preserves_seq() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 42, "btn", &[])
            .await;

        let msgs = drain_byo(&mut rx3);
        match &msgs[0][0] {
            Command::Event { seq, .. } => {
                assert_eq!(*seq, 42, "tap copy should preserve original sender's seq");
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tap_fan_out_preserves_props() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(
                sender,
                "sender",
                &EventKind::Scroll,
                0,
                "btn",
                &[Prop::val("delta-y", "-20"), Prop::val("scroll-y", "100")],
            )
            .await;

        let msgs = drain_byo(&mut rx3);
        match &msgs[0][0] {
            Command::Event { props, .. } => {
                assert!(
                    props.iter().any(|p| matches!(p, Prop::Value { key, value } if key == "delta-y" && value == "-20")),
                    "tap copy should preserve delta-y prop"
                );
                assert!(
                    props.iter().any(|p| matches!(p, Prop::Value { key, value } if key == "scroll-y" && value == "100")),
                    "tap copy should preserve scroll-y prop"
                );
            }
            other => panic!("expected Event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tap_fan_out_no_pending_ack() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, _rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "btn", &[])
            .await;

        // Only the primary routing should have a PendingAck (for owner), not the tapper.
        let tapper_acks: Vec<_> = router
            .pending_acks
            .keys()
            .filter(|(pid, _, _)| *pid == tapper)
            .collect();
        assert!(
            tapper_acks.is_empty(),
            "no PendingAck should exist for tapper"
        );
    }

    #[tokio::test]
    async fn tap_fan_out_unmatched_type() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        // Object type is "text", but tapper taps "view!scroll".
        let qid = QualifiedId::parse("sender:label").unwrap();
        router.state.upsert(
            ObjectKind::Type("text".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Scroll, 0, "label", &[])
            .await;

        let msgs = drain_byo(&mut rx3);
        assert!(
            msgs.is_empty(),
            "tapper should not receive event for unmatched type"
        );
    }

    #[tokio::test]
    async fn tap_fan_out_unmatched_event() {
        let mut router = Router::new();
        let sender = ProcessId(1);
        let owner = ProcessId(2);
        let tapper = ProcessId(3);

        let (p1, _rx1) = make_test_process(sender, "sender");
        let (p2, _rx2) = make_test_process(owner, "owner");
        let (p3, mut rx3) = make_test_process(tapper, "tapper");
        router.add_process(p1);
        router.add_process(p2);
        router.add_process(p3);

        let qid = QualifiedId::parse("sender:btn").unwrap();
        router.state.upsert(
            ObjectKind::Type("view".into()),
            &qid,
            &IndexMap::new(),
            owner,
        );
        // Tapper taps view!scroll, but event is click.
        router.register_tap(tapper, "tapper", "view", "scroll");

        router
            .handle_event(sender, "sender", &EventKind::Click, 0, "btn", &[])
            .await;

        let msgs = drain_byo(&mut rx3);
        assert!(
            msgs.is_empty(),
            "tapper should not receive event for unmatched event kind"
        );
    }
}
