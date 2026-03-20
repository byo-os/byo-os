//! Expansion logic — generates compositor-native view/text trees from control props.

use std::io;

use byo::byo_write;
use byo::emitter::Emitter;

use crate::state::ControlState;

/// Button variants and their base/hover/pressed class sets.
struct ButtonStyle {
    base_bg: &'static str,
    hover_bg: &'static str,
    pressed_bg: &'static str,
    text_color: &'static str,
}

fn button_style(variant: &str) -> ButtonStyle {
    match variant {
        "primary" => ButtonStyle {
            base_bg: "bg-blue-500",
            hover_bg: "bg-blue-400",
            pressed_bg: "bg-blue-600",
            text_color: "text-white",
        },
        "danger" => ButtonStyle {
            base_bg: "bg-red-500",
            hover_bg: "bg-red-400",
            pressed_bg: "bg-red-600",
            text_color: "text-white",
        },
        "ghost" => ButtonStyle {
            base_bg: "bg-transparent",
            hover_bg: "bg-zinc-800",
            pressed_bg: "bg-zinc-700",
            text_color: "text-zinc-300",
        },
        // "default" and anything else
        _ => ButtonStyle {
            base_bg: "bg-zinc-700",
            hover_bg: "bg-zinc-600",
            pressed_bg: "bg-zinc-800",
            text_color: "text-white",
        },
    }
}

fn button_size_classes(size: &str) -> (&'static str, &'static str) {
    // Returns (padding classes, text size class)
    match size {
        "sm" => ("px-3 py-1", "text-xs"),
        "lg" => ("px-6 py-3", "text-base"),
        // "md" and default
        _ => ("px-4 py-2", "text-sm"),
    }
}

/// Build the root class string for a button given its current state.
pub fn button_root_class(state: &ControlState) -> String {
    let variant = state
        .props
        .get("variant")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let size = state.props.get("size").map(|s| s.as_str()).unwrap_or("md");
    let style = button_style(variant);
    let (pad, _) = button_size_classes(size);

    let bg = if state.disabled {
        style.base_bg
    } else if state.pressed {
        style.pressed_bg
    } else if state.hover {
        style.hover_bg
    } else {
        style.base_bg
    };

    let mut class = format!(
        "inline-flex items-center justify-center rounded-lg {bg} {pad} transition-colors duration-150"
    );

    if state.disabled {
        class.push_str(" opacity-50");
    }

    class
}

/// Emit the expansion for a button.
pub fn expand_button<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    let id = &state.local_id;
    let label = state.props.get("label").map(|s| s.as_str()).unwrap_or("");
    let size = state.props.get("size").map(|s| s.as_str()).unwrap_or("md");
    let (_, text_size) = button_size_classes(size);
    let variant = state
        .props
        .get("variant")
        .map(|s| s.as_str())
        .unwrap_or("default");
    let style = button_style(variant);

    let root_class = button_root_class(state);

    let text_color = if state.disabled {
        format!("{}/40", style.text_color)
    } else {
        style.text_color.to_string()
    };
    let text_class = format!("{text_size} font-medium {text_color}");

    byo_write!(em,
        +view {format!("{id}-root")} class={root_class}
            if !state.disabled {
                events="pointerdown,pointerup,pointerenter,pointerleave,click"
                pointer-events=auto
            }
        {
            +text {format!("{id}-label")} content={label} class={text_class}
        }
    )
}

/// Build the checkbox box class string.
fn checkbox_box_class(checked: bool, hover: bool, disabled: bool) -> String {
    let mut class = String::from("w-5 h-5 rounded border-2 flex items-center justify-center");

    if checked {
        if disabled {
            class.push_str(" bg-blue-500 border-blue-500 opacity-50");
        } else if hover {
            class.push_str(" bg-blue-400 border-blue-400");
        } else {
            class.push_str(" bg-blue-500 border-blue-500");
        }
    } else if disabled {
        class.push_str(" border-zinc-500 opacity-50");
    } else if hover {
        class.push_str(" border-zinc-400");
    } else {
        class.push_str(" border-zinc-500");
    }

    class
}

