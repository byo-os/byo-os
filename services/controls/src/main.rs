//! BYO/OS Controls Daemon
//!
//! Claims `button`, `checkbox`, and `slider` types. Expands them into
//! compositor-native `view`/`text` primitives. Handles pointer events
//! for hover/press visual states and synthesizes semantic events
//! (`press`, `change`, `input`) for apps.

mod expand;
mod state;

use std::collections::HashMap;
use std::io::{self, Read, Stdout};

use byo::byo_write;
use byo::emitter::Emitter;
use byo::parser::parse;
use byo::protocol::{Command, EventKind, Prop, RequestKind};
use byo::scanner::{Handler, Scanner};
use tracing::info;

use crate::expand::{
    expand_button, expand_checkbox, expand_slider, patch_button_state, patch_checkbox_state,
    patch_slider_state,
};
use crate::state::{ControlKind, ControlState, Daemon};

/// Stdin handler that dispatches BYO commands to the daemon.
struct StdinHandler {
    daemon: Daemon,
    stdout: Stdout,
}

impl Handler for StdinHandler {
    fn on_byo(&mut self, payload: &[u8]) {
        let payload_str = match std::str::from_utf8(payload) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("invalid UTF-8 in payload: {e}");
                return;
            }
        };
        let commands = match parse(payload_str) {
            Ok(cmds) => cmds,
            Err(e) => {
                tracing::warn!("parse error: {e}");
                return;
            }
        };

        for cmd in commands {
            if let Err(e) = self.dispatch(cmd) {
                tracing::warn!("dispatch error: {e}");
            }
        }
    }
}

impl StdinHandler {
    fn new() -> Self {
        Self {
            daemon: Daemon::new(),
            stdout: io::stdout(),
        }
    }

