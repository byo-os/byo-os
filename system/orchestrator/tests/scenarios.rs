mod scenarios {
    pub mod basic;
    pub mod cascade;
    pub mod disconnect;
    pub mod expansion;
    pub mod late_connection;
    pub mod multi_app;
    pub mod nested_expansion;
    pub mod projection;
    pub mod re_expansion;
    pub mod state_reduction;
}

use tokio::sync::mpsc;

use byo_orchestrator::process::{Process, ProcessId, WriteMsg};
use byo_orchestrator::router::{Router, RouterMsg};

/// Create a mock process with a channel receiver for capturing messages.
///
/// Returns `(Process, Receiver)` where the receiver gets all `WriteMsg`s
/// the router sends to this process.
fn mock_process(id: u32, name: &str) -> (Process, mpsc::Receiver<WriteMsg>) {
    let (tx, rx) = mpsc::channel::<WriteMsg>(256);
    let process = Process {
        id: ProcessId(id),
        name: name.to_string(),
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
fn recv_byo_raw(rx: &mut mpsc::Receiver<WriteMsg>) -> String {
    match rx.try_recv() {
        Ok(WriteMsg::Byo(raw)) => {
            String::from_utf8((*raw).clone()).expect("BYO payload is valid UTF-8")
        }
        Ok(other) => panic!("expected WriteMsg::Byo, got: {other:?}"),
        Err(_) => panic!("no message available"),
    }
}

/// Try to receive a BYO message, returning None if the channel is empty.
fn try_recv_byo(rx: &mut mpsc::Receiver<WriteMsg>) -> Option<String> {
    match rx.try_recv() {
        Ok(WriteMsg::Byo(raw)) => {
            Some(String::from_utf8((*raw).clone()).expect("BYO payload is valid UTF-8"))
        }
        Ok(other) => panic!("expected WriteMsg::Byo, got: {other:?}"),
        Err(_) => None,
    }
}

/// Assert that no messages are pending on the channel.
fn assert_no_message(rx: &mut mpsc::Receiver<WriteMsg>) {
    match rx.try_recv() {
        Err(mpsc::error::TryRecvError::Empty) => {}
        Ok(msg) => panic!("expected no message, got: {msg:?}"),
        Err(e) => panic!("unexpected channel error: {e:?}"),
    }
}

/// Shorthand for creating a ProcessId.
fn pid(n: u32) -> ProcessId {
    ProcessId(n)
}
