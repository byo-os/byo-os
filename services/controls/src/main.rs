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
    expand_button, expand_checkbox, expand_scroll_view, expand_scrollbar, expand_slider,
    patch_button_state, patch_checkbox_state, patch_scroll_view_state, patch_scrollbar_state,
    patch_slider_state, resolve_scrollbar_style,
};
use crate::state::{ControlKind, ControlState, Daemon};

/// Which sub-element of a scrollbar was targeted by an event.
#[derive(Debug)]
enum ScrollbarTarget {
    Thumb,
    Track,
    Other,
}

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
            Some("scrollbar") => ControlKind::Scrollbar,
            Some("scroll-view") => ControlKind::ScrollView,
            other => {
                tracing::warn!("unknown control kind: {other:?}");
                return Ok(());
            }
        };

        // Update existing state or create new.
        // For re-expansions, preserve interaction state (hover, pressed, etc.)
        // and internal state (scroll positions, content/viewport sizes).
        if let Some(existing) = self.daemon.get_mut(&source_qid) {
            existing.update_props(&prop_map);
        } else {
            let state = ControlState::new(control_kind, &source_qid, &prop_map);
            self.daemon.register(state);
        }

        // Borrow daemon and stdout separately for the closure
        let daemon = &self.daemon;
        let state = daemon.get(&source_qid).unwrap();

        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            em.expanded_with(seq, |em| match control_kind {
                ControlKind::Button => expand_button(em, state),
                ControlKind::Checkbox => expand_checkbox(em, state),
                ControlKind::Slider => expand_slider(em, state),
                ControlKind::Scrollbar => expand_scrollbar(em, state),
                ControlKind::ScrollView => expand_scroll_view(em, state),
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

        // Timer fire events use a naming convention: {scrollbar_local_id}-fade
        if *kind == EventKind::Fire {
            return self.on_fire(seq, id);
        }

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

        let expansion_id = id.to_owned();

        match kind {
            EventKind::PointerEnter => {
                self.on_pointer_enter(ack_kind, seq, &source_qid, &expansion_id)?;
            }
            EventKind::PointerLeave => {
                self.on_pointer_leave(ack_kind, seq, &source_qid, &expansion_id)?;
            }
            EventKind::PointerDown => {
                self.on_pointer_down(ack_kind, seq, &source_qid, props, &expansion_id)?;
            }
            EventKind::PointerUp => {
                self.on_pointer_up(ack_kind, seq, &source_qid, &expansion_id)?;
            }
            EventKind::PointerMove => {
                self.on_pointer_move(ack_kind, seq, &source_qid, props, &expansion_id)?;
            }
            EventKind::Click => {
                self.on_click(ack_kind, seq, &source_qid)?;
            }
            EventKind::Scroll => {
                self.on_scroll(ack_kind, seq, &source_qid, props)?;
            }
            EventKind::Resize => {
                self.on_resize(ack_kind, seq, &source_qid, props)?;
            }
            _ => {
                let mut em = Emitter::new(&mut self.stdout);
                em.frame(|em| byo_write!(em, !ack {ack_kind} {seq} handled=true))?;
            }
        }

        Ok(())
    }

    /// Determine if the event target is a scrollbar sub-element and which one.
    fn scrollbar_target_kind(state: &ControlState, expansion_id: &str) -> ScrollbarTarget {
        let id = &state.local_id;
        if expansion_id == format!("{id}-thumb") {
            ScrollbarTarget::Thumb
        } else if expansion_id == format!("{id}-track") {
            ScrollbarTarget::Track
        } else {
            ScrollbarTarget::Other
        }
    }

    fn on_pointer_enter(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
        expansion_id: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        // Scrollbar sub-element hover tracking
        let mut fade_cancel_timer: Option<String> = None;
        if state.kind == ControlKind::Scrollbar {
            match Self::scrollbar_target_kind(state, expansion_id) {
                ScrollbarTarget::Thumb => state.thumb_hover = true,
                ScrollbarTarget::Track => state.track_hover = true,
                ScrollbarTarget::Other => {}
            }
            // Fade-in on hover and cancel any pending fade-out timer
            if resolve_scrollbar_style(state) == "modern" {
                state.fade_visible = true;
                fade_cancel_timer = Some(format!("{}-fade", state.local_id));
            }
        } else {
            state.hover = true;
        }

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
            if let Some(ref timer_id) = fade_cancel_timer {
                byo_write!(em, -timer {timer_id.as_str()})?;
            }
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
        expansion_id: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        let mut fade_arm_timer: Option<String> = None;
        if state.kind == ControlKind::Scrollbar {
            match Self::scrollbar_target_kind(state, expansion_id) {
                ScrollbarTarget::Thumb => {
                    state.thumb_hover = false;
                    // Don't clear thumb_pressed here — it's cleared on PointerUp.
                    // With pointer capture, PointerLeave fires when the cursor
                    // exits the thumb bounds but drag should continue.
                }
                ScrollbarTarget::Track => state.track_hover = false,
                ScrollbarTarget::Other => {}
            }
            // Arm fade-out timer on leave (fade_visible stays true until timer fires)
            if resolve_scrollbar_style(state) == "modern" {
                fade_arm_timer = Some(format!("{}-fade", state.local_id));
            }
        } else {
            state.hover = false;
            state.pressed = false;
        }

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
            if let Some(ref timer_id) = fade_arm_timer {
                byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
            }
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
        expansion_id: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        let is_slider = state.kind == ControlKind::Slider;
        let is_scrollbar = state.kind == ControlKind::Scrollbar;

        if is_scrollbar {
            let target = Self::scrollbar_target_kind(state, expansion_id);
            match target {
                ScrollbarTarget::Thumb => {
                    state.thumb_pressed = true;
                    let prop_map = props_to_map(props);
                    // Use viewport-relative coords (stable during capture)
                    let cy: f64 = prop_map
                        .get("client-y")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let cx: f64 = prop_map
                        .get("client-x")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let direction = state
                        .props
                        .get("direction")
                        .map(|s| s.as_str())
                        .unwrap_or("vertical");
                    let is_vertical = direction != "horizontal";
                    state.drag_start_pos = if is_vertical { cy } else { cx };
                    state.drag_start_scroll = state
                        .props
                        .get("scroll-position")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    // Derive track size from thumb pixel size + content/viewport ratio
                    let thumb_pixels: f64 = if is_vertical {
                        prop_map
                            .get("height")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1.0)
                    } else {
                        prop_map
                            .get("width")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1.0)
                    };
                    let thumb_pct = if state.content_size > 0.0 {
                        (state.viewport_size / state.content_size).clamp(0.05, 1.0)
                    } else {
                        1.0
                    };
                    state.drag_track_size = if thumb_pct > 0.0 {
                        thumb_pixels / thumb_pct
                    } else {
                        thumb_pixels
                    };

                    info!(
                        "thumb_down: scrollbar={source_qid} content={:.0} viewport={:.0} \
                         thumb_px={thumb_pixels:.0} thumb_pct={thumb_pct:.3} track={:.0} \
                         scroll_pos={:.1} drag_start={:.1}",
                        state.content_size,
                        state.viewport_size,
                        state.drag_track_size,
                        state.drag_start_scroll,
                        state.drag_start_pos,
                    );

                    let daemon = &self.daemon;
                    let state = daemon.get(source_qid).unwrap();
                    let mut em = Emitter::new(&mut self.stdout);
                    return em.frame(|em| {
                        byo_write!(em, !ack {ack_kind} {ack_seq} handled=true capture=true)?;
                        emit_visual_patch(em, state)
                    });
                }
                ScrollbarTarget::Track => {
                    // Track click: jump to position
                    let prop_map = props_to_map(props);
                    let direction = state
                        .props
                        .get("direction")
                        .map(|s| s.as_str())
                        .unwrap_or("vertical")
                        .to_owned();
                    let is_vertical = direction != "horizontal";
                    let pos: f64 = if is_vertical {
                        prop_map
                            .get("y")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0)
                    } else {
                        prop_map
                            .get("x")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0)
                    };
                    let track_size: f64 = if is_vertical {
                        prop_map
                            .get("height")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1.0)
                    } else {
                        prop_map
                            .get("width")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1.0)
                    };

                    let content_size = state.content_size;
                    let viewport_size = state.viewport_size;
                    let max_scroll = (content_size - viewport_size).max(0.0);

                    let new_scroll = if track_size > 0.0 {
                        (pos / track_size * max_scroll).clamp(0.0, max_scroll)
                    } else {
                        0.0
                    };

                    // Extract what we need before releasing the mutable borrow
                    let props_clone = state.props.clone();
                    let _ = state;

                    // Find the parent scroll-view to update
                    let scroll_view_qid = find_parent_scroll_view(&self.daemon, source_qid);

                    // Update scrollbar state
                    let mut props_update = props_clone;
                    props_update.insert("scroll-position".to_string(), new_scroll.to_string());
                    let state_mut = self.daemon.get_mut(source_qid).unwrap();
                    state_mut.props = props_update;

                    let daemon = &self.daemon;
                    let state = daemon.get(source_qid).unwrap();
                    let mut em = Emitter::new(&mut self.stdout);
                    return em.frame(|em| {
                        byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                        emit_visual_patch(em, state)?;
                        // Also patch parent scroll-view viewport if found
                        if let Some(sv_qid) = &scroll_view_qid
                            && let Some(sv_state) = daemon.get(sv_qid)
                        {
                            let sv_id = &sv_state.local_id;
                            if is_vertical {
                                byo_write!(em,
                                    @view {format!("{sv_id}-viewport")}
                                        scroll-y={new_scroll.to_string()}
                                )?;
                            } else {
                                byo_write!(em,
                                    @view {format!("{sv_id}-viewport")}
                                        scroll-x={new_scroll.to_string()}
                                )?;
                            }
                        }
                        Ok(())
                    });
                }
                ScrollbarTarget::Other => {}
            }
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        state.pressed = true;
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

    fn on_pointer_up(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
        expansion_id: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get_mut(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };
        if state.disabled {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        // Scrollbar thumb release
        if state.kind == ControlKind::Scrollbar {
            let was_thumb = matches!(
                Self::scrollbar_target_kind(state, expansion_id),
                ScrollbarTarget::Thumb
            );
            let direction = state
                .props
                .get("direction")
                .map(|s| s.as_str())
                .unwrap_or("vertical")
                .to_owned();
            if was_thumb {
                state.thumb_pressed = false;
            }
            let scroll_view_qid = if was_thumb {
                find_parent_scroll_view(&self.daemon, source_qid)
            } else {
                None
            };
            let daemon = &self.daemon;
            let state = daemon.get(source_qid).unwrap();
            let mut em = Emitter::new(&mut self.stdout);
            return em.frame(|em| {
                byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
                emit_visual_patch(em, state)?;
                // Release controlled scroll on parent viewport
                if let Some(ref sv_qid) = scroll_view_qid
                    && let Some(sv_state) = daemon.get(sv_qid)
                {
                    let sv_id = &sv_state.local_id;
                    let is_vertical = direction != "horizontal";
                    if is_vertical {
                        byo_write!(em, @view {format!("{sv_id}-viewport")} ~scroll-y)?;
                    } else {
                        byo_write!(em, @view {format!("{sv_id}-viewport")} ~scroll-x)?;
                    }
                }
                Ok(())
            });
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
        expansion_id: &str,
    ) -> io::Result<()> {
        let state = match self.daemon.get(source_qid) {
            Some(s) => s,
            None => return ack_only(&mut self.stdout, ack_kind, ack_seq),
        };

        // Scrollbar thumb drag
        if state.kind == ControlKind::Scrollbar && state.thumb_pressed {
            return self.handle_scrollbar_thumb_drag(ack_kind, ack_seq, source_qid, props);
        }

        let _ = expansion_id; // used for scrollbar above
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
            ControlKind::Slider | ControlKind::Scrollbar | ControlKind::ScrollView => {
                ack_only(&mut self.stdout, ack_kind, ack_seq)?;
            }
        }
        Ok(())
    }

    fn handle_scrollbar_thumb_drag(
        &mut self,
        ack_kind: &str,
        ack_seq: u64,
        source_qid: &str,
        props: &[Prop],
    ) -> io::Result<()> {
        let prop_map = props_to_map(props);
        let state = self.daemon.get(source_qid).unwrap();

        let direction = state
            .props
            .get("direction")
            .map(|s| s.as_str())
            .unwrap_or("vertical");
        let is_vertical = direction != "horizontal";
        // Use viewport-relative coords (stable during capture)
        let current_pos: f64 = if is_vertical {
            prop_map
                .get("client-y")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0)
        } else {
            prop_map
                .get("client-x")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0)
        };

        let content_size = state.content_size;
        let viewport_size = state.viewport_size;
        let drag_start_pos = state.drag_start_pos;
        let drag_start_scroll = state.drag_start_scroll;

        let max_scroll = (content_size - viewport_size).max(0.0);

        // Use the track size captured at drag start (stable, not affected by capture)
        let track_size = state.drag_track_size;

        // Compute scroll-per-pixel ratio
        let thumb_pct = if content_size > 0.0 {
            (viewport_size / content_size).clamp(0.05, 1.0)
        } else {
            1.0
        };
        let thumb_size = track_size * thumb_pct;
        let scrollable_track = track_size - thumb_size;

        let new_scroll = if scrollable_track > 0.0 {
            let scroll_per_pixel = max_scroll / scrollable_track;
            let delta = current_pos - drag_start_pos;
            (drag_start_scroll + delta * scroll_per_pixel).clamp(0.0, max_scroll)
        } else {
            0.0
        };

        // Update scrollbar state
        let state = self.daemon.get_mut(source_qid).unwrap();
        let mut props_update = state.props.clone();
        props_update.insert("scroll-position".to_string(), new_scroll.to_string());
        state.props = props_update;

        // Find parent scroll-view to update viewport
        let scroll_view_qid = find_parent_scroll_view(&self.daemon, source_qid);

        // Also update the scroll-view state
        if let Some(ref sv_qid) = scroll_view_qid
            && let Some(sv_state) = self.daemon.get_mut(sv_qid)
        {
            if is_vertical {
                sv_state.scroll_y = new_scroll;
            } else {
                sv_state.scroll_x = new_scroll;
            }
        }

        let daemon = &self.daemon;
        let state = daemon.get(source_qid).unwrap();
        info!(
            "thumb_drag: scrollbar={source_qid} content={content_size:.0} viewport={viewport_size:.0} \
             track={track_size:.0} drag_start={drag_start_pos:.1} current={current_pos:.1} \
             start_scroll={drag_start_scroll:.1} new_scroll={new_scroll:.1}",
        );
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;
            emit_visual_patch(em, state)?;
            if let Some(sv_qid) = &scroll_view_qid
                && let Some(sv_state) = daemon.get(sv_qid)
            {
                let sv_id = &sv_state.local_id;
                if is_vertical {
                    byo_write!(em,
                        @view {format!("{sv_id}-viewport")}
                            scroll-y={new_scroll.to_string()}
                    )?;
                } else {
                    byo_write!(em,
                        @view {format!("{sv_id}-viewport")}
                            scroll-x={new_scroll.to_string()}
                    )?;
                }
            }
            Ok(())
        })
    }

    fn on_scroll(
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
        if state.kind != ControlKind::ScrollView {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        // The compositor owns scroll state — read the authoritative clamped
        // position and dimensions from the event props.
        let prop_map = props_to_map(props);
        let scroll_y: f64 = prop_map
            .get("scroll-y")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let scroll_x: f64 = prop_map
            .get("scroll-x")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let content_height: f64 = prop_map
            .get("content-height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let content_width: f64 = prop_map
            .get("content-width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let viewport_height: f64 = prop_map
            .get("viewport-height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let viewport_width: f64 = prop_map
            .get("viewport-width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let overflow_y: f64 = prop_map
            .get("scroll-overflow-y")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let overflow_x: f64 = prop_map
            .get("scroll-overflow-x")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        let direction = state
            .props
            .get("direction")
            .map(|s| s.as_str())
            .unwrap_or("vertical");
        let scroll_y_enabled = direction == "vertical" || direction == "both";
        let scroll_x_enabled = direction == "horizontal" || direction == "both";

        let max_y = (content_height - viewport_height).max(0.0);
        let max_x = (content_width - viewport_width).max(0.0);

        // Check if we're at bounds (for nested scroll propagation)
        let delta_y: f64 = prop_map
            .get("delta-y")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let delta_x: f64 = prop_map
            .get("delta-x")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let at_bounds = if scroll_y_enabled {
            (delta_y > 0.0 && scroll_y <= 0.0) || (delta_y < 0.0 && scroll_y >= max_y)
        } else if scroll_x_enabled {
            (delta_x > 0.0 && scroll_x <= 0.0) || (delta_x < 0.0 && scroll_x >= max_x)
        } else {
            true
        };

        info!(
            "on_scroll: scroll_y={scroll_y:.1} scroll_x={scroll_x:.1} content={content_width:.0}x{content_height:.0} viewport={viewport_width:.0}x{viewport_height:.0} at_bounds={at_bounds}"
        );

        let wants_scroll = state.app_wants_event(&EventKind::Scroll);
        let id = state.local_id.clone();

        // Update scroll-view state from compositor-authoritative position
        let state = self.daemon.get_mut(source_qid).unwrap();
        state.scroll_y = scroll_y;
        state.scroll_x = scroll_x;

        let scroll_seq = if wants_scroll {
            Some(self.daemon.next_seq("scroll"))
        } else {
            None
        };

        // Update scrollbar internal state directly (avoids re-expand via @scrollbar)
        let (sb_y_qid, sb_x_qid) = find_child_scrollbars(&self.daemon, &id);
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            sb.content_size = content_height;
            sb.viewport_size = viewport_height;
            sb.scroll_overflow = overflow_y;
            sb.props
                .insert("content-size".to_string(), content_height.to_string());
            sb.props
                .insert("viewport-size".to_string(), viewport_height.to_string());
            sb.props
                .insert("scroll-position".to_string(), scroll_y.to_string());
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            sb.content_size = content_width;
            sb.viewport_size = viewport_width;
            sb.scroll_overflow = overflow_x;
            sb.props
                .insert("content-size".to_string(), content_width.to_string());
            sb.props
                .insert("viewport-size".to_string(), viewport_width.to_string());
            sb.props
                .insert("scroll-position".to_string(), scroll_x.to_string());
        }

        // Fade-in: make modern scrollbars visible on scroll and arm hide timer
        let mut fade_y_timer: Option<String> = None;
        let mut fade_x_timer: Option<String> = None;
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            if resolve_scrollbar_style(sb) == "modern" {
                sb.fade_visible = true;
                fade_y_timer = Some(format!("{}-fade", sb.local_id));
            }
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            if resolve_scrollbar_style(sb) == "modern" {
                sb.fade_visible = true;
                fade_x_timer = Some(format!("{}-fade", sb.local_id));
            }
        }

        // Get immutable ref for emission
        let daemon = &self.daemon;

        let handled = if at_bounds { "false" } else { "true" };

        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled={handled})?;

            // No viewport scroll-position patch — compositor owns ScrollPosition
            // directly via apply_overscroll_offset system.

            // Patch scrollbar visuals directly (compositor-native @view, no re-expand)
            if scroll_y_enabled
                && let Some(ref qid) = sb_y_qid
                && let Some(sb_state) = daemon.get(qid)
            {
                patch_scrollbar_state(em, sb_state)?;
            }
            if scroll_x_enabled
                && let Some(ref qid) = sb_x_qid
                && let Some(sb_state) = daemon.get(qid)
            {
                patch_scrollbar_state(em, sb_state)?;
            }

            // Arm fade-out timers for modern scrollbars
            if let Some(ref timer_id) = fade_y_timer {
                byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
            }
            if let Some(ref timer_id) = fade_x_timer {
                byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
            }

            // Release any controlled scroll props from a prior track click.
            // This is harmless if the props aren't set (removing absent prop is a no-op).
            if scroll_y_enabled {
                byo_write!(em, @view {format!("{id}-viewport")} ~scroll-y)?;
            }
            if scroll_x_enabled {
                byo_write!(em, @view {format!("{id}-viewport")} ~scroll-x)?;
            }

            // Forward passive scroll event to app
            if let Some(seq) = scroll_seq {
                byo_write!(em,
                    !scroll {seq} {source_qid}
                        scroll-x={scroll_x.to_string()}
                        scroll-y={scroll_y.to_string()}
                )?;
            }

            Ok(())
        })
    }

    fn on_resize(
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
        if state.kind != ControlKind::ScrollView {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        let prop_map = props_to_map(props);
        let viewport_height: f64 = prop_map
            .get("height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let viewport_width: f64 = prop_map
            .get("width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let content_height: f64 = prop_map
            .get("content-height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let content_width: f64 = prop_map
            .get("content-width")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        info!(
            "on_resize: viewport={viewport_width:.0}x{viewport_height:.0} content={content_width:.0}x{content_height:.0} scroll_y={:.1} scroll_x={:.1}",
            state.scroll_y, state.scroll_x
        );

        let direction = state
            .props
            .get("direction")
            .map(|s| s.as_str())
            .unwrap_or("vertical");
        let scroll_y_enabled = direction == "vertical" || direction == "both";
        let scroll_x_enabled = direction == "horizontal" || direction == "both";

        let id = state.local_id.clone();

        let wants_resize = state.app_wants_event(&EventKind::Resize);
        let resize_seq = if wants_resize {
            Some(self.daemon.next_seq("resize"))
        } else {
            None
        };

        // Update scrollbar internal state directly (avoids re-expand via @scrollbar).
        // Find child scrollbar QIDs and update their content_size/viewport_size + props.
        let (sb_y_qid, sb_x_qid) = find_child_scrollbars(&self.daemon, &id);
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            sb.content_size = content_height;
            sb.viewport_size = viewport_height;
            sb.props
                .insert("content-size".to_string(), content_height.to_string());
            sb.props
                .insert("viewport-size".to_string(), viewport_height.to_string());
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            sb.content_size = content_width;
            sb.viewport_size = viewport_width;
            sb.props
                .insert("content-size".to_string(), content_width.to_string());
            sb.props
                .insert("viewport-size".to_string(), viewport_width.to_string());
        }

        // Fade-in: make modern scrollbars visible on resize and arm hide timer
        let mut fade_y_timer: Option<String> = None;
        let mut fade_x_timer: Option<String> = None;
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            if resolve_scrollbar_style(sb) == "modern" {
                sb.fade_visible = true;
                fade_y_timer = Some(format!("{}-fade", sb.local_id));
            }
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            let sb = self.daemon.get_mut(qid).unwrap();
            if resolve_scrollbar_style(sb) == "modern" {
                sb.fade_visible = true;
                fade_x_timer = Some(format!("{}-fade", sb.local_id));
            }
        }

        // Emit visual patches directly (compositor-native @view, no re-expand)
        let daemon = &self.daemon;
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;

            if scroll_y_enabled
                && let Some(ref qid) = sb_y_qid
                && let Some(sb_state) = daemon.get(qid)
            {
                patch_scrollbar_state(em, sb_state)?;
            }
            if scroll_x_enabled
                && let Some(ref qid) = sb_x_qid
                && let Some(sb_state) = daemon.get(qid)
            {
                patch_scrollbar_state(em, sb_state)?;
            }

            // Arm fade-out timers for modern scrollbars
            if let Some(ref timer_id) = fade_y_timer {
                byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
            }
            if let Some(ref timer_id) = fade_x_timer {
                byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
            }

            // Forward resize event to app if subscribed
            if let Some(seq) = resize_seq {
                byo_write!(em,
                    !resize {seq} {source_qid}
                        width={viewport_width.to_string()}
                        height={viewport_height.to_string()}
                        content-width={content_width.to_string()}
                        content-height={content_height.to_string()}
                )?;
            }

            Ok(())
        })
    }

    fn on_fire(&mut self, seq: u64, id: &str) -> io::Result<()> {
        // Timer IDs follow the convention: {scrollbar_local_id}-fade
        let scrollbar_local_id = match id.strip_suffix("-fade") {
            Some(s) => s,
            None => {
                info!("  fire: unknown timer {id}, ACK-only");
                let mut em = Emitter::new(&mut self.stdout);
                return em.frame(|em| byo_write!(em, !ack fire {seq} handled=true));
            }
        };

        // Find the scrollbar by local_id
        let source_qid = self
            .daemon
            .controls
            .iter()
            .find(|(_, state)| {
                state.kind == ControlKind::Scrollbar && state.local_id == scrollbar_local_id
            })
            .map(|(qid, _)| qid.clone());

        let source_qid = match source_qid {
            Some(qid) => qid,
            None => {
                info!("  fire: no scrollbar for timer {id}");
                let mut em = Emitter::new(&mut self.stdout);
                return em.frame(|em| byo_write!(em, !ack fire {seq} handled=true));
            }
        };

        let state = self.daemon.get_mut(&source_qid).unwrap();

        // If the scrollbar is currently hovered, re-arm the timer instead of hiding
        if state.thumb_hover || state.track_hover || state.thumb_pressed {
            let timer_id = id;
            let mut em = Emitter::new(&mut self.stdout);
            return em.frame(|em| {
                byo_write!(em, !ack fire {seq} handled=true)?;
                byo_write!(em, +timer {timer_id} delay=2000)
            });
        }

        // Fade out: set invisible and patch
        state.fade_visible = false;

        let daemon = &self.daemon;
        let state = daemon.get(&source_qid).unwrap();
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack fire {seq} handled=true)?;
            emit_visual_patch(em, state)
        })
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
        ControlKind::Scrollbar => patch_scrollbar_state(em, state),
        ControlKind::ScrollView => patch_scroll_view_state(em, state),
    }
}