/// Emit the expansion for a checkbox.
pub fn expand_checkbox<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    let id = &state.local_id;
    let label = state.props.get("label").map(|s| s.as_str()).unwrap_or("");
    let checked = state.checked.unwrap_or(false);

    let mut root_class = String::from("inline-flex items-center gap-2");
    if state.disabled {
        root_class.push_str(" opacity-50");
    }

    let box_class = checkbox_box_class(checked, state.hover, state.disabled);
    let check_content = if checked { "\u{2713}" } else { "" };

    let check_class = if state.disabled {
        "text-white/40 text-xs font-bold"
    } else {
        "text-white text-xs font-bold"
    };
    let label_class = if state.disabled {
        "text-sm text-white/40"
    } else {
        "text-sm text-white"
    };

    byo_write!(em,
        +view {format!("{id}-root")} class={root_class}
            if !state.disabled {
                events="pointerdown,pointerup,pointerenter,pointerleave,click"
                pointer-events=auto
            }
        {
            +view {format!("{id}-box")} class={box_class} {
                +text {format!("{id}-check")} content={check_content} class={check_class}
            }
            +text {format!("{id}-label")} content={label} class={label_class}
        }
    )
}

/// Emit the expansion for a slider.
pub fn expand_slider<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    let id = &state.local_id;
    let label = state.props.get("label").map(|s| s.as_str()).unwrap_or("");
    let value = state.value.unwrap_or(50.0);
    let pct = state.slider_pct();

    let mut root_class = String::from("flex flex-col gap-1");
    if state.disabled {
        root_class.push_str(" opacity-50");
    }

    let fill_bg = if state.disabled {
        "bg-blue-500"
    } else if state.pressed || state.hover {
        "bg-blue-400"
    } else {
        "bg-blue-500"
    };

    let value_display = format_value(value);

    let label_class = if state.disabled {
        "text-sm text-zinc-400/40"
    } else {
        "text-sm text-zinc-400"
    };
    let value_class = if state.disabled {
        "text-sm text-white/40"
    } else {
        "text-sm text-white"
    };

    byo_write!(em,
        +view {format!("{id}-root")} class={root_class} {
            +view {format!("{id}-header")} class="flex justify-between" {
                +text {format!("{id}-label")} content={label} class={label_class}
                +text {format!("{id}-value")} content={value_display.as_str()} class={value_class}
            }
            +view {format!("{id}-track")} class="h-2 rounded-full bg-zinc-700"
                if !state.disabled {
                    events="pointerdown,pointermove,pointerup"
                    pointer-events=auto
                }
            {
                +view {format!("{id}-fill")}
                    class={format!("h-full rounded-full {fill_bg}")}
                    width={format!("{pct:.1}%")}
            }
        }
    )
}

/// Format a numeric value for display: show integer if whole, else one decimal.
fn format_value(v: f64) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Emit a patch for button hover/press state change (lightweight, no re-expand).
pub fn patch_button_state<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let class = button_root_class(state);
    byo_write!(em, @view {format!("{id}-root")} class={class})
}

/// Emit a patch for checkbox visual state (hover, checked).
pub fn patch_checkbox_state<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let checked = state.checked.unwrap_or(false);
    let box_class = checkbox_box_class(checked, state.hover, state.disabled);
    let check_content = if checked { "\u{2713}" } else { "" };

    byo_write!(em,
        @view {format!("{id}-box")} class={box_class}
        @text {format!("{id}-check")} content={check_content}
    )
}

/// Emit a patch for slider visual state (value, hover).
pub fn patch_slider_state<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let value = state.value.unwrap_or(50.0);
    let pct = state.slider_pct();

    let fill_bg = if state.pressed || state.hover {
        "bg-blue-400"
    } else {
        "bg-blue-500"
    };

    let value_display = format_value(value);

    byo_write!(em,
        @text {format!("{id}-value")} content={value_display.as_str()}
        @view {format!("{id}-fill")}
            class={format!("h-full rounded-full {fill_bg}")}
            width={format!("{pct:.1}%")}
    )
}

// ---------------------------------------------------------------------------
// Scrollbar
// ---------------------------------------------------------------------------

/// Minimum thumb size as a percentage of the track.
const MIN_THUMB_PCT: f64 = 5.0;

/// Minimum thumb size during overscroll squeeze, in logical pixels.
const MIN_SQUEEZE_THUMB_PX: f64 = 20.0;

