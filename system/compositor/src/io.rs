//! Stdin reader thread and stdout emitter for BYO protocol IO.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use bevy::winit::{EventLoopProxy, EventLoopProxyWrapper, WinitUserEvent};
use byo::emitter::Emitter;
use byo::parser::parse_bytes;
use byo::scanner::{Handler, Scanner};
use bytes::Bytes;
use std::io::{self, Read, Stdout};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// A batch of parsed BYO commands received from stdin.
#[derive(Message)]
pub struct ByoBatch(pub Vec<byo::Command>);

/// Resource wrapping the receiving end of the stdin command channel.
#[derive(Resource)]
pub struct ByoCommandQueue {
    rx: Mutex<mpsc::Receiver<Vec<byo::Command>>>,
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
    event_loop_proxy: EventLoopProxy<WinitUserEvent>,
}

impl Handler for StdinHandler {
    fn on_byo(&mut self, payload: &[u8]) {
        let bytes = Bytes::copy_from_slice(payload);
        match parse_bytes(bytes) {
            Ok(commands) if !commands.is_empty() => {
                if self.tx.send(commands).is_ok() {
                    // Wake the Bevy event loop so drain_commands runs immediately
                    let _ = self.event_loop_proxy.send_event(WinitUserEvent::WakeUp);
                }
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("BYO parse error: {e}");
            }
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
        };
        let stdin = io::stdin();
        let mut buf = [0u8; 8192];
        loop {
            match stdin.lock().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => scanner.feed(&buf[..n], &mut handler),
                Err(e) => {
                    eprintln!("stdin read error: {e}");
                    break;
                }
            }
        }
    });

    let emitter = StdoutEmitter::new();

    // Send observe subscription for compositor types
    emitter.frame(|em| {
        byo::byo_write!(em,
            ?observe 0 view,text,layer,window
        )
    });

    commands.insert_resource(ByoCommandQueue { rx: Mutex::new(rx) });
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
        messages.write(ByoBatch(commands));
        got_commands = true;
    }
    if got_commands {
        redraw.write(RequestRedraw);
    }
}
