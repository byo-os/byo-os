//! Stdin reader thread and stdout emitter for BYO protocol IO.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use bevy::winit::{EventLoopProxy, EventLoopProxyWrapper, WinitUserEvent};
use byo::emitter::Emitter;
use byo::parser::parse_bytes;
use byo::protocol::PragmaKind;
use byo::scanner::{Handler, Scanner};
use bytes::Bytes;
use std::io::{self, Read, Stdout};
use std::sync::mpsc;
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
    pub rx: Mutex<mpsc::Receiver<StdinEvent>>,
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
    tx: mpsc::Sender<StdinEvent>,
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
                    if let byo::Command::Pragma { kind, targets } = cmd {
                        match kind {
                            PragmaKind::Redirect => {
                                if let Some(target) = targets.first() {
                                    if target.as_ref() == "_" {
                                        self.pipe_target.clear(); // discard
                                    } else {
                                        self.pipe_target = target.to_string();
                                    }
                                    debug!("redirect passthrough to {:?}", self.pipe_target);
                                }
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
    let (tx, rx) = mpsc::channel();

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

    // Send observe subscription for compositor types (including tty)
    emitter.frame(|em| {
        byo::byo_write!(em,
            #observe view,text,layer,window,tty
        )
    });

    commands.insert_resource(StdinEventQueue { rx: Mutex::new(rx) });
    commands.insert_resource(emitter);
}

/// PreUpdate system: drains the unified stdin event queue, feeding TTY data
/// and graphics commands in the exact order they arrived from the scanner.
///
/// This preserves cursor positioning: passthrough VT100 sequences update the
/// cursor before graphics placements that reference cursor position.
#[allow(clippy::too_many_arguments)]
pub fn process_stdin_events(
    queue: Res<StdinEventQueue>,
    mut byo_messages: MessageWriter<ByoBatch>,
    id_map: Res<IdMap>,
    mut ttys: Query<&mut crate::tty::TtyState>,
    mut store: ResMut<KittyGfxImageStore>,
    mut images: ResMut<Assets<Image>>,
    emitter: Res<StdoutEmitter>,
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
                let entity = id_map.get_entity(&target);
                if let Some(entity) = entity
                    && let Ok(mut state) = ttys.get_mut(entity)
                {
                    state.feed(&data);
                }
            }
            StdinEvent::KittyGfx {
                payload,
                tty_target,
            } => {
                // Read cursor position from the target TTY before processing.
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
    }
    if got_events {
        redraw.write(RequestRedraw);
    }
}