/// Compute scrollbar thumb position and size as percentages.
/// Returns (offset_pct, thumb_pct) accounting for overscroll squeeze.
fn scrollbar_thumb_geometry(state: &ControlState) -> (f64, f64) {
    let content_size = state.content_size;
    let viewport_size = state.viewport_size;
    let scroll_position = state
        .props
        .get("scroll-position")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let overflow = state.scroll_overflow;

    if content_size <= viewport_size || content_size <= 0.0 {
        // Content fits in viewport — full-size thumb
        return (0.0, 100.0);
    }

    let thumb_pct = (viewport_size / content_size * 100.0).clamp(MIN_THUMB_PCT, 100.0);
    let max_scroll = content_size - viewport_size;
    let scroll_ratio = if max_scroll > 0.0 {
        (scroll_position / max_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let offset_pct = scroll_ratio * (100.0 - thumb_pct);

    // Overscroll squeeze: shrink thumb proportional to overflow relative to viewport
    if overflow.abs() > 0.5 && viewport_size > 0.0 {
        let min_squeeze_pct = MIN_SQUEEZE_THUMB_PX / viewport_size * 100.0;
        let squeeze_ratio = (overflow.abs() / viewport_size).min(1.0);
        let squeeze_pct = thumb_pct * squeeze_ratio;
        let squeezed_thumb = (thumb_pct - squeeze_pct).max(min_squeeze_pct);

        if overflow > 0.0 {
            // Past end: thumb squeezes upward from the bottom
            let bottom_edge = offset_pct + thumb_pct;
            let squeezed_offset = bottom_edge - squeezed_thumb;
            (
                squeezed_offset.clamp(0.0, 100.0 - squeezed_thumb),
                squeezed_thumb,
            )
        } else {
            // Past start: thumb squeezes downward from the top
            (
                offset_pct.clamp(0.0, 100.0 - squeezed_thumb),
                squeezed_thumb,
            )
        }
    } else {
        (offset_pct, thumb_pct)
    }
}

/// Resolve the scrollbar style: "modern", "classic", or "auto" (defaults to "modern").
pub fn resolve_scrollbar_style(state: &ControlState) -> &'static str {
    match state
        .props
        .get("style")
        .map(|s| s.as_str())
        .unwrap_or("auto")
    {
        "classic" => "classic",
        "modern" => "modern",
        // "auto" defaults to modern
        _ => "modern",
    }
}

/// Compute the thumb background color based on state, style, and fade visibility.
fn scrollbar_thumb_bg(state: &ControlState, style: &str) -> &'static str {
    match style {
        "classic" => {
            if state.thumb_pressed {
                "bg-white/60"
            } else if state.thumb_hover {
                "bg-white/50"
            } else {
                "bg-white/30"
            }
        }
        _ => {
            // Modern: color-alpha encodes fade visibility
            if !state.fade_visible {
                "bg-white/0"
            } else if state.thumb_pressed {
                "bg-white/60"
            } else if state.thumb_hover || state.track_hover {
                "bg-white/50"
            } else {
                "bg-white/30"
            }
        }
    }
}

/// Compute the `transition` wire prop for the scrollbar track.
/// Modern scrollbars transition both color and size; classic returns empty (no transition).
fn scrollbar_track_transition(style: &str, is_vertical: bool) -> &'static str {
    match style {
        "classic" => "",
        _ => {
            if is_vertical {
                "background-color 300ms ease-out, width 300ms ease-out"
            } else {
                "background-color 300ms ease-out, height 300ms ease-out"
            }
        }
    }
}

/// Emit the expansion for a scrollbar.
pub fn expand_scrollbar<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    let id = &state.local_id;
    let is_vertical = state.is_vertical();
    let style = resolve_scrollbar_style(state);

    let (offset_pct, thumb_pct) = scrollbar_thumb_geometry(state);

    let thumb_bg = scrollbar_thumb_bg(state, style);
    let hovered = state.thumb_hover || state.track_hover || state.thumb_pressed;

    // Classic scrollbars are always interactive.
    // Modern scrollbars are only interactive when faded in.
    let interactive = style == "classic" || state.fade_visible;
    let pe = if interactive { "auto" } else { "none" };

    // Modern track transitions both color and size (width for vertical, height for horizontal).
    let track_transition = scrollbar_track_transition(style, is_vertical);

    if is_vertical {
        let (root_class, track_class, thumb_class) = scrollbar_classes_v(style, hovered, thumb_bg);

        byo_write!(em,
            +view {format!("{id}-root")} class={root_class} {
                +view {format!("{id}-track")} class={track_class}
                    events="pointerdown,pointerenter,pointerleave"
                    pointer-events={pe}
                    if !track_transition.is_empty() { transition={track_transition} }
                {
                    +view {format!("{id}-thumb")} class={thumb_class}
                        events="pointerdown,pointermove,pointerup,pointerenter,pointerleave"
                        pointer-events={pe}
                        top={format!("{offset_pct:.2}%")}
                        height={format!("{thumb_pct:.2}%")}
                }
            }
        )
    } else {
        let (root_class, track_class, thumb_class) = scrollbar_classes_h(style, hovered, thumb_bg);

        byo_write!(em,
            +view {format!("{id}-root")} class={root_class} {
                +view {format!("{id}-track")} class={track_class}
                    events="pointerdown,pointerenter,pointerleave"
                    pointer-events={pe}
                    if !track_transition.is_empty() { transition={track_transition} }
                {
                    +view {format!("{id}-thumb")} class={thumb_class}
                        events="pointerdown,pointermove,pointerup,pointerenter,pointerleave"
                        pointer-events={pe}
                        left={format!("{offset_pct:.2}%")}
                        width={format!("{thumb_pct:.2}%")}
                }
            }
        )
    }
}

