//! Process spawning and I/O management.
//!
//! Each child process gets a reader task (stdout → router) and a writer
//! task (router → stdin). Communication uses raw bytes; the scanner splits
//! them into BYO payloads, graphics payloads, and passthrough.

use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

use bytes::Bytes;

use byo::scanner::{Handler, Scanner};
use byo::{APC_START, KITTY_GFX_PROTOCOL_ID, PROTOCOL_ID, ST};

use crate::router::RouterMsg;

/// Unique identifier for a managed process.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ProcessId(pub u32);

/// Message sent to a process's stdin writer task.
///
/// Payloads use `Arc<Vec<u8>>` so fan-out to multiple observers is a
/// refcount bump instead of a deep copy.
#[derive(Debug, Clone)]
pub enum WriteMsg {
    /// Raw BYO payload (will be APC-framed with `B` prefix).
    Byo(Arc<Vec<u8>>),
    /// Raw graphics payload (will be APC-framed with `G` prefix).
    Graphics(Arc<Vec<u8>>),
    /// Raw bytes, no framing.
    Passthrough(Arc<Vec<u8>>),
}

/// A managed child process.
pub struct Process {
    pub id: ProcessId,
    pub name: String,
    pub tx: mpsc::UnboundedSender<WriteMsg>,
}

/// Scanner handler that collects complete payloads and sends them to the
/// router channel.
struct CollectHandler {
    process_id: ProcessId,
    router_tx: mpsc::Sender<RouterMsg>,
}

impl Handler for CollectHandler {
    fn on_byo(&mut self, payload: &[u8]) {
        let bytes = Bytes::copy_from_slice(payload);
        let commands = match byo::parser::parse_bytes(bytes) {
            Ok(cmds) => cmds,
            Err(e) => {
                tracing::warn!(
                    "parse error from pid={}: {e} (payload: {:?})",
                    self.process_id.0,
                    String::from_utf8_lossy(payload)
                );
                return;
            }
        };
        let msg = RouterMsg::Byo {
            from: self.process_id,
            commands,
        };
        // try_send avoids deadlocks in the sync scanner callback.
        let _ = self.router_tx.try_send(msg);
    }

    fn on_kitty_gfx(&mut self, payload: &[u8]) {
        let msg = RouterMsg::Graphics {
            from: self.process_id,
            raw: payload.to_vec(),
        };
        let _ = self.router_tx.try_send(msg);
    }

    fn on_passthrough(&mut self, data: &[u8]) {
        let msg = RouterMsg::Passthrough {
            from: self.process_id,
            raw: data.to_vec(),
        };
        let _ = self.router_tx.try_send(msg);
    }
}

/// Spawn a child process and start reader/writer tasks.
///
/// Returns the `Process` handle (with the write channel) and spawns:
/// - A reader task that reads the child's stdout, runs it through `Scanner`,
///   and sends `RouterMsg`s to `router_tx`.
/// - A writer task that receives `WriteMsg`s and writes to the child's stdin
///   with appropriate APC framing.
pub fn spawn_process(
    id: ProcessId,
    name: String,
    command: &str,
    args: &[String],
    router_tx: mpsc::Sender<RouterMsg>,
) -> std::io::Result<Process> {
    // Split the command string on whitespace so that commands like
    // "cargo run -p byo-compositor" work without requiring separate args.
    let parts: Vec<&str> = command.split_whitespace().collect();
    let (program, inline_args) = parts.split_first().unwrap_or((&command, &[]));
    let mut child = TokioCommand::new(program)
        .args(inline_args)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    let stdout = child.stdout.take().expect("child stdout piped");
    let stdin = child.stdin.take().expect("child stdin piped");

    let (write_tx, write_rx) = mpsc::unbounded_channel::<WriteMsg>();

    // Reader task: child stdout → Scanner → RouterMsg.
    let reader_router_tx = router_tx.clone();
    let reader_id = id;
    tokio::spawn(async move {
        reader_task(reader_id, stdout, reader_router_tx).await;
    });

    // Writer task: WriteMsg → child stdin.
    tokio::spawn(async move {
        writer_task(stdin, write_rx).await;
    });

    // Monitor task: detect child exit → send Disconnected.
    let monitor_router_tx = router_tx;
    let monitor_id = id;
    tokio::spawn(async move {
        let _ = child.wait().await;
        let _ = monitor_router_tx
            .send(RouterMsg::Disconnected {
                process: monitor_id,
            })
            .await;
    });

    Ok(Process {
        id,
        name,
        tx: write_tx,
    })
}

async fn reader_task(
    process_id: ProcessId,
    mut stdout: tokio::process::ChildStdout,
    router_tx: mpsc::Sender<RouterMsg>,
) {
    let mut scanner = Scanner::new();
    let mut handler = CollectHandler {
        process_id,
        router_tx: router_tx.clone(),
    };
    let mut buf = [0u8; 4096];

    loop {
        match stdout.read(&mut buf).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                scanner.feed(&buf[..n], &mut handler);
            }
            Err(_) => break,
        }
    }

    let _ = router_tx
        .send(RouterMsg::Disconnected {
            process: process_id,
        })
        .await;
}

async fn writer_task(
    mut stdin: tokio::process::ChildStdin,
    mut rx: mpsc::UnboundedReceiver<WriteMsg>,
) {
    let mut frame = Vec::new();
    while let Some(msg) = rx.recv().await {
        let result = match &msg {
            WriteMsg::Byo(payload) => {
                frame.clear();
                frame.reserve(APC_START.len() + 1 + payload.len() + ST.len());
                frame.extend_from_slice(APC_START);
                frame.push(PROTOCOL_ID);
                frame.extend_from_slice(payload);
                frame.extend_from_slice(ST);
                stdin.write_all(&frame).await
            }
            WriteMsg::Graphics(payload) => {
                frame.clear();
                frame.reserve(APC_START.len() + 1 + payload.len() + ST.len());
                frame.extend_from_slice(APC_START);
                frame.push(KITTY_GFX_PROTOCOL_ID);
                frame.extend_from_slice(payload);
                frame.extend_from_slice(ST);
                stdin.write_all(&frame).await
            }
            WriteMsg::Passthrough(data) => stdin.write_all(data).await,
        };
        if result.is_err() {
            break;
        }
    }
}
