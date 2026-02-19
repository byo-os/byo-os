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

/// A batch of parsed BYO commands received from stdin.
#[derive(Message)]
pub struct ByoBatch(pub Vec<byo::Command>);

/// Passthrough bytes destined for a named TTY entity.
#[derive(Message)]
pub struct TtyInput {
    pub target: String,
    pub data: Vec<u8>,
}

/// Resource wrapping the receiving end of the stdin command channel.
#[derive(Resource)]
pub struct ByoCommandQueue {
    rx: Mutex<mpsc::Receiver<Vec<byo::Command>>>,
}

/// Resource wrapping the receiving end of the tty input channel.
#[derive(Resource)]
pub struct TtyInputQueue {
    rx: Mutex<mpsc::Receiver<TtyInput>>,
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
}

/// Scanner handler that parses BYO payloads and sends them through a channel,
/// then wakes the Bevy event loop for immediate processing.
struct StdinHandler {
    tx: mpsc::Sender<Vec<byo::Command>>,
    tty_tx: mpsc::Sender<TtyInput>,
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

                if self.tx.send(commands).is_ok() {
                    let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
                }
            }
            Ok(_) => {}
            Err(e) => {
                warn!("parse error: {e}");
            }
        }
    }

    fn on_passthrough(&mut self, data: &[u8]) {
        if self.pipe_target.is_empty() {
            return; // discard
        }
        let input = TtyInput {
            target: self.pipe_target.clone(),
            data: data.to_vec(),
        };
        if self.tty_tx.send(input).is_ok() {
            let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
        }
    }
}

/// Startup system: spawns the stdin reader thread, creates the stdout
/// emitter, and sends the initial observe subscription.
pub fn setup_io(mut commands: Commands, event_loop_proxy: Res<EventLoopProxyWrapper>) {
    let (tx, rx) = mpsc::channel();
    let (tty_tx, tty_rx) = mpsc::channel();

    // Clone the inner winit EventLoopProxy for the stdin thread
    let proxy: EventLoopProxy<WinitUserEvent> = (*event_loop_proxy).clone();

    // Spawn stdin reader thread
    std::thread::spawn(move || {
        let mut scanner = Scanner::new();
        let mut handler = StdinHandler {
            tx,
            tty_tx,
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

    commands.insert_resource(ByoCommandQueue { rx: Mutex::new(rx) });
    commands.insert_resource(TtyInputQueue {
        rx: Mutex::new(tty_rx),
    });
    commands.insert_resource(emitter);
}

/// PreUpdate system: drains the command channel into `ByoBatch` messages.
pub fn drain_commands(
    queue: Res<ByoCommandQueue>,
    mut messages: MessageWriter<ByoBatch>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let rx = queue.rx.lock().unwrap();
    let mut got_commands = false;
    while let Ok(commands) = rx.try_recv() {
        debug!("received batch of {} commands", commands.len());
        messages.write(ByoBatch(commands));
        got_commands = true;
    }
    if got_commands {
        redraw.write(RequestRedraw);
    }
}

/// PreUpdate system: drains the tty input channel into `TtyInput` messages.
pub fn drain_tty_input(
    queue: Res<TtyInputQueue>,
    mut messages: MessageWriter<TtyInput>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let rx = queue.rx.lock().unwrap();
    let mut got_input = false;
    while let Ok(input) = rx.try_recv() {
        messages.write(input);
        got_input = true;
    }
    if got_input {
        redraw.write(RequestRedraw);
    }
}