/// Modern track width classes (both px-based so the transition system interpolates smoothly).
const MODERN_TRACK_THIN_CLASS: &str = "w-2";
const MODERN_TRACK_FULL_CLASS: &str = "w-4";
const MODERN_TRACK_THIN_CLASS_H: &str = "h-2";
const MODERN_TRACK_FULL_CLASS_H: &str = "h-4";

/// Compute vertical scrollbar CSS classes for root, track, and thumb.
/// For modern style, also returns the track width as a percentage prop value.
fn scrollbar_classes_v(style: &str, hovered: bool, thumb_bg: &str) -> (String, String, String) {
    match style {
        "classic" => {
            let root = "absolute right-0 top-0 bottom-0 w-4 pointer-events-none".to_string();
            let track = "w-full h-full rounded-full bg-zinc-800/50".to_string();
            let thumb =
                format!("absolute w-full rounded-full {thumb_bg} transition-colors duration-100");
            (root, track, thumb)
        }
        _ => {
            // Modern: w-4 root is a fixed invisible hit area.
            // Track is right-aligned inside root, grows from w-2 to w-4 on hover.
            // Both use px-based classes so the transition system can interpolate smoothly.
            let root = "absolute right-0 top-0 bottom-0 w-4 pointer-events-none".to_string();
            let track_w = if hovered {
                MODERN_TRACK_FULL_CLASS
            } else {
                MODERN_TRACK_THIN_CLASS
            };
            let track_bg = if hovered { " bg-zinc-800/50" } else { "" };
            let track = format!("absolute right-0 top-0 bottom-0 {track_w} rounded-full{track_bg}");
            let thumb =
                format!("absolute w-full rounded-full {thumb_bg} transition-colors duration-300");
            (root, track, thumb)
        }
    }
}

/// Compute horizontal scrollbar CSS classes for root, track, and thumb.
fn scrollbar_classes_h(style: &str, hovered: bool, thumb_bg: &str) -> (String, String, String) {
    match style {
        "classic" => {
            let root = "absolute bottom-0 left-0 right-0 h-4 pointer-events-none".to_string();
            let track = "h-full w-full rounded-full bg-zinc-800/50".to_string();
            let thumb =
                format!("absolute h-full rounded-full {thumb_bg} transition-colors duration-100");
            (root, track, thumb)
        }
        _ => {
            // Modern: h-4 root is a fixed invisible hit area.
            // Track is bottom-aligned inside root, grows from h-2 to h-4 on hover.
            // Both use px-based classes so the transition system can interpolate smoothly.
            let root = "absolute bottom-0 left-0 right-0 h-4 pointer-events-none".to_string();
            let track_h = if hovered {
                MODERN_TRACK_FULL_CLASS_H
            } else {
                MODERN_TRACK_THIN_CLASS_H
            };
            let track_bg = if hovered { " bg-zinc-800/50" } else { "" };
            let track =
                format!("absolute bottom-0 left-0 right-0 {track_h} rounded-full{track_bg}");
            let thumb =
                format!("absolute h-full rounded-full {thumb_bg} transition-colors duration-300");
            (root, track, thumb)
        }
    }
}

