//! Stdin reader thread and stdout emitter for BYO protocol IO.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use bevy::winit::{EventLoopProxy, EventLoopProxyWrapper, WinitUserEvent};
use byo::channel::{TrackedReceiver, TrackedSender, tracked_channel};
use byo::emitter::Emitter;
use byo::parser::parse_bytes;
use byo::protocol::PragmaKind;
use byo::scanner::{Handler, Scanner};
use bytes::Bytes;
use std::io::{self, Read, Stdout};
use std::sync::{Arc, Mutex};

use crate::id_map::IdMap;
use crate::kitty_gfx::store::KittyGfxImageStore;

/// A batch of parsed BYO commands received from stdin.
#[derive(Message)]
pub struct ByoBatch(pub Vec<byo::Command>);

/// Ordered stdin event — preserves interleaving from the scanner.
pub enum StdinEvent {
    /// A batch of parsed BYO protocol commands.
    Byo(Vec<byo::Command>),
    /// Passthrough bytes destined for a named TTY entity.
    Tty { target: String, data: Vec<u8> },
    /// Raw kitty graphics payload, tagged with the current passthrough target.
    KittyGfx {
        payload: Vec<u8>,
        tty_target: String,
    },
}

/// Resource wrapping the receiving end of the unified stdin event channel.
#[derive(Resource)]
pub struct StdinEventQueue {
    pub rx: Mutex<TrackedReceiver<StdinEvent>>,
}

/// Buffered TTY/KittyGfx events from `process_stdin_events` that need entity
/// lookups. Processed after `process_commands` creates new entities.
#[derive(Resource, Default)]
pub struct DeferredTtyEvents {
    pub tty: Vec<(String, Vec<u8>)>,
    pub kitty_gfx: Vec<(Vec<u8>, String)>,
}

/// Resource wrapping a shared stdout emitter for sending BYO commands.
#[derive(Resource, Clone)]
pub struct StdoutEmitter {
    inner: Arc<Mutex<Emitter<Stdout>>>,
}

impl StdoutEmitter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Emitter::new(io::stdout()))),
        }
    }

    /// Send a framed BYO message via stdout.
    pub fn frame(&self, f: impl FnOnce(&mut Emitter<Stdout>) -> io::Result<()>) {
        let mut em = self.inner.lock().unwrap();
        em.frame(f).unwrap();
    }

    /// Send a framed kitty graphics response via stdout.
    pub fn kitty_gfx_frame(&self, payload: &[u8]) {
        let mut em = self.inner.lock().unwrap();
        em.kitty_gfx_frame(payload).unwrap();
    }
}

/// Scanner handler that parses BYO payloads and sends them through a single
/// ordered channel, then wakes the Bevy event loop for immediate processing.
struct StdinHandler {
    tx: TrackedSender<StdinEvent>,
    event_loop_proxy: EventLoopProxy<WinitUserEvent>,
    /// Current passthrough target. `"/"` = root tty, empty = discard.
    pipe_target: String,
}

impl Handler for StdinHandler {
    fn on_byo(&mut self, payload: &[u8]) {
        trace!("received payload ({} bytes)", payload.len());
        let bytes = Bytes::copy_from_slice(payload);
        match parse_bytes(bytes) {
            Ok(commands) if !commands.is_empty() => {
                trace!("parsed {} commands", commands.len());

                // Scan for redirect/unredirect pragmas and update pipe_target.
                for cmd in &commands {
                    if let byo::Command::Pragma(pragma) = cmd {
                        match pragma {
                            PragmaKind::Redirect(target) => {
                                if target.as_ref() == "_" {
                                    self.pipe_target.clear(); // discard
                                } else {
                                    self.pipe_target = target.to_string();
                                }
                                debug!("redirect passthrough to {:?}", self.pipe_target);
                            }
                            PragmaKind::Unredirect => {
                                self.pipe_target = "/".to_string();
                                debug!("unredirect passthrough to root tty");
                            }
                            _ => {}
                        }
                    }
                }

                if self.tx.send(StdinEvent::Byo(commands)).is_ok() {
                    let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
                }
            }
            Ok(_) => {}
            Err(e) => {
                warn!("parse error: {e}");
            }
        }
    }

    fn on_kitty_gfx(&mut self, payload: &[u8]) {
        trace!("received graphics payload ({} bytes)", payload.len());
        let event = StdinEvent::KittyGfx {
            payload: payload.to_vec(),
            tty_target: self.pipe_target.clone(),
        };
        if self.tx.send(event).is_ok() {
            let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
        }
    }

