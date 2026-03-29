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

use byo::byo_read_props;
use byo::byo_write;
use byo::emitter::Emitter;
use byo::parser::parse;
use byo::protocol::{Command, EventKind, MessageKind, Prop, RequestKind};
use byo::scanner::{Handler, Scanner};
use tracing::info;

use crate::expand::{
    expand_button, expand_checkbox, expand_scroll_view, expand_scrollbar, expand_slider,
    patch_button_state, patch_checkbox_state, patch_scroll_view_state, patch_scrollbar_state,
    patch_slider_state, resolve_scrollbar_style,
};
use crate::state::{ControlKind, ControlState, Daemon, KindState};

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
            Command::Message {
                kind: kind @ (MessageKind::ScrollTo | MessageKind::ScrollBy),
                target,
                props,
                ..
            } => {
                self.handle_scroll_message(&kind, &target, &props)?;
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
    fn scrollbar_target_kind(local_id: &str, expansion_id: &str) -> ScrollbarTarget {
        match expansion_id.strip_prefix(local_id) {
            Some("-thumb") => ScrollbarTarget::Thumb,
            Some("-track") => ScrollbarTarget::Track,
            _ => ScrollbarTarget::Other,
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
        match &mut state.kind_state {
            KindState::Scrollbar {
                thumb_hover,
                track_hover,
                fade_visible,
                ..
            } => {
                match Self::scrollbar_target_kind(&state.local_id, expansion_id) {
                    ScrollbarTarget::Thumb => *thumb_hover = true,
                    ScrollbarTarget::Track => *track_hover = true,
                    ScrollbarTarget::Other => {}
                }
                // Fade-in on hover and cancel any pending fade-out timer
                if resolve_scrollbar_style(&state.props) == "modern" {
                    *fade_visible = true;
                    fade_cancel_timer = Some(format!("{}-fade", state.local_id));
                }
            }
            KindState::Button { hover, .. }
            | KindState::Checkbox { hover, .. }
            | KindState::Slider { hover, .. } => {
                *hover = true;
            }
            _ => {}
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
        match &mut state.kind_state {
            KindState::Scrollbar {
                thumb_hover,
                track_hover,
                ..
            } => {
                match Self::scrollbar_target_kind(&state.local_id, expansion_id) {
                    ScrollbarTarget::Thumb => {
                        *thumb_hover = false;
                        // Don't clear thumb_pressed here — it's cleared on PointerUp.
                        // With pointer capture, PointerLeave fires when the cursor
                        // exits the thumb bounds but drag should continue.
                    }
                    ScrollbarTarget::Track => *track_hover = false,
                    ScrollbarTarget::Other => {}
                }
                // Arm fade-out timer on leave (fade_visible stays true until timer fires)
                if resolve_scrollbar_style(&state.props) == "modern" {
                    fade_arm_timer = Some(format!("{}-fade", state.local_id));
                }
            }
            KindState::Button { hover, pressed }
            | KindState::Checkbox { hover, pressed, .. }
            | KindState::Slider { hover, pressed, .. } => {
                *hover = false;
                *pressed = false;
            }
            _ => {}
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

        let is_slider = state.kind() == ControlKind::Slider;
        let is_scrollbar = state.kind() == ControlKind::Scrollbar;

        if is_scrollbar {
            let target = Self::scrollbar_target_kind(&state.local_id, expansion_id);
            match target {
                ScrollbarTarget::Thumb => {
                    byo_read_props!(props,
                        cy: f64 = "client-y",
                        cx: f64 = "client-x",
                        height: f64 = "height" | 1.0,
                        width: f64 = "width" | 1.0,
                    );
                    let is_vertical = state.is_vertical();
                    let scroll_pos = get_f64(&state.props, "scroll-position", 0.0);
                    let thumb_pixels = if is_vertical { height } else { width };
                    let KindState::Scrollbar {
                        thumb_pressed,
                        drag_start_pos,
                        drag_start_scroll,
                        drag_track_size,
                        content_size,
                        viewport_size,
                        ..
                    } = &mut state.kind_state
                    else {
                        unreachable!()
                    };
                    *thumb_pressed = true;
                    *drag_start_pos = if is_vertical { cy } else { cx };
                    *drag_start_scroll = scroll_pos;
                    let thumb_pct = if *content_size > 0.0 {
                        (*viewport_size / *content_size).clamp(0.05, 1.0)
                    } else {
                        1.0
                    };
                    *drag_track_size = if thumb_pct > 0.0 {
                        thumb_pixels / thumb_pct
                    } else {
                        thumb_pixels
                    };

                    info!(
                        "thumb_down: scrollbar={source_qid} content={:.0} viewport={:.0} \
                         thumb_px={thumb_pixels:.0} thumb_pct={thumb_pct:.3} track={:.0} \
                         scroll_pos={:.1} drag_start={:.1}",
                        *content_size,
                        *viewport_size,
                        *drag_track_size,
                        *drag_start_scroll,
                        *drag_start_pos,
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
                    byo_read_props!(props,
                        y: f64 = "y",
                        x: f64 = "x",
                        height: f64 = "height" | 1.0,
                        width: f64 = "width" | 1.0,
                    );
                    let is_vertical = state.is_vertical();
                    let pos = if is_vertical { y } else { x };
                    let track_size = if is_vertical { height } else { width };

                    let KindState::Scrollbar {
                        content_size,
                        viewport_size,
                        ..
                    } = &state.kind_state
                    else {
                        unreachable!()
                    };
                    let content_size = *content_size;
                    let viewport_size = *viewport_size;
                    let max_scroll = (content_size - viewport_size).max(0.0);

                    let new_scroll = if track_size > 0.0 {
                        (pos / track_size * max_scroll).clamp(0.0, max_scroll)
                    } else {
                        0.0
                    };

                    // Update scrollbar state
                    state
                        .props
                        .insert("scroll-position".to_string(), new_scroll.to_string());

                    // Find the parent scroll-view to update
                    let scroll_view_qid =
                        self.daemon.scrollbar_parent(source_qid).map(str::to_owned);

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

        match &mut state.kind_state {
            KindState::Button { pressed, .. }
            | KindState::Checkbox { pressed, .. }
            | KindState::Slider { pressed, .. } => *pressed = true,
            _ => {}
        }
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
        if state.kind() == ControlKind::Scrollbar {
            let was_thumb = matches!(
                Self::scrollbar_target_kind(&state.local_id, expansion_id),
                ScrollbarTarget::Thumb
            );
            let is_vertical = state.is_vertical();
            if was_thumb && let KindState::Scrollbar { thumb_pressed, .. } = &mut state.kind_state {
                *thumb_pressed = false;
            }
            let scroll_view_qid = if was_thumb {
                self.daemon.scrollbar_parent(source_qid).map(str::to_owned)
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
                    if is_vertical {
                        byo_write!(em, @view {format!("{sv_id}-viewport")} ~scroll-y)?;
                    } else {
                        byo_write!(em, @view {format!("{sv_id}-viewport")} ~scroll-x)?;
                    }
                }
                Ok(())
            });
        }

        let was_slider_pressed = state.pressed() && state.kind() == ControlKind::Slider;
        let value = state.value();
        let wants_change = state.app_wants_event(&EventKind::Change);
        match &mut state.kind_state {
            KindState::Button { pressed, .. }
            | KindState::Checkbox { pressed, .. }
            | KindState::Slider { pressed, .. } => *pressed = false,
            _ => {}
        }
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
        if matches!(
            state.kind_state,
            KindState::Scrollbar {
                thumb_pressed: true,
                ..
            }
        ) {
            return self.handle_scrollbar_thumb_drag(ack_kind, ack_seq, source_qid, props);
        }

        let _ = expansion_id; // used for scrollbar above
        if state.disabled || !state.pressed() || state.kind() != ControlKind::Slider {
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
        match state.kind() {
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
        byo_read_props!(props,
            client_y: f64 = "client-y",
            client_x: f64 = "client-x",
        );
        let state = self.daemon.get(source_qid).unwrap();

        let is_vertical = state.is_vertical();
        let current_pos = if is_vertical { client_y } else { client_x };

        let KindState::Scrollbar {
            content_size,
            viewport_size,
            drag_start_pos,
            drag_start_scroll,
            drag_track_size,
            ..
        } = &state.kind_state
        else {
            unreachable!()
        };
        let content_size = *content_size;
        let viewport_size = *viewport_size;
        let drag_start_pos = *drag_start_pos;
        let drag_start_scroll = *drag_start_scroll;

        let max_scroll = (content_size - viewport_size).max(0.0);

        // Use the track size captured at drag start (stable, not affected by capture)
        let track_size = *drag_track_size;

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
        state
            .props
            .insert("scroll-position".to_string(), new_scroll.to_string());

        // Find parent scroll-view to update viewport
        let scroll_view_qid = self.daemon.scrollbar_parent(source_qid).map(str::to_owned);

        // Also update the scroll-view state
        if let Some(ref sv_qid) = scroll_view_qid
            && let Some(sv_state) = self.daemon.get_mut(sv_qid)
            && let KindState::ScrollView { scroll_x, scroll_y } = &mut sv_state.kind_state
        {
            if is_vertical {
                *scroll_y = new_scroll;
            } else {
                *scroll_x = new_scroll;
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
        if state.kind() != ControlKind::ScrollView {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        // The compositor owns scroll state — read the authoritative clamped
        // position and dimensions from the event props.
        byo_read_props!(props,
            scroll_y: f64 = "scroll-y",
            scroll_x: f64 = "scroll-x",
            content_height: f64 = "content-height",
            content_width: f64 = "content-width",
            viewport_height: f64 = "viewport-height",
            viewport_width: f64 = "viewport-width",
            overflow_y: f64 = "scroll-overflow-y",
            overflow_x: f64 = "scroll-overflow-x",
            delta_y: f64 = "delta-y",
            delta_x: f64 = "delta-x",
        );

        let (scroll_y_enabled, scroll_x_enabled) = state.scroll_axes();

        let max_y = (content_height - viewport_height).max(0.0);
        let max_x = (content_width - viewport_width).max(0.0);
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
        if let KindState::ScrollView {
            scroll_x: sx,
            scroll_y: sy,
        } = &mut state.kind_state
        {
            *sy = scroll_y;
            *sx = scroll_x;
        }

        let scroll_seq = if wants_scroll {
            Some(self.daemon.next_seq("scroll"))
        } else {
            None
        };

        // Update scrollbar internal state directly (avoids re-expand via @scrollbar)
        let (sb_y_qid, sb_x_qid) = self.daemon.scroll_view_children(source_qid);
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            update_scrollbar_dimensions(
                &mut self.daemon,
                qid,
                content_height,
                viewport_height,
                Some(scroll_y),
                Some(overflow_y),
            );
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            update_scrollbar_dimensions(
                &mut self.daemon,
                qid,
                content_width,
                viewport_width,
                Some(scroll_x),
                Some(overflow_x),
            );
        }

        // Fade-in: make modern scrollbars visible on scroll and arm hide timer
        let axes = ScrollbarAxes {
            fade_y_timer: if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
                fade_in_scrollbar(&mut self.daemon, qid)
            } else {
                None
            },
            fade_x_timer: if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
                fade_in_scrollbar(&mut self.daemon, qid)
            } else {
                None
            },
            sb_y_qid,
            sb_x_qid,
            scroll_y_enabled,
            scroll_x_enabled,
        };

        let daemon = &self.daemon;
        let handled = if at_bounds { "false" } else { "true" };

        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled={handled})?;

            emit_scrollbar_patches_and_timers(em, daemon, &axes)?;

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
        if state.kind() != ControlKind::ScrollView {
            return ack_only(&mut self.stdout, ack_kind, ack_seq);
        }

        byo_read_props!(props,
            viewport_height: f64 = "height",
            viewport_width: f64 = "width",
            content_height: f64 = "content-height",
            content_width: f64 = "content-width",
        );

        let (sv_scroll_y, sv_scroll_x) = match &state.kind_state {
            KindState::ScrollView { scroll_x, scroll_y } => (*scroll_y, *scroll_x),
            _ => (0.0, 0.0),
        };
        info!(
            "on_resize: viewport={viewport_width:.0}x{viewport_height:.0} content={content_width:.0}x{content_height:.0} scroll_y={sv_scroll_y:.1} scroll_x={sv_scroll_x:.1}"
        );

        let (scroll_y_enabled, scroll_x_enabled) = state.scroll_axes();

        let wants_resize = state.app_wants_event(&EventKind::Resize);
        let resize_seq = if wants_resize {
            Some(self.daemon.next_seq("resize"))
        } else {
            None
        };

        // Update scrollbar internal state directly (avoids re-expand via @scrollbar).
        let (sb_y_qid, sb_x_qid) = self.daemon.scroll_view_children(source_qid);
        if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
            update_scrollbar_dimensions(
                &mut self.daemon,
                qid,
                content_height,
                viewport_height,
                None,
                None,
            );
        }
        if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
            update_scrollbar_dimensions(
                &mut self.daemon,
                qid,
                content_width,
                viewport_width,
                None,
                None,
            );
        }

        // Fade-in: make modern scrollbars visible on resize and arm hide timer
        let axes = ScrollbarAxes {
            fade_y_timer: if scroll_y_enabled && let Some(ref qid) = sb_y_qid {
                fade_in_scrollbar(&mut self.daemon, qid)
            } else {
                None
            },
            fade_x_timer: if scroll_x_enabled && let Some(ref qid) = sb_x_qid {
                fade_in_scrollbar(&mut self.daemon, qid)
            } else {
                None
            },
            sb_y_qid,
            sb_x_qid,
            scroll_y_enabled,
            scroll_x_enabled,
        };

        let daemon = &self.daemon;
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| {
            byo_write!(em, !ack {ack_kind} {ack_seq} handled=true)?;

            emit_scrollbar_patches_and_timers(em, daemon, &axes)?;

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
                matches!(state.kind_state, KindState::Scrollbar { .. })
                    && state.local_id == scrollbar_local_id
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
        if state.is_scrollbar_active() {
            let timer_id = id;
            let mut em = Emitter::new(&mut self.stdout);
            return em.frame(|em| {
                byo_write!(em, !ack fire {seq} handled=true)?;
                byo_write!(em, +timer {timer_id} delay=2000)
            });
        }

        // Fade out: set invisible and patch
        if let KindState::Scrollbar { fade_visible, .. } = &mut state.kind_state {
            *fade_visible = false;
        }

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
        let checked = state.checked().unwrap_or(false);
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
            if let KindState::Checkbox { checked, .. } = &mut state.kind_state {
                *checked = Some(new_checked);
            }

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
        byo_read_props!(props, x: f64 = "x", width: f64 = "width");

        let state = self.daemon.get(source_qid).unwrap();
        let is_controlled = state.is_controlled_slider();
        let min = state.slider_min();
        let max = state.slider_max();
        let wants_input = state.app_wants_event(&EventKind::Input);
        let prev_value = state.value();

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
            if let KindState::Slider { value, .. } = &mut state.kind_state {
                *value = Some(snapped);
            }
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

    /// Handle `.scroll-to` / `.scroll-by` messages on a scroll-view.
    ///
    /// Forwards the message to the viewport view. The compositor handles
    /// the actual scroll, clamping, and emits `!scroll` with authoritative
    /// position/dimensions — which updates the scrollbar via the existing path.
    fn handle_scroll_message(
        &mut self,
        kind: &MessageKind,
        target: &byo::ByteStr,
        props: &[Prop],
    ) -> io::Result<()> {
        let target_str = target.as_ref();

        let state = match self.daemon.get(target_str) {
            Some(s) if s.kind() == ControlKind::ScrollView => s,
            _ => {
                tracing::warn!("scroll message for unknown scroll-view {target_str}");
                return Ok(());
            }
        };

        let viewport_id = format!("{}-viewport", state.local_id);
        let mut em = Emitter::new(&mut self.stdout);
        em.frame(|em| em.message(kind.as_str(), &viewport_id, props))
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
    match state.kind() {
        ControlKind::Button => patch_button_state(em, state),
        ControlKind::Checkbox => patch_checkbox_state(em, state),
        ControlKind::Slider => patch_slider_state(em, state),
        ControlKind::Scrollbar => patch_scrollbar_state(em, state),
        ControlKind::ScrollView => patch_scroll_view_state(em, state),
    }
}

/// Update a child scrollbar's content/viewport dimensions and optionally scroll position.
fn update_scrollbar_dimensions(
    daemon: &mut Daemon,
    qid: &str,
    content_size: f64,
    viewport_size: f64,
    scroll_position: Option<f64>,
    overflow: Option<f64>,
) {
    let sb = daemon.get_mut(qid).unwrap();
    if let KindState::Scrollbar {
        content_size: cs,
        viewport_size: vs,
        scroll_overflow: so,
        ..
    } = &mut sb.kind_state
    {
        *cs = content_size;
        *vs = viewport_size;
        if let Some(overflow) = overflow {
            *so = overflow;
        }
    }
    sb.props
        .insert("content-size".to_string(), content_size.to_string());
    sb.props
        .insert("viewport-size".to_string(), viewport_size.to_string());
    if let Some(pos) = scroll_position {
        sb.props
            .insert("scroll-position".to_string(), pos.to_string());
    }
}

/// Fade-in a modern scrollbar and return its timer ID (for arming the fade-out timer).
fn fade_in_scrollbar(daemon: &mut Daemon, qid: &str) -> Option<String> {
    let sb = daemon.get_mut(qid).unwrap();
    if sb.scrollbar_style() == "modern" {
        if let KindState::Scrollbar { fade_visible, .. } = &mut sb.kind_state {
            *fade_visible = true;
        }
        Some(format!("{}-fade", sb.local_id))
    } else {
        None
    }
}

/// Resolved scrollbar state for a scroll-view's two axes.
struct ScrollbarAxes {
    sb_y_qid: Option<String>,
    sb_x_qid: Option<String>,
    scroll_y_enabled: bool,
    scroll_x_enabled: bool,
    fade_y_timer: Option<String>,
    fade_x_timer: Option<String>,
}

/// Emit scrollbar patches and arm fade-out timers for the given axes.
fn emit_scrollbar_patches_and_timers<W: io::Write>(
    em: &mut Emitter<W>,
    daemon: &Daemon,
    axes: &ScrollbarAxes,
) -> io::Result<()> {
    if axes.scroll_y_enabled
        && let Some(qid) = &axes.sb_y_qid
        && let Some(sb_state) = daemon.get(qid)
    {
        patch_scrollbar_state(em, sb_state)?;
    }
    if axes.scroll_x_enabled
        && let Some(qid) = &axes.sb_x_qid
        && let Some(sb_state) = daemon.get(qid)
    {
        patch_scrollbar_state(em, sb_state)?;
    }
    if let Some(timer_id) = &axes.fade_y_timer {
        byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
    }
    if let Some(timer_id) = &axes.fade_x_timer {
        byo_write!(em, +timer {timer_id.as_str()} delay=2000)?;
    }
    Ok(())
}

/// Emit an ACK-only frame (no additional response).
fn ack_only(stdout: &mut Stdout, kind: &str, seq: u64) -> io::Result<()> {
    let mut em = Emitter::new(stdout);
    em.frame(|em| byo_write!(em, !ack {kind} {seq} handled=true))
}

/// Read an f64 from a prop map, returning a default if missing or unparseable.
fn get_f64(map: &HashMap<String, String>, key: &str, default: f64) -> f64 {
    map.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
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
        em.frame(|em| {
            byo_write!(em,
                #claim button, checkbox, slider, scrollbar, scroll-view
                #handle scroll-view?scroll-to, scroll-view?scroll-by
            )
        })
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