/// Emit a patch for scrollbar visual state (thumb position/size, hover/press, style, fade).
pub fn patch_scrollbar_state<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let is_vertical = state.is_vertical();
    let style = resolve_scrollbar_style(state);

    let (offset_pct, thumb_pct) = scrollbar_thumb_geometry(state);
    let thumb_bg = scrollbar_thumb_bg(state, style);
    let hovered = state.thumb_hover || state.track_hover || state.thumb_pressed;

    let interactive = style == "classic" || state.fade_visible;
    let pe = if interactive { "auto" } else { "none" };
    let track_transition = scrollbar_track_transition(style, is_vertical);

    if is_vertical {
        let (root_class, track_class, thumb_class) = scrollbar_classes_v(style, hovered, thumb_bg);
        byo_write!(em,
            @view {format!("{id}-root")} class={root_class}
            @view {format!("{id}-track")} class={track_class} pointer-events={pe}
                if !track_transition.is_empty() { transition={track_transition} }
            @view {format!("{id}-thumb")} class={thumb_class} pointer-events={pe}
                top={format!("{offset_pct:.2}%")}
                height={format!("{thumb_pct:.2}%")}
        )
    } else {
        let (root_class, track_class, thumb_class) = scrollbar_classes_h(style, hovered, thumb_bg);
        byo_write!(em,
            @view {format!("{id}-root")} class={root_class}
            @view {format!("{id}-track")} class={track_class} pointer-events={pe}
                if !track_transition.is_empty() { transition={track_transition} }
            @view {format!("{id}-thumb")} class={thumb_class} pointer-events={pe}
                left={format!("{offset_pct:.2}%")}
                width={format!("{thumb_pct:.2}%")}
        )
    }
}

// ---------------------------------------------------------------------------
// Scroll-view
// ---------------------------------------------------------------------------

/// Emit the expansion for a scroll-view.
pub fn expand_scroll_view<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let direction = state
        .props
        .get("direction")
        .map(|s| s.as_str())
        .unwrap_or("vertical");
    let scrollbar_style = state
        .props
        .get("scrollbar")
        .map(|s| s.as_str())
        .unwrap_or("auto");

    // Passthrough width/height/class from app props
    let width = state.props.get("width").map(|s| s.as_str()).unwrap_or("");
    let height = state.props.get("height").map(|s| s.as_str()).unwrap_or("");
    let extra_class = state.props.get("class").map(|s| s.as_str()).unwrap_or("");

    // Root uses flex-col so the viewport can fill remaining space via flex-1.
    // relative is needed for absolutely-positioned scrollbar overlays.
    let root_class = format!("relative flex flex-col overflow-clip {extra_class}");

    // Determine which axes scroll
    let scroll_y = direction == "vertical" || direction == "both";
    let scroll_x = direction == "horizontal" || direction == "both";

    let overflow_y = if scroll_y { "scroll" } else { "hidden" };
    let overflow_x = if scroll_x { "scroll" } else { "hidden" };

    let show_scrollbar = scrollbar_style != "none";
    let show_y = show_scrollbar && scroll_y;
    let show_x = show_scrollbar && scroll_x;

    // Viewport uses flex-1 min-h-0 to fill the root's remaining space.
    // min-h-0 prevents the default min-height:auto from expanding the viewport
    // to fit its content, which is essential for overflow clipping to work.
    // Classic scrollbars shrink the viewport via margin so they don't overlap content.
    let is_classic = scrollbar_style == "classic";
    let viewport_class = match (is_classic && show_y, is_classic && show_x) {
        (true, true) => "flex-1 min-h-0 min-w-0 mr-4 mb-4",
        (true, false) => "flex-1 min-h-0 min-w-0 mr-4",
        (false, true) => "flex-1 min-h-0 min-w-0 mb-4",
        (false, false) => "flex-1 min-h-0 min-w-0",
    };

    // Passthrough controlled / default scroll props from scroll-view to viewport
    let scroll_x_prop = state.props.get("scroll-x");
    let scroll_y_prop = state.props.get("scroll-y");
    let default_scroll_x = state.props.get("default-scroll-x");
    let default_scroll_y = state.props.get("default-scroll-y");

    // Start root — scroll events are caught here (stable during overscroll)
    // and forwarded to the viewport via forward() for nested dispatch.
    let scroll_events = format!("scroll forward({id}-viewport)");
    byo_write!(em,
        +view {format!("{id}-root")} class={root_class.as_str()}
            events={scroll_events.as_str()}
            if !width.is_empty() { width={width} }
            if !height.is_empty() { height={height} }
        {
            +view {format!("{id}-viewport")} class={viewport_class}
                overflow-x={overflow_x}
                overflow-y={overflow_y}
                if let Some(v) = scroll_x_prop { scroll-x={v.as_str()} }
                if let Some(v) = scroll_y_prop { scroll-y={v.as_str()} }
                if let Some(v) = default_scroll_x { default-scroll-x={v.as_str()} }
                if let Some(v) = default_scroll_y { default-scroll-y={v.as_str()} }
                events="resize"
            {
                ::_ {}
            }
            if show_y {
                +scrollbar {format!("{id}-scrollbar-y")}
                    direction=vertical
                    style={scrollbar_style}
                    scroll-position={state.scroll_y.to_string()}
                    content-size="0"
                    viewport-size="0"
            }
            if show_x {
                +scrollbar {format!("{id}-scrollbar-x")}
                    direction=horizontal
                    style={scrollbar_style}
                    scroll-position={state.scroll_x.to_string()}
                    content-size="0"
                    viewport-size="0"
            }
        }
    )
}

