//! Timer virtual service — observes `timer` objects and fires events on expiry.
//!
//! Any process can create one-shot timers:
//! ```text
//! +timer my-fade delay=2000    — fires after 2000ms
//! +timer my-fade delay=2000    — upsert resets the countdown
//! -timer my-fade               — cancel
//! ```
//!
//! When a timer fires, the service sends `!fire seq qid` to the timer's
//! owner (routed via the standard `handle_event` path) and auto-destroys
//! the timer with `-timer qid`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use byo::byo_write;
use byo::emitter::Emitter;
use byo::parser::parse;
use byo::protocol::{Command, Prop};

use crate::channel::{TrackedUnboundedReceiver, TrackedUnboundedSender};
use crate::process::{ProcessId, WriteMsg};
use crate::router::RouterMsg;

/// Emit BYO commands and send them as a `RouterMsg::Byo` to the router.
fn emit_and_send(
    pid: ProcessId,
    router_tx: &TrackedUnboundedSender<RouterMsg>,
    f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> std::io::Result<()>,
) {
    let mut buf = Vec::new();
    let mut em = Emitter::new(&mut buf);
    if f(&mut em).is_err() {
        return;
    }
    let payload_str = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(commands) = parse(payload_str) {
        let _ = router_tx.send(RouterMsg::Byo {
            from: pid,
            commands,
        });
    }
}

/// Main timer service loop. Runs as a tokio task.
///
/// Receives observer projections of `timer` objects via `write_rx`,
/// tracks deadlines, and fires events when timers expire.
pub async fn timer_service(
    pid: ProcessId,
    mut write_rx: TrackedUnboundedReceiver<WriteMsg>,
    router_tx: TrackedUnboundedSender<RouterMsg>,
) {
    // Subscribe as an observer for `timer` type objects.
    emit_and_send(pid, &router_tx, |em| byo_write!(em, #observe timer));

    // qid → deadline
    let mut timers: HashMap<String, Instant> = HashMap::new();
    let mut event_seq: u64 = 0;

    loop {
        // Compute sleep duration until the next timer fires.
        let sleep_dur = timers
            .values()
            .map(|d| d.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_secs(3600));

        tokio::select! {
            msg = write_rx.recv() => {
                match msg {
                    Some(WriteMsg::Byo(payload)) => {
                        process_observer_payload(&payload, &mut timers);
                    }
                    None => break, // channel closed
                    _ => {} // Graphics/Passthrough — ignore
                }
            }
            _ = tokio::time::sleep(sleep_dur) => {
                let now = Instant::now();
                let expired: Vec<String> = timers
                    .iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(qid, _)| qid.clone())
                    .collect();

                for qid in expired {
                    timers.remove(&qid);
                    let seq = event_seq;
                    event_seq += 1;

                    // Fire the event and auto-destroy the timer.
                    let qid_ref = qid.as_str();
                    emit_and_send(pid, &router_tx, |em| {
                        byo_write!(em,
                            !fire {seq} {qid_ref}
                            -timer {qid_ref}
                        )
                    });
                }
            }
        }
    }
}

/// Parse an observer projection payload and update the timer map.
///
/// The orchestrator forwards `+timer`/`@timer`/`-timer` commands to us
/// because we observe the `timer` type.
fn process_observer_payload(payload: &[u8], timers: &mut HashMap<String, Instant>) {
    let payload_str = match std::str::from_utf8(payload) {
        Ok(s) => s,
        Err(_) => return,
    };

    let commands = match parse(payload_str) {
        Ok(cmds) => cmds,
        Err(_) => return,
    };

    for cmd in commands {
        match cmd {
            Command::Upsert {
                kind, id, props, ..
            } if kind.as_ref() == "timer" => {
                let delay_ms = extract_delay(&props);
                let deadline = Instant::now() + Duration::from_millis(delay_ms);
                timers.insert(id.to_string(), deadline);
            }
            Command::Patch {
                kind, id, props, ..
            } if kind.as_ref() == "timer" => {
                let delay_ms = extract_delay(&props);
                let deadline = Instant::now() + Duration::from_millis(delay_ms);
                timers.insert(id.to_string(), deadline);
            }
            Command::Destroy { kind, id } if kind.as_ref() == "timer" => {
                timers.remove(id.as_ref());
            }
            _ => {}
        }
    }
}

/// Extract the `delay` prop value (in milliseconds), defaulting to 1000ms.
fn extract_delay(props: &[Prop]) -> u64 {
    for prop in props {
        if let Prop::Value { key, value } = prop
            && key.as_ref() == "delay"
            && let Ok(ms) = value.as_ref().parse::<u64>()
        {
            return ms;
        }
    }
    1000
}
