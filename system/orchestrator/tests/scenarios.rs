mod scenarios {
    pub mod basic;
    pub mod cascade;
    pub mod disconnect;
    pub mod event_routing;
    pub mod expansion;
    pub mod graphics;
    pub mod late_connection;
    pub mod multi_app;
    pub mod nested_expansion;
    pub mod passthrough;
    pub mod projection;
    pub mod re_expansion;
    pub mod state_reduction;
}

use byo_orchestrator::channel::{TrackedUnboundedReceiver, tracked_unbounded_channel};
use byo_orchestrator::process::{Process, ProcessId, WriteMsg};
use byo_orchestrator::router::{Router, RouterMsg};

/// Create a mock process with a channel receiver for capturing messages.
///
/// Returns `(Process, Receiver)` where the receiver gets all `WriteMsg`s
/// the router sends to this process.
fn mock_process(id: u32, name: &str) -> (Process, TrackedUnboundedReceiver<WriteMsg>) {
    let (tx, rx) = tracked_unbounded_channel::<WriteMsg>(format!("test-write-{name}"), 1000, 500);
    let process = Process {
        id: ProcessId(id),
        name: name.to_string(),
        kind: byo_orchestrator::process::ProcessKind::Subprocess,
        tx,
    };
    (process, rx)
}

/// Send a BYO payload string to the router from a given process.
async fn send_byo(router: &mut Router, from: ProcessId, payload: &str) {
    let commands = byo::parser::parse(payload).expect("test payload should parse");
    router.handle(RouterMsg::Byo { from, commands }).await;
}

/// Receive the next BYO message from a mock process's channel as a raw string.
///
/// Panics if no message is available or if the message is not a `WriteMsg::Byo`.
fn recv_byo_raw(rx: &mut TrackedUnboundedReceiver<WriteMsg>) -> String {
    match rx.try_recv() {
        Ok(WriteMsg::Byo(raw)) => {
            String::from_utf8((*raw).clone()).expect("BYO payload is valid UTF-8")
        }
        Ok(other) => panic!("expected WriteMsg::Byo, got: {other:?}"),
        Err(_) => panic!("no message available"),
    }
}

/// Try to receive a BYO message, returning None if the channel is empty.
fn try_recv_byo(rx: &mut TrackedUnboundedReceiver<WriteMsg>) -> Option<String> {
    match rx.try_recv() {
        Ok(WriteMsg::Byo(raw)) => {
            Some(String::from_utf8((*raw).clone()).expect("BYO payload is valid UTF-8"))
        }
        Ok(other) => panic!("expected WriteMsg::Byo, got: {other:?}"),
        Err(_) => None,
    }
}

/// Assert that no messages are pending on the channel.
fn assert_no_message(rx: &mut TrackedUnboundedReceiver<WriteMsg>) {
    match rx.try_recv() {
        Err(_) => {}
        Ok(msg) => panic!("expected no message, got: {msg:?}"),
    }
}

/// Shorthand for creating a ProcessId.
fn pid(n: u32) -> ProcessId {
    ProcessId(n)
}

/// Send a kitty graphics payload to the router from a given process.
async fn send_graphics(router: &mut Router, from: ProcessId, payload: &[u8]) {
    router
        .handle(RouterMsg::Graphics {
            from,
            raw: payload.to_vec(),
        })
        .await;
}

/// Send passthrough bytes to the router from a given process.
async fn send_passthrough(router: &mut Router, from: ProcessId, data: &[u8]) {
    router
        .handle(RouterMsg::Passthrough {
            from,
            raw: data.to_vec(),
        })
        .await;
}

/// Receive the next Graphics message from a mock process's channel.
fn recv_graphics_raw(rx: &mut TrackedUnboundedReceiver<WriteMsg>) -> Vec<u8> {
    match rx.try_recv() {
        Ok(WriteMsg::Graphics(raw)) => (*raw).clone(),
        Ok(other) => panic!("expected WriteMsg::Graphics, got: {other:?}"),
        Err(_) => panic!("no message available"),
    }
}

/// Receive the next Passthrough message from a mock process's channel.
fn recv_passthrough_raw(rx: &mut TrackedUnboundedReceiver<WriteMsg>) -> Vec<u8> {
    match rx.try_recv() {
        Ok(WriteMsg::Passthrough(raw)) => (*raw).clone(),
        Ok(other) => panic!("expected WriteMsg::Passthrough, got: {other:?}"),
        Err(_) => panic!("no message available"),
    }
}

/// Receive the next message (any type) from a mock process's channel.
#[allow(dead_code)]
fn recv_any(rx: &mut TrackedUnboundedReceiver<WriteMsg>) -> WriteMsg {
    rx.try_recv().expect("no message available")
}