/// Emit a patch for scroll-view visual state.
/// Scroll position is compositor-owned; we only patch visual props here.
pub fn patch_scroll_view_state<W: io::Write>(
    _em: &mut Emitter<W>,
    _state: &ControlState,
) -> io::Result<()> {
    // No visual patches needed for scroll-view itself.
    // Scroll position is managed via controlled props (scroll-x/scroll-y)
    // on the viewport during thumb drag, and by the compositor otherwise.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ControlKind, ControlState};
    use std::collections::HashMap;

    fn make_props(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn expand_to_string(f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| em.expanded_with(0, f)).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn button_default_expansion() {
        let props = make_props(&[("label", "Save")]);
        let state = ControlState::new(ControlKind::Button, "app:save", &props);
        let out = expand_to_string(|em| expand_button(em, &state));
        assert!(out.contains("+view save-root"));
        assert!(out.contains("+text save-label content=Save"));
        assert!(out.contains("bg-zinc-700"));
        assert!(out.contains("events="));
        assert!(out.contains("pointer-events=auto"));
    }

    #[test]
    fn button_primary_variant() {
        let props = make_props(&[("label", "Save"), ("variant", "primary")]);
        let state = ControlState::new(ControlKind::Button, "app:save", &props);
        let out = expand_to_string(|em| expand_button(em, &state));
        assert!(out.contains("bg-blue-500"));
        assert!(out.contains("text-white"));
    }

    #[test]
    fn button_disabled() {
        let props = make_props(&[("label", "Save"), ("disabled", "")]);
        let state = ControlState::new(ControlKind::Button, "app:save", &props);
        let out = expand_to_string(|em| expand_button(em, &state));
        assert!(out.contains("opacity-50"));
        assert!(!out.contains("events="));
    }

    #[test]
    fn checkbox_unchecked() {
        let props = make_props(&[("label", "Agree")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:agree", &props);
        let out = expand_to_string(|em| expand_checkbox(em, &state));
        assert!(out.contains("+view agree-root"));
        assert!(out.contains("+view agree-box"));
        assert!(out.contains("+text agree-check content=\"\""));
        assert!(out.contains("+text agree-label content=Agree"));
        assert!(out.contains("border-zinc-500"));
    }

    #[test]
    fn checkbox_checked() {
        let props = make_props(&[("label", "Agree"), ("checked", "true")]);
        let state = ControlState::new(ControlKind::Checkbox, "app:agree", &props);
        let out = expand_to_string(|em| expand_checkbox(em, &state));
        assert!(out.contains("bg-blue-500"));
        assert!(out.contains("\u{2713}"));
    }

    #[test]
    fn slider_default() {
        let props = make_props(&[("label", "Volume")]);
        let state = ControlState::new(ControlKind::Slider, "app:vol", &props);
        let out = expand_to_string(|em| expand_slider(em, &state));
        assert!(out.contains("+view vol-root"));
        assert!(out.contains("+view vol-track"));
        assert!(out.contains("+view vol-fill"));
        assert!(out.contains("+text vol-label content=Volume"));
        assert!(out.contains("+text vol-value content=50"));
        assert!(out.contains("width=50.0%"));
    }

    #[test]
    fn slider_custom_value() {
        let props = make_props(&[("label", "Brightness"), ("value", "75")]);
        let state = ControlState::new(ControlKind::Slider, "app:bright", &props);
        let out = expand_to_string(|em| expand_slider(em, &state));
        assert!(out.contains("content=75"));
        assert!(out.contains("width=75.0%"));
    }

    // ── Scrollbar ──────────────────────────────────────────────────

    #[test]
    fn scrollbar_vertical_default() {
        let props = make_props(&[
            ("direction", "vertical"),
            ("scroll-position", "0"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        assert!(out.contains("+view sb-root"));
        assert!(out.contains("+view sb-track"));
        assert!(out.contains("+view sb-thumb"));
        assert!(out.contains("right-0 top-0 bottom-0"));
        assert!(out.contains("w-2")); // modern default: thin track
        assert!(out.contains("height=50.00%"));
        assert!(out.contains("top=0.00%"));
    }

    #[test]
    fn scrollbar_vertical_scrolled() {
        let props = make_props(&[
            ("direction", "vertical"),
            ("scroll-position", "250"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        // thumb is 50% size, at 50% scroll → offset = 0.5 * (100 - 50) = 25%
        assert!(out.contains("top=25.00%"));
        assert!(out.contains("height=50.00%"));
    }

    #[test]
    fn scrollbar_horizontal() {
        let props = make_props(&[
            ("direction", "horizontal"),
            ("scroll-position", "0"),
            ("content-size", "2000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        assert!(out.contains("bottom-0 left-0 right-0"));
        assert!(out.contains("h-2")); // modern default: thin track
        assert!(out.contains("width=25.00%"));
        assert!(out.contains("left=0.00%"));
    }

    #[test]
    fn scrollbar_content_fits() {
        let props = make_props(&[
            ("direction", "vertical"),
            ("scroll-position", "0"),
            ("content-size", "100"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        // When content fits, thumb should be 100%
        assert!(out.contains("height=100.00%"));
        assert!(out.contains("top=0.00%"));
    }

    // ── Scroll-view ────────────────────────────────────────────────

    #[test]
    fn scroll_view_vertical_default() {
        let props = make_props(&[("height", "400")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:content", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        eprintln!("=== SCROLL-VIEW EXPANSION ===\n{out}\n=== END ===");
        assert!(out.contains("+view content-root"));
        assert!(out.contains("+view content-viewport"));
        assert!(out.contains("overflow-y=scroll"));
        assert!(out.contains("overflow-x=hidden"));
        // No scroll-y in output: uncontrolled mode (compositor owns scroll position)
        assert!(!out.contains("scroll-y="));
        // Scroll events on root with forward() to viewport, resize on viewport
        assert!(out.contains("events=\"scroll forward(content-viewport)\""));
        assert!(out.contains("events=resize"));
        assert!(out.contains("::_ {"));
        assert!(out.contains("+scrollbar content-scrollbar-y"));
        assert!(out.contains("direction=vertical"));
    }

    #[test]
    fn scroll_view_horizontal() {
        let props = make_props(&[("direction", "horizontal"), ("width", "600")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:hscroll", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        assert!(out.contains("overflow-x=scroll"));
        assert!(out.contains("overflow-y=hidden"));
        assert!(out.contains("+scrollbar hscroll-scrollbar-x"));
        assert!(out.contains("direction=horizontal"));
        // Should NOT have vertical scrollbar
        assert!(!out.contains("+scrollbar hscroll-scrollbar-y"));
    }

    #[test]
    fn scroll_view_both_axes() {
        let props = make_props(&[("direction", "both"), ("width", "600"), ("height", "400")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:both", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        assert!(out.contains("overflow-x=scroll"));
        assert!(out.contains("overflow-y=scroll"));
        assert!(out.contains("+scrollbar both-scrollbar-y"));
        assert!(out.contains("+scrollbar both-scrollbar-x"));
    }

    #[test]
    fn scroll_view_no_scrollbar() {
        let props = make_props(&[("scrollbar", "none"), ("height", "400")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:nosb", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        assert!(out.contains("+view nosb-viewport"));
        assert!(!out.contains("+scrollbar"));
    }

    #[test]
    fn scroll_view_passthrough_class() {
        let props = make_props(&[("class", "bg-zinc-900"), ("height", "400")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:styled", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        assert!(out.contains("bg-zinc-900"));
    }

    // ── Scrollbar thumb geometry ───────────────────────────────────

    #[test]
    fn thumb_geometry_half_viewport() {
        let props = make_props(&[
            ("scroll-position", "0"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let (offset, size) = scrollbar_thumb_geometry(&state);
        assert!((size - 50.0).abs() < 0.01);
        assert!(offset.abs() < 0.01);
    }

    #[test]
    fn thumb_geometry_fully_scrolled() {
        let props = make_props(&[
            ("scroll-position", "500"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let (offset, size) = scrollbar_thumb_geometry(&state);
        assert!((size - 50.0).abs() < 0.01);
        // offset should be 100 - 50 = 50%
        assert!((offset - 50.0).abs() < 0.01);
    }

    #[test]
    fn scrollbar_classic_style() {
        let props = make_props(&[
            ("direction", "vertical"),
            ("style", "classic"),
            ("scroll-position", "0"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        assert!(out.contains("w-4")); // classic: wider
        assert!(out.contains("bg-zinc-800/50")); // classic: visible track bg
    }

    #[test]
    fn scrollbar_modern_thickens_on_hover() {
        let props = make_props(&[
            ("direction", "vertical"),
            ("style", "modern"),
            ("scroll-position", "0"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let mut state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        // Not hovered — thin track, root always w-4
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        assert!(out.contains("w-4")); // root is always w-4 for hover hit area
        assert!(out.contains("w-2")); // track is thin

        // Hovered — track expands to full width
        state.thumb_hover = true;
        state.fade_visible = true;
        let out = expand_to_string(|em| expand_scrollbar(em, &state));
        // Both root and track are w-4 when hovered
        assert!(out.contains("w-4"));
        assert!(out.contains("bg-zinc-800/50")); // track bg visible on hover
    }

    #[test]
    fn scroll_view_classic_scrollbar() {
        let props = make_props(&[("scrollbar", "classic"), ("height", "400")]);
        let state = ControlState::new(ControlKind::ScrollView, "app:content", &props);
        let out = expand_to_string(|em| expand_scroll_view(em, &state));
        // Classic style should still emit scrollbar
        assert!(out.contains("+scrollbar content-scrollbar-y"));
        assert!(out.contains("style=classic"));
    }

    #[test]
    fn thumb_geometry_min_size() {
        let props = make_props(&[
            ("scroll-position", "0"),
            ("content-size", "100000"),
            ("viewport-size", "100"),
        ]);
        let state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        let (_, size) = scrollbar_thumb_geometry(&state);
        assert!((size - MIN_THUMB_PCT).abs() < 0.01);
    }

    #[test]
    fn thumb_geometry_overscroll_squeeze_start() {
        let props = make_props(&[
            ("scroll-position", "0"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let mut state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        // No overflow — normal thumb
        let (offset, size) = scrollbar_thumb_geometry(&state);
        assert!((size - 50.0).abs() < 0.01);
        assert!((offset - 0.0).abs() < 0.01);

        // Overflow past start (negative) — thumb shrinks, stays at top
        state.scroll_overflow = -50.0;
        let (offset2, size2) = scrollbar_thumb_geometry(&state);
        assert!(size2 < size, "thumb should shrink during overscroll");
        let min_pct = MIN_SQUEEZE_THUMB_PX / 500.0 * 100.0; // 4%
        assert!(size2 >= min_pct);
        assert!((offset2 - 0.0).abs() < 0.01, "should stay near top");
    }

    #[test]
    fn thumb_geometry_overscroll_squeeze_end() {
        let props = make_props(&[
            ("scroll-position", "500"),
            ("content-size", "1000"),
            ("viewport-size", "500"),
        ]);
        let mut state = ControlState::new(ControlKind::Scrollbar, "controls:sb", &props);
        // Fully scrolled, no overflow
        let (offset, size) = scrollbar_thumb_geometry(&state);
        assert!((size - 50.0).abs() < 0.01);
        assert!((offset - 50.0).abs() < 0.01);

        // Overflow past end (positive) — thumb shrinks, pushed toward bottom
        state.scroll_overflow = 50.0;
        let (offset2, size2) = scrollbar_thumb_geometry(&state);
        assert!(size2 < size, "thumb should shrink during overscroll");
        let min_pct = MIN_SQUEEZE_THUMB_PX / 500.0 * 100.0; // 4%
        assert!(size2 >= min_pct);
        // Bottom edge should remain roughly at 100%
        assert!((offset2 + size2 - 100.0).abs() < 0.5);
    }
}