    fn on_passthrough(&mut self, data: &[u8]) {
        if self.pipe_target.is_empty() {
            return; // discard
        }
        let event = StdinEvent::Tty {
            target: self.pipe_target.clone(),
            data: data.to_vec(),
        };
        if self.tx.send(event).is_ok() {
            let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
        }
    }
}

/// Startup system: spawns the stdin reader thread, creates the stdout
/// emitter, and sends the initial observe subscription.
pub fn setup_io(mut commands: Commands, event_loop_proxy: Res<EventLoopProxyWrapper>) {
    let (tx, rx) = tracked_channel::<StdinEvent>("stdin", 1024, 512);

    // Clone the inner winit EventLoopProxy for the stdin thread
    let proxy: EventLoopProxy<WinitUserEvent> = (*event_loop_proxy).clone();

    // Spawn stdin reader thread
    std::thread::spawn(move || {
        let mut scanner = Scanner::new();
        let mut handler = StdinHandler {
            tx,
            event_loop_proxy: proxy,
            pipe_target: "/".to_string(),
        };
        let stdin = io::stdin();
        let mut buf = [0u8; 8192];
        info!("stdin reader started");
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) => {
                    debug!("stdin EOF");
                    break;
                }
                Ok(n) => {
                    trace!("read {n} bytes from stdin");
                    scanner.feed(&buf[..n], &mut handler);
                }
                Err(e) => {
                    warn!("stdin read error: {e}");
                    break;
                }
            }
        }
    });

    let emitter = StdoutEmitter::new();

    // Send observe subscription for compositor types (including tty and graphics)
    // and register as handler for measure requests
    emitter.frame(|em| {
        byo::byo_write!(em,
            #observe view,text,layer,window,tty,G
            #handle view?measure,text?measure,layer?measure,window?measure
            #handle view?scroll-to,view?scroll-by
        )
    });

    commands.insert_resource(StdinEventQueue { rx: Mutex::new(rx) });
    commands.insert_resource(emitter);
}

/// PreUpdate system: drains the unified stdin event queue.
///
/// BYO commands are written as [`ByoBatch`] messages for `process_commands`.
/// TTY passthrough and KittyGfx events are buffered in [`DeferredTtyEvents`]
/// so they can be processed AFTER `process_commands` creates new entities —
/// this prevents a race where passthrough arrives in the same frame as the
/// `+tty` command that creates the target entity.
pub fn process_stdin_events(
    queue: Res<StdinEventQueue>,
    mut byo_messages: MessageWriter<ByoBatch>,
    mut deferred: ResMut<DeferredTtyEvents>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let rx = queue.rx.lock().unwrap();
    let mut got_events = false;
    while let Ok(event) = rx.try_recv() {
        got_events = true;
        match event {
            StdinEvent::Byo(commands) => {
                debug!("received batch of {} commands", commands.len());
                byo_messages.write(ByoBatch(commands));
            }
            StdinEvent::Tty { target, data } => {
                deferred.tty.push((target, data));
            }
            StdinEvent::KittyGfx {
                payload,
                tty_target,
            } => {
                deferred.kitty_gfx.push((payload, tty_target));
            }
        }
    }
    if got_events {
        redraw.write(RequestRedraw);
    }
}

/// PreUpdate system (runs after `process_commands`): processes buffered TTY
/// passthrough and KittyGfx events now that entities have been created.
///
/// Preserves cursor positioning: passthrough VT100 sequences update the
/// cursor before graphics placements that reference cursor position.
#[allow(clippy::too_many_arguments)]
pub fn process_deferred_tty(
    mut deferred: ResMut<DeferredTtyEvents>,
    id_map: Res<IdMap>,
    mut ttys: Query<&mut crate::tty::TtyState>,
    mut store: ResMut<KittyGfxImageStore>,
    mut images: ResMut<Assets<Image>>,
    emitter: Res<StdoutEmitter>,
) {
    for (target, data) in deferred.tty.drain(..) {
        if let Some(entity) = id_map.get_entity(&target)
            && let Ok(mut state) = ttys.get_mut(entity)
        {
            state.feed(&data);
        } else {
            warn!(
                "TTY data for unknown target {target:?}, dropped {} bytes",
                data.len()
            );
        }
    }
    for (payload, tty_target) in deferred.kitty_gfx.drain(..) {
        let cursor_pos = id_map
            .get_entity(&tty_target)
            .and_then(|e| ttys.get(e).ok())
            .map(|state| state.cursor_point())
            .unwrap_or((0, 0));

        crate::kitty_gfx::systems::process_single(
            &payload,
            &mut store,
            &mut images,
            &emitter,
            cursor_pos,
            &tty_target,
        );
    }
}