    fn dispatch(&mut self, cmd: Command) -> io::Result<()> {
        match cmd {
            Command::Request {
                kind: RequestKind::Expand,
                seq,
                targets,
                props,
            } => {
                self.handle_expand(seq, &targets, &props)?;
            }
            Command::Event {
                kind,
                seq,
                id,
                props,
            } => {
                self.handle_event(&kind, seq, &id, &props)?;
            }
            Command::Destroy { id, .. } => {
                self.handle_destroy(&id);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_expand(
        &mut self,
        seq: u64,
        targets: &[byo::ByteStr],
        props: &[Prop],
    ) -> io::Result<()> {
        let source_qid = match targets.first() {
            Some(t) => t.to_string(),
            None => return Ok(()),
        };

        // Extract the `kind` prop to determine control type
        let prop_map = props_to_map(props);
        let control_kind = match prop_map.get("kind").map(|s| s.as_str()) {
            Some("button") => ControlKind::Button,
            Some("checkbox") => ControlKind::Checkbox,
            Some("slider") => ControlKind::Slider,
            other => {
                tracing::warn!("unknown control kind: {other:?}");
                return Ok(());
            }
        };

        // Remove old state if re-expanding
        self.daemon.remove(&source_qid);

        let state = ControlState::new(control_kind, &source_qid, &prop_map);
        self.daemon.register(state);

        // Borrow daemon and stdout separately for the closure
        let daemon = &self.daemon;
        let state = daemon.get(&source_qid).unwrap();

        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            em.expanded_with(seq, |em| match control_kind {
                ControlKind::Button => expand_button(em, state),
                ControlKind::Checkbox => expand_checkbox(em, state),
                ControlKind::Slider => expand_slider(em, state),
            })
        })?;

        info!("expanded {control_kind:?} {source_qid}");
        Ok(())
    }

    fn handle_event(
        &mut self,
        kind: &EventKind,
        seq: u64,
        id: &str,
        props: &[Prop],
    ) -> io::Result<()> {
        info!("received event {}({seq}) target={id}", kind.as_str());

        // Look up which control this expansion ID belongs to
        let source_qid = match self.daemon.resolve(id) {
            Some(qid) => qid.to_owned(),
            None => {
                info!("  unknown ID {id}, ACK-only");
                // Unknown ID — ACK and move on
                let ack_kind = kind.as_str();
                let mut em = Emitter::new(&mut self.stdout);
                return em.frame(|em| byo_write!(em, !ack {ack_kind} {seq} handled=true));
            }
        };

        // Emit ACK + response in a single APC frame to reduce pipe traffic.
        // Each handler writes ACK + visual patches + forwarded events together.
        let ack_kind = kind.as_str();

        match kind {
            EventKind::PointerEnter => {
                self.on_pointer_enter(ack_kind, seq, &source_qid)?;
            }
            EventKind::PointerLeave => {
                self.on_pointer_leave(ack_kind, seq, &source_qid)?;
            }
            EventKind::PointerDown => {
                self.on_pointer_down(ack_kind, seq, &source_qid, props)?;
            }
            EventKind::PointerUp => {
                self.on_pointer_up(ack_kind, seq, &source_qid)?;
            }
            EventKind::PointerMove => {
                self.on_pointer_move(ack_kind, seq, &source_qid, props)?;
            }
            EventKind::Click => {
                self.on_click(ack_kind, seq, &source_qid)?;
            }
            _ => {
                let mut em = Emitter::new(&mut self.stdout);
                em.frame(|em| byo_write!(em, !ack {ack_kind} {seq} handled=true))?;
            }
        }

        Ok(())
    }

    fn on_pointer_enter(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        state.hover = true;
        let wants_enter = state.app_wants_event(&EventKind::PointerEnter);
        let enter_seq = if wants_enter {
            Some(self.daemon.next_seq("pointerenter"))
        } else {
            None
        };

        let daemon = &self.daemon;
        let state = daemon.get(source_qid).unwrap();
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
            emit_visual_patch(em, state)?;
            if let Some(seq) = enter_seq {
                byo_write!(em, !pointerenter {seq} {source_qid})?;
            }
            Ok(())
        })
    }

    fn on_pointer_leave(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        state.hover = false;
        state.pressed = false;
        let wants_leave = state.app_wants_event(&EventKind::PointerLeave);
        let leave_seq = if wants_leave {
            Some(self.daemon.next_seq("pointerleave"))
        } else {
            None
        };

        let daemon = &self.daemon;
        let state = daemon.get(source_qid).unwrap();
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
            emit_visual_patch(em, state)?;
            if let Some(seq) = leave_seq {
                byo_write!(em, !pointerleave {seq} {source_qid})?;
            }
            Ok(())
        })
    }

    fn on_pointer_down(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
        props: &[Prop],
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        state.pressed = true;
        let is_slider = state.kind == ControlKind::Slider;

        if is_slider {
            self.handle_slider_pointer(ack_kind, ack_seq, props, source_qid, true)
        } else {
            let daemon = &self.daemon;
            let state = daemon.get(source_qid).unwrap();
            let mut em = Emitter::new(&mut self.stdout);
            em.frame(|em| {
                byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                emit_visual_patch(em, state)
            })
        }
    }

    fn on_pointer_up(&mut self, ack_kind: &str, ack_seq: u64, source_qid: &str) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        let was_slider_pressed = state.pressed && state.kind == ControlKind::Slider;
        let value = state.value;
        let wants_change = state.app_wants_event(&EventKind::Change);
        state.pressed = false;
        let change_seq = if was_slider_pressed && wants_change {
            Some(self.daemon.next_seq("change"))
        } else {
            None
        };

        let daemon = &self.daemon;
        let state = daemon.get(source_qid).unwrap();
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
            emit_visual_patch(em, state)?;
            if let Some(seq) = change_seq {
                let v = value.unwrap_or(50.0);
                byo_write!(em, !change {seq} {source_qid} value={v.to_string()})?;
            }
            Ok(())
        })
    }

    fn on_pointer_move(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
        props: &[Prop],
    ) -> io::Result<()> {
        let state = match self.daemon.get(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled || !state.pressed || state.kind != ControlKind::Slider {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        self.handle_slider_pointer(ack_kind, ack_seq, props, source_qid, false)
    }

    fn on_click(&mut self, ack_kind: &str, ack_seq: u64, source_qid: &str) -> io::Result<()> {
        let state = match self.daemon.get(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }
        match state.kind {
            ControlKind::Button => {
                let press_seq = if state.app_wants_event(&EventKind::Press) {
                    Some(self.daemon.next_seq("press"))
                } else {
                    None
                };
                let mut em = Emitter::new(&mut self.stdout);
                em.frame(|em| {
                    byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                    if let Some(seq) = press_seq {
                        byo_write!(em, !press {seq} {source_qid})?;
                    }
                    Ok(())
                })?;
            }
            ControlKind::Checkbox => {
                self.handle_checkbox_click(ack_kind, ack_seq, source_qid)?;
            }
            ControlKind::Slider => {
                ack_only(&mut self.stdout, ack_kind, ack_seq)?;
            }
        }
        Ok(())
    }

    fn handle_checkbox_click(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
    ) -> io::Result<()> {
        let state = self.daemon.get(source_qid).unwrap();
        let is_controlled = state.is_controlled_checkbox();
        let checked = state.checked.unwrap_or(false);
        let new_checked = !checked;
        let wants_change = state.app_wants_event(&EventKind::Change);
        let change_seq = if wants_change {
            Some(self.daemon.next_seq("change"))
        } else {
            None
        };

        if is_controlled {
            let mut em = Emitter::new(&mut self.stdout);
            em.frame(|em| {
                byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                if let Some(seq) = change_seq {
                    byo_write!(em, !change {seq} {source_qid} checked={new_checked.to_string()})?;
                }
                Ok(())
            })?;
        } else {
            // Uncontrolled: toggle internal state, patch visuals, emit event
            let state = self.daemon.get_mut(source_qid).unwrap();
            state.checked = Some(new_checked);

            let daemon = &self.daemon;
            let state = daemon.get(source_qid).unwrap();
            let mut em = Emitter::new(&mut self.stdout);
            em.frame(|em| {
                byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                patch_checkbox_state(em, state)?;
                if let Some(seq) = change_seq {
                    byo_write!(em, !change {seq} {source_qid} checked={new_checked.to_string()})?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    fn handle_slider_pointer(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        props: &[Prop],
        source_qid: &str,
        request_capture: bool,
    ) -> io::Result<()> {
        // x = pointer position relative to the dispatch element (track)
        // width = dispatch element's computed width
        let prop_map = props_to_map(props);
        let x: f64 = prop_map
            .get("x")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let width: f64 = prop_map
            .get("width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        let state = self.daemon.get(source_qid).unwrap();
        let is_controlled = state.is_controlled_slider();
        let min = state.slider_min();
        let max = state.slider_max();
        let wants_input = state.app_wants_event(&EventKind::Input);
        let prev_value = state.value;

        // Compute raw value from pointer position
        let ratio = if width > 0.0 {
            (x / width).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let raw_value = min + ratio * (max - min);
        let snapped = state.snap_slider_value(raw_value);

        // Skip if value hasn't changed (reduces pipe traffic during drag)
        let value_changed = prev_value.is_none_or(|v| (v - snapped).abs() > f64::EPSILON);

        if is_controlled {
            if wants_input && value_changed {
                let input_seq = self.daemon.next_seq("input");
                let mut em = Emitter::new(&mut self.stdout);
                em.frame(|em| {
                    byo_write!(em,
                        !ack {ack_kind} {ack_seq} handled=true
                            if request_capture { capture=true }
                    )?;
                    byo_write!(em, !input {input_seq} {source_qid} value={snapped.to_string()})
                })?;
            } else {
                let mut em = Emitter::new(&mut self.stdout);
                em.frame(|em| {
                    byo_write!(em,
                        !ack {ack_kind} {ack_seq} handled=true
                            if request_capture { capture=true }
                    )
                })?;
            }
        } else if value_changed {
            // Uncontrolled: update internal state, patch visuals, emit input
            let state = self.daemon.get_mut(source_qid).unwrap();
            state.value = Some(snapped);
            let input_seq = if wants_input {
                Some(self.daemon.next_seq("input"))
            } else {
                None
            };

            let daemon = &self.daemon;
            let state = daemon.get(source_qid).unwrap();
            let mut em = Emitter::new(&mut self.stdout);
            em.frame(|em| {
                byo_write!(em,
                    !ack {ack_kind} {ack_seq} handled=true
                        if request_capture { capture=true }
                )?;
                patch_slider_state(em, state)?;
                if let Some(seq) = input_seq {
                    byo_write!(em, !input {seq} {source_qid} value={snapped.to_string()})?;
                }
                Ok(())
            })?;
        } else {
            let mut em = Emitter::new(&mut self.stdout);
            em.frame(|em| {
                byo_write!(em,
                    !ack {ack_kind} {ack_seq} handled=true
                        if request_capture { capture=true }
                )
            })?;
        }

        Ok(())
    }

    fn handle_destroy(&mut self, id: &str) {
        // The orchestrator sends -type {qualified_id} when the source is destroyed.
        // Try direct lookup first, then check reverse map.
        if self.daemon.get(id).is_some() {
            info!("destroy control {id}");
            self.daemon.remove(id);
        } else if let Some(source_qid) = self.daemon.resolve(id) {
            let source_qid = source_qid.to_owned();
            info!("destroy control {source_qid} (via expansion id {id})");
            self.daemon.remove(&source_qid);
        }
    }
}

/// Emit a visual patch for the current state of a control.
fn emit_visual_patch<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    match state.kind {
        ControlKind::Button => patch_button_state(em, state),
        ControlKind::Checkbox => patch_checkbox_state(em, state),
        ControlKind::Slider => patch_slider_state(em, state),
    }
}

/// Emit an ACK-only frame (no additional response).
fn ack_only(stdout: &mut Stdout, kind: &str, seq: u64) -> io::Result<()> {
    let mut em = Emitter::new(stdout);
    em.frame(|em| byo_write!(em, !ack {kind} {seq} handled=true))
}

/// Convert a Prop slice to a HashMap for easy lookup.
fn props_to_map(props: &[Prop]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for prop in props {
        match prop {
            Prop::Value { key, value } => {
                map.insert(key.to_string(), value.to_string());
            }
            Prop::Boolean { key } => {
                map.insert(key.to_string(), String::new());
            }
            Prop::Remove { .. } => {}
        }
    }
    map
}

fn main() {
    tracing_subscriber::fmt().with_writer(io::stderr).init();

    info!("controls daemon starting");

    // Claim button, checkbox, slider
    {
        let mut stdout = io::stdout();
        let mut em = Emitter::new(&mut stdout);
        em.frame(|em| byo_write!(em, #claim button, checkbox, slider))
            .expect("failed to claim types");
    }

    // Main event loop
    let mut handler = StdinHandler::new();
    let mut scanner = Scanner::new();
    let mut buf = [0u8; 8192];

    let stdin = io::stdin();
    loop {
        let n = match stdin.lock().read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("stdin read error: {e}");
                break;
            }
        };
        scanner.feed(&buf[..n], &mut handler);
    }

    info!("controls daemon exiting");
}
