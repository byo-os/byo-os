//! Expansion logic — generates compositor-native view/text trees from control props.

use std::io;

use byo::emitter::Emitter;
use byo::protocol::Prop;

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

    let mut root_props = vec![Prop::val("class", root_class)];

    if !state.disabled {
        root_props.push(Prop::val(
            "events",
            "pointerdown,pointerup,pointerenter,pointerleave,click",
        ));
        root_props.push(Prop::val("pointer-events", "auto"));
    }

    let text_color = if state.disabled {
        // Use alpha modifier for disabled text (e.g. text-white/40)
        format!("{}/40", style.text_color)
    } else {
        style.text_color.to_string()
    };
    let text_class = format!("{text_size} font-medium {text_color}");

    em.upsert_with("view", &format!("{id}-root"), &root_props, |em| {
        em.upsert(
            "text",
            &format!("{id}-label"),
            &[Prop::val("content", label), Prop::val("class", text_class)],
        )
    })
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

    let mut root_props = vec![Prop::val("class", root_class)];
    if !state.disabled {
        root_props.push(Prop::val(
            "events",
            "pointerdown,pointerup,pointerenter,pointerleave,click",
        ));
        root_props.push(Prop::val("pointer-events", "auto"));
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

    em.upsert_with("view", &format!("{id}-root"), &root_props, |em| {
        em.upsert_with(
            "view",
            &format!("{id}-box"),
            &[Prop::val("class", box_class)],
            |em| {
                em.upsert(
                    "text",
                    &format!("{id}-check"),
                    &[
                        Prop::val("content", check_content),
                        Prop::val("class", check_class),
                    ],
                )
            },
        )?;
        em.upsert(
            "text",
            &format!("{id}-label"),
            &[Prop::val("content", label), Prop::val("class", label_class)],
        )
    })
}

/// Emit the expansion for a slider.
pub fn expand_slider<W: io::Write>(em: &mut Emitter<W>, state: &ControlState) -> io::Result<()> {
    let id = &state.local_id;
    let label = state.props.get("label").map(|s| s.as_str()).unwrap_or("");
    let value = state.value.unwrap_or(50.0);
    let min = state.slider_min();
    let max = state.slider_max();

    let pct = if (max - min).abs() > f64::EPSILON {
        ((value - min) / (max - min) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

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

    let mut track_props = vec![Prop::val("class", "h-2 rounded-full bg-zinc-700")];
    if !state.disabled {
        track_props.push(Prop::val("events", "pointerdown,pointermove,pointerup"));
        track_props.push(Prop::val("pointer-events", "auto"));
    }

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

    em.upsert_with(
        "view",
        &format!("{id}-root"),
        &[Prop::val("class", root_class)],
        |em| {
            em.upsert_with(
                "view",
                &format!("{id}-header"),
                &[Prop::val("class", "flex justify-between")],
                |em| {
                    em.upsert(
                        "text",
                        &format!("{id}-label"),
                        &[Prop::val("content", label), Prop::val("class", label_class)],
                    )?;
                    em.upsert(
                        "text",
                        &format!("{id}-value"),
                        &[
                            Prop::val("content", value_display.as_str()),
                            Prop::val("class", value_class),
                        ],
                    )
                },
            )?;
            em.upsert_with("view", &format!("{id}-track"), &track_props, |em| {
                em.upsert(
                    "view",
                    &format!("{id}-fill"),
                    &[
                        Prop::val("class", format!("h-full rounded-full {fill_bg}")),
                        Prop::val("width", format!("{pct:.1}%")),
                    ],
                )
            })
        },
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
    em.patch("view", &format!("{id}-root"), &[Prop::val("class", class)])
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

    em.patch(
        "view",
        &format!("{id}-box"),
        &[Prop::val("class", box_class)],
    )?;
    em.patch(
        "text",
        &format!("{id}-check"),
        &[Prop::val("content", check_content)],
    )
}

/// Emit a patch for slider visual state (value, hover).
pub fn patch_slider_state<W: io::Write>(
    em: &mut Emitter<W>,
    state: &ControlState,
) -> io::Result<()> {
    let id = &state.local_id;
    let value = state.value.unwrap_or(50.0);
    let min = state.slider_min();
    let max = state.slider_max();

    let pct = if (max - min).abs() > f64::EPSILON {
        ((value - min) / (max - min) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    let fill_bg = if state.pressed || state.hover {
        "bg-blue-400"
    } else {
        "bg-blue-500"
    };

    let value_display = format_value(value);

    em.patch(
        "text",
        &format!("{id}-value"),
        &[Prop::val("content", value_display.as_str())],
    )?;
    em.patch(
        "view",
        &format!("{id}-fill"),
        &[
            Prop::val("class", format!("h-full rounded-full {fill_bg}")),
            Prop::val("width", format!("{pct:.1}%")),
        ],
    )
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
}