/// Find the parent scroll-view for a scrollbar.
///
/// The scrollbar's source_qid is like "controls:content-scrollbar-y", and
/// the parent scroll-view has source_qid like "app:content". We look for
/// a scroll-view whose local_id matches the scrollbar's local_id prefix
/// before "-scrollbar-y" or "-scrollbar-x".
fn find_parent_scroll_view(daemon: &Daemon, scrollbar_qid: &str) -> Option<String> {
    let scrollbar_state = daemon.get(scrollbar_qid)?;
    let scrollbar_local = &scrollbar_state.local_id;

    // The scrollbar local_id is "{parent_id}-scrollbar-{axis}"
    let parent_suffix = scrollbar_local
        .strip_suffix("-scrollbar-y")
        .or_else(|| scrollbar_local.strip_suffix("-scrollbar-x"))?;

    // Search all scroll-views for one whose local_id matches
    for (qid, state) in &daemon.controls {
        if state.kind == ControlKind::ScrollView && state.local_id == parent_suffix {
            return Some(qid.clone());
        }
    }
    None
}

/// Find child scrollbar QIDs for a scroll-view by its local_id.
/// Returns (y_scrollbar_qid, x_scrollbar_qid).
fn find_child_scrollbars(
    daemon: &Daemon,
    scroll_view_local_id: &str,
) -> (Option<String>, Option<String>) {
    let mut y_qid = None;
    let mut x_qid = None;
    let y_local = format!("{scroll_view_local_id}-scrollbar-y");
    let x_local = format!("{scroll_view_local_id}-scrollbar-x");
    for (qid, state) in &daemon.controls {
        if state.kind == ControlKind::Scrollbar {
            if state.local_id == y_local {
                y_qid = Some(qid.clone());
            } else if state.local_id == x_local {
                x_qid = Some(qid.clone());
            }
        }
    }
    (y_qid, x_qid)
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
        em.frame(|em| byo_write!(em, #claim button, checkbox, slider, scrollbar, scroll-view))
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
