//! Tailwind CSS class parser for BYO/OS compositor.
//!
//! Splits a `class` string on whitespace and applies each recognized utility
//! class by setting the corresponding field on [`ViewProps`]. Unknown classes
//! are silently ignored.

use bevy::prelude::*;

use crate::props::text::TextProps;
use crate::props::types::*;
use crate::props::view::ViewProps;
use crate::style::color::parse_color;
use crate::style::palette::tailwind_color;
use crate::transition::config::{
    EaseFn, TransitionProperty, tw_transition_colors, tw_transition_default, tw_transition_opacity,
};

/// Apply all Tailwind utility classes in `class_str` to `props`.
pub fn apply_classes(props: &mut ViewProps, class_str: &str) {
    for class in class_str.split_whitespace() {
        apply_class(props, class);
    }
}

/// Apply Tailwind utility classes relevant to text styling.
///
/// Handles `text-{size}`, `text-{color}`, `text-{align}`, and `font-{weight}` classes.
pub fn apply_text_classes(props: &mut TextProps, class_str: &str) {
    for class in class_str.split_whitespace() {
        apply_text_class(props, class);
    }
}

fn apply_text_class(props: &mut TextProps, class: &str) {
    // Text alignment
    match class {
        "text-left" => {
            props.text_align = Some(ByoTextAlign::Left);
            return;
        }
        "text-center" => {
            props.text_align = Some(ByoTextAlign::Center);
            return;
        }
        "text-right" => {
            props.text_align = Some(ByoTextAlign::Right);
            return;
        }
        "text-justify" => {
            props.text_align = Some(ByoTextAlign::Justified);
            return;
        }
        _ => {}
    }

    // Text size: text-{size}
    if let Some(rest) = class.strip_prefix("text-") {
        // Check size names first
        if let Some(size) = text_size(rest) {
            props.font_size = Some(size);
            return;
        }
        // Otherwise try as color: text-{color}-{shade}[/{opacity}]
        if let Some(color) = parse_color_class(rest) {
            props.color = Some(ByoColor(color));
        }
    }
}

/// Map Tailwind text size class names to font sizes in px.
fn text_size(name: &str) -> Option<f32> {
    Some(match name {
        "xs" => 12.0,
        "sm" => 14.0,
        "base" => 16.0,
        "lg" => 18.0,
        "xl" => 20.0,
        "2xl" => 24.0,
        "3xl" => 30.0,
        "4xl" => 36.0,
        "5xl" => 48.0,
        "6xl" => 60.0,
        "7xl" => 72.0,
        "8xl" => 96.0,
        "9xl" => 128.0,
        _ => return None,
    })
}

/// Convert a Tailwind spacing token to a [`Val`].
///
/// The Tailwind spacing scale maps integer `n` to `n * 4.0` px, with special
/// fractional and named values. Arbitrary values in brackets (e.g. `[100px]`,
/// `[50%]`) are also supported.
///
/// Returns `None` if the token is not a recognized spacing value.
pub fn spacing_scale(s: &str) -> Option<Val> {
    // Arbitrary value: [...]
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return parse_val(inner);
    }

    // Named values
    match s {
        "px" => return Some(Val::Px(1.0)),
        "0" => return Some(Val::Px(0.0)),
        "0.5" => return Some(Val::Px(2.0)),
        "1" => return Some(Val::Px(4.0)),
        "1.5" => return Some(Val::Px(6.0)),
        "2" => return Some(Val::Px(8.0)),
        "2.5" => return Some(Val::Px(10.0)),
        "3" => return Some(Val::Px(12.0)),
        "3.5" => return Some(Val::Px(14.0)),
        "4" => return Some(Val::Px(16.0)),
        "5" => return Some(Val::Px(20.0)),
        "6" => return Some(Val::Px(24.0)),
        "7" => return Some(Val::Px(28.0)),
        "8" => return Some(Val::Px(32.0)),
        "9" => return Some(Val::Px(36.0)),
        "10" => return Some(Val::Px(40.0)),
        "11" => return Some(Val::Px(44.0)),
        "12" => return Some(Val::Px(48.0)),
        "14" => return Some(Val::Px(56.0)),
        "16" => return Some(Val::Px(64.0)),
        "20" => return Some(Val::Px(80.0)),
        "24" => return Some(Val::Px(96.0)),
        "28" => return Some(Val::Px(112.0)),
        "32" => return Some(Val::Px(128.0)),
        "36" => return Some(Val::Px(144.0)),
        "40" => return Some(Val::Px(160.0)),
        "44" => return Some(Val::Px(176.0)),
        "48" => return Some(Val::Px(192.0)),
        "52" => return Some(Val::Px(208.0)),
        "56" => return Some(Val::Px(224.0)),
        "60" => return Some(Val::Px(240.0)),
        "64" => return Some(Val::Px(256.0)),
        "72" => return Some(Val::Px(288.0)),
        "80" => return Some(Val::Px(320.0)),
        "96" => return Some(Val::Px(384.0)),
        _ => {}
    }

    // Fractions
    if let Some(pct) = parse_fraction(s) {
        return Some(Val::Percent(pct));
    }

    None
}

// ---------------------------------------------------------------------------
// Sizing scale — extends spacing with named values
// ---------------------------------------------------------------------------

/// Extended sizing scale used by `w-`, `h-`, `min-w-`, etc.
/// Adds `full`, `auto`, `screen`, `min`, `max` on top of spacing_scale.
fn sizing_scale(s: &str, is_height: bool) -> Option<Val> {
    match s {
        "full" => Some(Val::Percent(100.0)),
        "auto" => Some(Val::Auto),
        "screen" => {
            if is_height {
                Some(Val::Vh(100.0))
            } else {
                Some(Val::Vw(100.0))
            }
        }
        "min" => Some(Val::VMin(100.0)),
        "max" => Some(Val::VMax(100.0)),
        _ => spacing_scale(s),
    }
}

/// Parse a fraction like `1/2`, `2/3`, `3/4` into a percentage.
fn parse_fraction(s: &str) -> Option<f32> {
    let (num, den) = s.split_once('/')?;
    let n: f32 = num.parse().ok()?;
    let d: f32 = den.parse().ok()?;
    if d == 0.0 {
        return None;
    }
    Some((n / d * 1000.0).round() / 10.0)
}

// ---------------------------------------------------------------------------
// Color parsing helpers
// ---------------------------------------------------------------------------

/// Parse a color class value, handling Tailwind palette colors, special names,
/// arbitrary values, and opacity modifiers.
///
/// Patterns:
/// - `transparent` / `black` / `white`
/// - `[#hex]` or `[rgb(...)]`
/// - `{color}-{shade}` or `{color}-{shade}/{opacity}`
fn parse_color_class(value: &str) -> Option<Color> {
    // Special keywords
    match value {
        "transparent" => return Some(Color::NONE),
        "black" => return Some(Color::srgba_u8(0, 0, 0, 255)),
        "white" => return Some(Color::srgba_u8(255, 255, 255, 255)),
        "inherit" | "current" => return None,
        _ => {}
    }

    // Arbitrary: [#hex] or [rgb(...)]
    if let Some(inner) = value.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return parse_color(inner);
    }

    // Check for opacity modifier: color-shade/opacity
    let (color_shade, opacity) = if let Some((cs, op)) = value.rsplit_once('/') {
        let alpha: f32 = op.parse().ok()?;
        (cs, Some(alpha / 100.0))
    } else {
        (value, None)
    };

    // Split into color name and shade
    let (color_name, shade_str) = color_shade.rsplit_once('-')?;
    let shade: u16 = shade_str.parse().ok()?;

    let mut color = tailwind_color(color_name, shade)?;

    if let Some(alpha) = opacity {
        let srgba = color.to_srgba();
        color = Color::srgba(srgba.red, srgba.green, srgba.blue, alpha.clamp(0.0, 1.0));
    }

    Some(color)
}

// ---------------------------------------------------------------------------
// Border radius sizes
// ---------------------------------------------------------------------------

fn rounded_size(suffix: &str) -> Option<Val> {
    Some(match suffix {
        "" | "DEFAULT" => Val::Px(4.0),
        "none" => Val::Px(0.0),
        "sm" => Val::Px(2.0),
        "md" => Val::Px(6.0),
        "lg" => Val::Px(8.0),
        "xl" => Val::Px(12.0),
        "2xl" => Val::Px(16.0),
        "3xl" => Val::Px(24.0),
        "full" => Val::Px(9999.0),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Directional rect helpers
// ---------------------------------------------------------------------------

/// Ensure `opt` contains a `ByoRect` and return a mutable reference to the
/// inner `UiRect`.
fn ensure_rect(opt: &mut Option<ByoRect>) -> &mut UiRect {
    &mut opt.get_or_insert_with(|| ByoRect(UiRect::DEFAULT)).0
}

/// Ensure `opt` contains a `ByoBorderRadius` and return a mutable reference
/// to the inner `BorderRadius`.
fn ensure_border_radius(opt: &mut Option<ByoBorderRadius>) -> &mut BorderRadius {
    &mut opt
        .get_or_insert_with(|| ByoBorderRadius(BorderRadius::DEFAULT))
        .0
}

// ---------------------------------------------------------------------------
// Main class application
// ---------------------------------------------------------------------------

fn apply_class(props: &mut ViewProps, class: &str) {
    // ── Exact matches ─────────────────────────────────────────────────
    match class {
        // Display
        "flex" => {
            props.display = Some(ByoDisplay::Flex);
            return;
        }
        "block" => {
            props.display = Some(ByoDisplay::Block);
            return;
        }
        "grid" => {
            props.display = Some(ByoDisplay::Grid);
            return;
        }
        "hidden" => {
            props.hidden = true;
            props.display = Some(ByoDisplay::None);
            return;
        }
        "inline-flex" => {
            props.display = Some(ByoDisplay::Flex);
            return;
        }

        // Position
        "relative" => {
            props.position = Some(ByoPositionType::Relative);
            return;
        }
        "absolute" => {
            props.position = Some(ByoPositionType::Absolute);
            return;
        }

        // Flex direction
        "flex-row" => {
            props.flex_direction = Some(ByoFlexDirection::Row);
            return;
        }
        "flex-col" => {
            props.flex_direction = Some(ByoFlexDirection::Column);
            return;
        }
        "flex-row-reverse" => {
            props.flex_direction = Some(ByoFlexDirection::RowReverse);
            return;
        }
        "flex-col-reverse" => {
            props.flex_direction = Some(ByoFlexDirection::ColumnReverse);
            return;
        }

        // Flex wrap
        "flex-wrap" => {
            props.flex_wrap = Some(ByoFlexWrap::Wrap);
            return;
        }
        "flex-nowrap" => {
            props.flex_wrap = Some(ByoFlexWrap::NoWrap);
            return;
        }
        "flex-wrap-reverse" => {
            props.flex_wrap = Some(ByoFlexWrap::WrapReverse);
            return;
        }

        // Align items
        "items-start" => {
            props.align_items = Some(ByoAlignItems::Start);
            return;
        }
        "items-end" => {
            props.align_items = Some(ByoAlignItems::End);
            return;
        }
        "items-center" => {
            props.align_items = Some(ByoAlignItems::Center);
            return;
        }
        "items-baseline" => {
            props.align_items = Some(ByoAlignItems::Baseline);
            return;
        }
        "items-stretch" => {
            props.align_items = Some(ByoAlignItems::Stretch);
            return;
        }

        // Justify content
        "justify-start" => {
            props.justify_content = Some(ByoJustifyContent::Start);
            return;
        }
        "justify-end" => {
            props.justify_content = Some(ByoJustifyContent::End);
            return;
        }
        "justify-center" => {
            props.justify_content = Some(ByoJustifyContent::Center);
            return;
        }
        "justify-between" => {
            props.justify_content = Some(ByoJustifyContent::SpaceBetween);
            return;
        }
        "justify-around" => {
            props.justify_content = Some(ByoJustifyContent::SpaceAround);
            return;
        }
        "justify-evenly" => {
            props.justify_content = Some(ByoJustifyContent::SpaceEvenly);
            return;
        }
        "justify-stretch" => {
            props.justify_content = Some(ByoJustifyContent::Stretch);
            return;
        }

        // Align self
        "self-auto" => {
            props.align_self = Some(ByoAlignSelf::Auto);
            return;
        }
        "self-start" => {
            props.align_self = Some(ByoAlignSelf::Start);
            return;
        }
        "self-end" => {
            props.align_self = Some(ByoAlignSelf::End);
            return;
        }
        "self-center" => {
            props.align_self = Some(ByoAlignSelf::Center);
            return;
        }
        "self-baseline" => {
            props.align_self = Some(ByoAlignSelf::Baseline);
            return;
        }
        "self-stretch" => {
            props.align_self = Some(ByoAlignSelf::Stretch);
            return;
        }

        // Flex grow/shrink
        "grow" => {
            props.flex_grow = Some(1.0);
            return;
        }
        "grow-0" => {
            props.flex_grow = Some(0.0);
            return;
        }
        "shrink" => {
            props.flex_shrink = Some(1.0);
            return;
        }
        "shrink-0" => {
            props.flex_shrink = Some(0.0);
            return;
        }

        // Flex shorthands
        "flex-1" => {
            props.flex_grow = Some(1.0);
            props.flex_shrink = Some(1.0);
            props.flex_basis = Some(ByoVal(Val::Px(0.0)));
            return;
        }
        "flex-auto" => {
            props.flex_grow = Some(1.0);
            props.flex_shrink = Some(1.0);
            props.flex_basis = Some(ByoVal(Val::Auto));
            return;
        }
        "flex-initial" => {
            props.flex_grow = Some(0.0);
            props.flex_shrink = Some(1.0);
            props.flex_basis = Some(ByoVal(Val::Auto));
            return;
        }
        "flex-none" => {
            props.flex_grow = Some(0.0);
            props.flex_shrink = Some(0.0);
            props.flex_basis = Some(ByoVal(Val::Auto));
            return;
        }

        // Overflow
        "overflow-hidden" => {
            props.overflow = Some(ByoOverflow::Hidden);
            return;
        }
        "overflow-visible" => {
            props.overflow = Some(ByoOverflow::Visible);
            return;
        }
        "overflow-scroll" => {
            props.overflow = Some(ByoOverflow::Scroll);
            return;
        }
        "overflow-clip" => {
            props.overflow = Some(ByoOverflow::Clip);
            return;
        }

        // Border (bare) — 1px all sides
        "border" => {
            props.border_width = Some(ByoRect(UiRect::all(Val::Px(1.0))));
            return;
        }

        // Rounded (bare) — 4px
        "rounded" => {
            props.border_radius = Some(ByoBorderRadius(BorderRadius::all(Val::Px(4.0))));
            return;
        }

        _ => {}
    }

    // ── Prefix-based matches ──────────────────────────────────────────

    // Width: w-{value}
    if let Some(rest) = class.strip_prefix("w-") {
        if let Some(val) = sizing_scale(rest, false) {
            props.width = Some(ByoVal(val));
        }
        return;
    }

    // Height: h-{value}
    if let Some(rest) = class.strip_prefix("h-") {
        if let Some(val) = sizing_scale(rest, true) {
            props.height = Some(ByoVal(val));
        }
        return;
    }

    // Min-width: min-w-{value}
    if let Some(rest) = class.strip_prefix("min-w-") {
        if let Some(val) = sizing_scale(rest, false) {
            props.min_width = Some(ByoVal(val));
        }
        return;
    }

    // Max-width: max-w-{value}
    if let Some(rest) = class.strip_prefix("max-w-") {
        if let Some(val) = sizing_scale(rest, false) {
            props.max_width = Some(ByoVal(val));
        }
        return;
    }

    // Min-height: min-h-{value}
    if let Some(rest) = class.strip_prefix("min-h-") {
        if let Some(val) = sizing_scale(rest, true) {
            props.min_height = Some(ByoVal(val));
        }
        return;
    }

    // Max-height: max-h-{value}
    if let Some(rest) = class.strip_prefix("max-h-") {
        if let Some(val) = sizing_scale(rest, true) {
            props.max_height = Some(ByoVal(val));
        }
        return;
    }

    // Gap: gap-x-{n}, gap-y-{n}, gap-{n}
    if let Some(rest) = class.strip_prefix("gap-") {
        if let Some(rest2) = rest.strip_prefix("x-") {
            if let Some(val) = spacing_scale(rest2) {
                props.column_gap = Some(ByoVal(val));
            }
        } else if let Some(rest2) = rest.strip_prefix("y-") {
            if let Some(val) = spacing_scale(rest2) {
                props.row_gap = Some(ByoVal(val));
            }
        } else if let Some(val) = spacing_scale(rest) {
            props.gap = Some(ByoVal(val));
        }
        return;
    }

    // Padding: px-, py-, pt-, pr-, pb-, pl-, p-
    if let Some(rest) = class.strip_prefix("px-") {
        if let Some(val) = spacing_scale(rest) {
            let rect = ensure_rect(&mut props.padding);
            rect.left = val;
            rect.right = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("py-") {
        if let Some(val) = spacing_scale(rest) {
            let rect = ensure_rect(&mut props.padding);
            rect.top = val;
            rect.bottom = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("pt-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.padding).top = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("pr-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.padding).right = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("pb-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.padding).bottom = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("pl-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.padding).left = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("p-") {
        if let Some(val) = spacing_scale(rest) {
            props.padding = Some(ByoRect(UiRect::all(val)));
        }
        return;
    }

    // Margin: mx-, my-, mt-, mr-, mb-, ml-, m-
    if let Some(rest) = class.strip_prefix("mx-") {
        if let Some(val) = spacing_scale(rest) {
            let rect = ensure_rect(&mut props.margin);
            rect.left = val;
            rect.right = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("my-") {
        if let Some(val) = spacing_scale(rest) {
            let rect = ensure_rect(&mut props.margin);
            rect.top = val;
            rect.bottom = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("mt-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.margin).top = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("mr-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.margin).right = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("mb-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.margin).bottom = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("ml-") {
        if let Some(val) = spacing_scale(rest) {
            ensure_rect(&mut props.margin).left = val;
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("m-") {
        if let Some(val) = spacing_scale(rest) {
            props.margin = Some(ByoRect(UiRect::all(val)));
        }
        return;
    }

    // Opacity: opacity-{0..100}
    if let Some(rest) = class.strip_prefix("opacity-") {
        if let Ok(n) = rest.parse::<u32>() {
            props.opacity = Some(n as f32 / 100.0);
        }
        return;
    }

    // Border radius: rounded-{corner}-{size} and rounded-{size}
    if let Some(rest) = class.strip_prefix("rounded-") {
        // Per-corner: rounded-tl-, rounded-tr-, rounded-bl-, rounded-br-
        if let Some(suffix) = rest.strip_prefix("tl-") {
            if let Some(val) = rounded_size(suffix) {
                ensure_border_radius(&mut props.border_radius).top_left = val;
            }
            return;
        }
        if let Some(suffix) = rest.strip_prefix("tr-") {
            if let Some(val) = rounded_size(suffix) {
                ensure_border_radius(&mut props.border_radius).top_right = val;
            }
            return;
        }
        if let Some(suffix) = rest.strip_prefix("bl-") {
            if let Some(val) = rounded_size(suffix) {
                ensure_border_radius(&mut props.border_radius).bottom_left = val;
            }
            return;
        }
        if let Some(suffix) = rest.strip_prefix("br-") {
            if let Some(val) = rounded_size(suffix) {
                ensure_border_radius(&mut props.border_radius).bottom_right = val;
            }
            return;
        }
        // All corners: rounded-{size}
        if let Some(val) = rounded_size(rest) {
            props.border_radius = Some(ByoBorderRadius(BorderRadius::all(val)));
        }
        return;
    }

    // Border width (must come after border-color checks):
    // border-t-{n}, border-r-{n}, border-b-{n}, border-l-{n}, border-{n}
    // and border-{color}-{shade}[/{opacity}]
    if let Some(rest) = class.strip_prefix("border-") {
        // Directional border widths
        if let Some(suffix) = rest.strip_prefix("t-")
            && let Ok(n) = suffix.parse::<f32>()
        {
            ensure_rect(&mut props.border_width).top = Val::Px(n);
            return;
        }
        if let Some(suffix) = rest.strip_prefix("r-")
            && let Ok(n) = suffix.parse::<f32>()
        {
            ensure_rect(&mut props.border_width).right = Val::Px(n);
            return;
        }
        if let Some(suffix) = rest.strip_prefix("b-")
            && let Ok(n) = suffix.parse::<f32>()
        {
            ensure_rect(&mut props.border_width).bottom = Val::Px(n);
            return;
        }
        if let Some(suffix) = rest.strip_prefix("l-")
            && let Ok(n) = suffix.parse::<f32>()
        {
            ensure_rect(&mut props.border_width).left = Val::Px(n);
            return;
        }

        // Border width shorthand: border-0, border-2, border-4, border-8
        if let Ok(n) = rest.parse::<f32>() {
            props.border_width = Some(ByoRect(UiRect::all(Val::Px(n))));
            return;
        }

        // Border color: border-{color}-{shade}[/{opacity}]
        if let Some(color) = parse_color_class(rest) {
            props.border_color = Some(ByoColor(color));
        }
        return;
    }

    // Background color: bg-{value}
    if let Some(rest) = class.strip_prefix("bg-") {
        if let Some(color) = parse_color_class(rest) {
            props.background_color = Some(ByoColor(color));
        }
        return;
    }

    // Position offsets: top-, right-, bottom-, left-
    if let Some(rest) = class.strip_prefix("top-") {
        if let Some(val) = spacing_scale(rest) {
            props.top = Some(ByoVal(val));
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("right-") {
        if let Some(val) = spacing_scale(rest) {
            props.right = Some(ByoVal(val));
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("bottom-") {
        if let Some(val) = spacing_scale(rest) {
            props.bottom = Some(ByoVal(val));
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("left-") {
        if let Some(val) = spacing_scale(rest) {
            props.left = Some(ByoVal(val));
        }
        return;
    }

    // Order: order-{n}
    if let Some(rest) = class.strip_prefix("order-")
        && let Ok(n) = rest.parse::<i32>()
    {
        props.order = Some(n);
        return;
    }

    // Transition classes
    apply_transition_class(
        class,
        &mut props.tw_transition_property,
        &mut props.tw_transition_duration,
        &mut props.tw_transition_easing,
        &mut props.tw_transition_delay,
    );

    // 2D transforms on views
    apply_2d_transform_class(
        class,
        &mut props.translate_x,
        &mut props.translate_y,
        &mut props.rotate,
        &mut props.scale,
        &mut props.scale_x,
        &mut props.scale_y,
    );

    // Unknown classes — silently ignored
}

// ---------------------------------------------------------------------------
// Transform class parsing (shared 2D helper)
// ---------------------------------------------------------------------------

/// Parse 2D transform classes: translate-x/y, rotate, scale, scale-x/y.
/// Used by both view `apply_classes()` and `apply_transform_classes()`.
fn apply_2d_transform_class(
    class: &str,
    translate_x: &mut Option<f32>,
    translate_y: &mut Option<f32>,
    rotate: &mut Option<f32>,
    scale: &mut Option<f32>,
    scale_x: &mut Option<f32>,
    scale_y: &mut Option<f32>,
) {
    // Negative prefix support
    let (class, neg) = if let Some(rest) = class.strip_prefix('-') {
        (rest, true)
    } else {
        (class, false)
    };
    let sign = if neg { -1.0 } else { 1.0 };

    // translate-x-{n}, translate-y-{n}
    if let Some(rest) = class.strip_prefix("translate-x-") {
        if let Some(Val::Px(px)) = spacing_scale(rest) {
            *translate_x = Some(px * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("translate-y-") {
        if let Some(Val::Px(px)) = spacing_scale(rest) {
            *translate_y = Some(px * sign);
        }
        return;
    }

    // scale-x-{n}, scale-y-{n}, scale-{n}
    if let Some(rest) = class.strip_prefix("scale-x-") {
        if let Some(v) = parse_scale_value(rest) {
            *scale_x = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("scale-y-") {
        if let Some(v) = parse_scale_value(rest) {
            *scale_y = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("scale-") {
        if let Some(v) = parse_scale_value(rest) {
            *scale = Some(v);
        }
        return;
    }

    // rotate-{n}
    if let Some(rest) = class.strip_prefix("rotate-")
        && let Some(v) = parse_rotation_value(rest)
    {
        *rotate = Some(v * sign);
    }
}

/// Parse a scale class value: integer (percentage → f32) or arbitrary `[value]`.
fn parse_scale_value(s: &str) -> Option<f32> {
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return inner.parse::<f32>().ok();
    }
    s.parse::<u32>().ok().map(|n| n as f32 / 100.0)
}

/// Parse a rotation class value: integer (degrees) or arbitrary `[value]`.
fn parse_rotation_value(s: &str) -> Option<f32> {
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return inner.parse::<f32>().ok();
    }
    s.parse::<f32>().ok()
}

// ---------------------------------------------------------------------------
// 3D transform + PBR class parsing (windows/layers)
// ---------------------------------------------------------------------------

/// All class-resolved values for 3D objects (windows/layers).
/// Covers transforms, PBR material properties, colors, and transitions.
#[derive(Debug, Default, Clone)]
pub struct TransformStyle {
    // Layer format
    pub format: Option<ByoTextureFormat>,
    // Transitions (tw-derived)
    pub tw_transition_property: Option<TransitionProperty>,
    pub tw_transition_duration: Option<f32>,
    pub tw_transition_easing: Option<EaseFn>,
    pub tw_transition_delay: Option<f32>,
    // Transforms
    pub translate_x: Option<f32>,
    pub translate_y: Option<f32>,
    pub translate_z: Option<f32>,
    pub rotate: Option<f32>,
    pub rotate_x: Option<f32>,
    pub rotate_y: Option<f32>,
    pub rotate_z: Option<f32>,
    pub scale: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    pub scale_z: Option<f32>,
    // PBR: 0-100 scale properties
    pub perceptual_roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub reflectance: Option<f32>,
    pub clearcoat: Option<f32>,
    pub clearcoat_perceptual_roughness: Option<f32>,
    pub anisotropy_strength: Option<f32>,
    pub specular_transmission: Option<f32>,
    pub diffuse_transmission: Option<f32>,
    // PBR: arbitrary-only floats
    pub emissive_exposure_weight: Option<f32>,
    pub ior: Option<f32>,
    pub thickness: Option<f32>,
    pub attenuation_distance: Option<f32>,
    pub anisotropy_rotation: Option<f32>,
    pub depth_bias: Option<f32>,
    // PBR: colors
    pub base_color: Option<Color>,
    pub emissive_color: Option<Color>,
    pub attenuation_color: Option<Color>,
    // PBR: booleans
    pub unlit: Option<bool>,
    pub double_sided: Option<bool>,
    pub fog_enabled: Option<bool>,
    // PBR: enum
    pub alpha_mode: Option<ByoAlphaMode>,
}

/// Apply transform + PBR shorthand classes for windows and layers.
pub fn apply_transform_classes(style: &mut TransformStyle, class_str: &str) {
    for class in class_str.split_whitespace() {
        apply_transform_class(style, class);
    }
}

fn apply_transform_class(style: &mut TransformStyle, class: &str) {
    // ── Layer format ─────────────────────────────────────────────────
    match class {
        "format-rgba8unorm-srgb" => {
            style.format = Some(ByoTextureFormat::Rgba8UnormSrgb);
            return;
        }
        "format-rgba8unorm" => {
            style.format = Some(ByoTextureFormat::Rgba8Unorm);
            return;
        }
        "format-rgb10a2unorm" => {
            style.format = Some(ByoTextureFormat::Rgb10a2Unorm);
            return;
        }
        "format-rgba16float" => {
            style.format = Some(ByoTextureFormat::Rgba16Float);
            return;
        }
        "format-rgba32float" => {
            style.format = Some(ByoTextureFormat::Rgba32Float);
            return;
        }
        _ => {}
    }

    // ── Exact matches (booleans, enums) ──────────────────────────────
    match class {
        "unlit" => {
            style.unlit = Some(true);
            return;
        }
        "lit" => {
            style.unlit = Some(false);
            return;
        }
        "double-sided" => {
            style.double_sided = Some(true);
            return;
        }
        "fog-enabled" => {
            style.fog_enabled = Some(true);
            return;
        }
        "fog-disabled" => {
            style.fog_enabled = Some(false);
            return;
        }
        // Alpha mode
        "alpha-opaque" => {
            style.alpha_mode = Some(ByoAlphaMode::Opaque);
            return;
        }
        "alpha-blend" => {
            style.alpha_mode = Some(ByoAlphaMode::Blend);
            return;
        }
        "alpha-premultiplied" => {
            style.alpha_mode = Some(ByoAlphaMode::Premultiplied);
            return;
        }
        "alpha-add" => {
            style.alpha_mode = Some(ByoAlphaMode::Add);
            return;
        }
        "alpha-multiply" => {
            style.alpha_mode = Some(ByoAlphaMode::Multiply);
            return;
        }
        _ => {}
    }

    // ── Transition classes ──────────────────────────────────────────────
    apply_transition_class(
        class,
        &mut style.tw_transition_property,
        &mut style.tw_transition_duration,
        &mut style.tw_transition_easing,
        &mut style.tw_transition_delay,
    );

    // ── Negative prefix for transforms ───────────────────────────────
    let (class, neg) = if let Some(rest) = class.strip_prefix('-') {
        (rest, true)
    } else {
        (class, false)
    };
    let sign = if neg { -1.0 } else { 1.0 };

    // ── Translations ─────────────────────────────────────────────────
    if let Some(rest) = class.strip_prefix("translate-z-") {
        if let Some(Val::Px(px)) = spacing_scale(rest) {
            style.translate_z = Some(px * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("translate-x-") {
        if let Some(Val::Px(px)) = spacing_scale(rest) {
            style.translate_x = Some(px * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("translate-y-") {
        if let Some(Val::Px(px)) = spacing_scale(rest) {
            style.translate_y = Some(px * sign);
        }
        return;
    }

    // ── Rotations ────────────────────────────────────────────────────
    if let Some(rest) = class.strip_prefix("rotate-x-") {
        if let Some(v) = parse_rotation_value(rest) {
            style.rotate_x = Some(v * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("rotate-y-") {
        if let Some(v) = parse_rotation_value(rest) {
            style.rotate_y = Some(v * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("rotate-z-") {
        if let Some(v) = parse_rotation_value(rest) {
            style.rotate_z = Some(v * sign);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("rotate-")
        && let Some(v) = parse_rotation_value(rest)
    {
        style.rotate = Some(v * sign);
        return;
    }

    // ── Scale ────────────────────────────────────────────────────────
    if let Some(rest) = class.strip_prefix("scale-x-") {
        if let Some(v) = parse_scale_value(rest) {
            style.scale_x = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("scale-y-") {
        if let Some(v) = parse_scale_value(rest) {
            style.scale_y = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("scale-z-") {
        if let Some(v) = parse_scale_value(rest) {
            style.scale_z = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("scale-") {
        if let Some(v) = parse_scale_value(rest) {
            style.scale = Some(v);
        }
        return;
    }

    // ── PBR colors: base-{color}, emissive-{color}, attenuation-{color} ──
    if let Some(rest) = class.strip_prefix("emissive-") {
        // emissive-exposure-[v] must be checked before color parsing
        if let Some(rest2) = rest.strip_prefix("exposure-") {
            if let Some(v) = parse_arbitrary_value(rest2) {
                style.emissive_exposure_weight = Some(v);
            }
        } else if let Some(color) = parse_color_class(rest) {
            style.emissive_color = Some(color);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("base-") {
        if let Some(color) = parse_color_class(rest) {
            style.base_color = Some(color);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("attenuation-") {
        // attenuation-distance-[v] vs attenuation-{color}
        if let Some(inner) = rest.strip_prefix("distance-") {
            if let Some(v) = parse_arbitrary_value(inner) {
                style.attenuation_distance = Some(v);
            }
            return;
        }
        if let Some(color) = parse_color_class(rest) {
            style.attenuation_color = Some(color);
        }
        return;
    }

    // ── PBR 0-100 scale: roughness, metallic, reflectance, etc. ──────
    if let Some(rest) = class.strip_prefix("roughness-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.perceptual_roughness = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("metallic-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.metallic = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("reflectance-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.reflectance = Some(v);
        }
        return;
    }
    // clearcoat-roughness-{n} must come before clearcoat-{n}
    if let Some(rest) = class.strip_prefix("clearcoat-roughness-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.clearcoat_perceptual_roughness = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("clearcoat-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.clearcoat = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("anisotropy-rotation-") {
        if let Some(v) = parse_arbitrary_value(rest) {
            style.anisotropy_rotation = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("anisotropy-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.anisotropy_strength = Some(v);
        }
        return;
    }
    // diffuse-transmission-{n} must come before transmission-{n}
    if let Some(rest) = class.strip_prefix("diffuse-transmission-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.diffuse_transmission = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("transmission-") {
        if let Some(v) = parse_pbr_value(rest) {
            style.specular_transmission = Some(v);
        }
        return;
    }

    // ── PBR arbitrary-only floats ────────────────────────────────────
    if let Some(rest) = class.strip_prefix("ior-") {
        if let Some(v) = parse_arbitrary_value(rest) {
            style.ior = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("thickness-") {
        if let Some(v) = parse_arbitrary_value(rest) {
            style.thickness = Some(v);
        }
        return;
    }
    if let Some(rest) = class.strip_prefix("depth-bias-")
        && let Some(v) = parse_arbitrary_value(rest)
    {
        style.depth_bias = Some(v);
    }
}

// ---------------------------------------------------------------------------
// Transition class parsing (shared helper)
// ---------------------------------------------------------------------------

/// Parse transition-related Tailwind utility classes.
fn apply_transition_class(
    class: &str,
    property: &mut Option<TransitionProperty>,
    duration: &mut Option<f32>,
    easing: &mut Option<EaseFn>,
    delay: &mut Option<f32>,
) {
    match class {
        "transition" => {
            *property = Some(tw_transition_default());
            if duration.is_none() {
                *duration = Some(0.15);
            }
            if easing.is_none() {
                *easing = Some(EaseFn::SmoothStep);
            }
            return;
        }
        "transition-all" => {
            *property = Some(TransitionProperty::All);
            if duration.is_none() {
                *duration = Some(0.15);
            }
            if easing.is_none() {
                *easing = Some(EaseFn::SmoothStep);
            }
            return;
        }
        "transition-colors" => {
            *property = Some(tw_transition_colors());
            if duration.is_none() {
                *duration = Some(0.15);
            }
            if easing.is_none() {
                *easing = Some(EaseFn::SmoothStep);
            }
            return;
        }
        "transition-opacity" => {
            *property = Some(tw_transition_opacity());
            if duration.is_none() {
                *duration = Some(0.15);
            }
            if easing.is_none() {
                *easing = Some(EaseFn::SmoothStep);
            }
            return;
        }
        "transition-none" => {
            *property = None;
            *duration = None;
            *easing = None;
            *delay = None;
            return;
        }
        "ease-linear" => {
            *easing = Some(EaseFn::Linear);
            return;
        }
        "ease-in" => {
            *easing = Some(EaseFn::CubicIn);
            return;
        }
        "ease-out" => {
            *easing = Some(EaseFn::CubicOut);
            return;
        }
        "ease-in-out" => {
            *easing = Some(EaseFn::CubicInOut);
            return;
        }
        _ => {}
    }

    // duration-{N} or duration-[Nms]
    if let Some(rest) = class.strip_prefix("duration-") {
        if let Some(d) = parse_duration_class(rest) {
            *duration = Some(d);
        }
        return;
    }

    // delay-{N} or delay-[Nms]
    if let Some(rest) = class.strip_prefix("delay-")
        && let Some(d) = parse_duration_class(rest)
    {
        *delay = Some(d);
    }
}

/// Parse a duration/delay class value: integer (ms) or arbitrary `[Nms]`/`[Ns]`.
fn parse_duration_class(s: &str) -> Option<f32> {
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return crate::transition::config::parse_duration(inner);
    }
    s.parse::<u32>().ok().map(|n| n as f32 / 1000.0)
}

// ---------------------------------------------------------------------------
// PBR parsing helpers
// ---------------------------------------------------------------------------

/// Parse a PBR shorthand value: 0-100 → 0.0-1.0, or arbitrary `[value]`.
fn parse_pbr_value(s: &str) -> Option<f32> {
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return inner.parse::<f32>().ok();
    }
    s.parse::<u32>()
        .ok()
        .map(|n| (n as f32 / 100.0).clamp(0.0, 1.0))
}

/// Parse an arbitrary-only value: `[value]`.
fn parse_arbitrary_value(s: &str) -> Option<f32> {
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    inner.parse::<f32>().ok()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn props_from(classes: &str) -> ViewProps {
        let mut props = ViewProps::default();
        apply_classes(&mut props, classes);
        props
    }

    fn assert_color_approx(actual: Color, expected: Color) {
        let a = actual.to_srgba();
        let e = expected.to_srgba();
        assert!(
            (a.red - e.red).abs() < 0.02
                && (a.green - e.green).abs() < 0.02
                && (a.blue - e.blue).abs() < 0.02
                && (a.alpha - e.alpha).abs() < 0.02,
            "Colors differ: {a:?} vs {e:?}"
        );
    }

    // ── Display ───────────────────────────────────────────────────────

    #[test]
    fn display_flex() {
        let p = props_from("flex");
        assert!(matches!(p.display, Some(ByoDisplay::Flex)));
    }

    #[test]
    fn display_hidden() {
        let p = props_from("hidden");
        assert!(p.hidden);
        assert!(matches!(p.display, Some(ByoDisplay::None)));
    }

    #[test]
    fn display_grid() {
        let p = props_from("grid");
        assert!(matches!(p.display, Some(ByoDisplay::Grid)));
    }

    #[test]
    fn display_inline_flex() {
        let p = props_from("inline-flex");
        assert!(matches!(p.display, Some(ByoDisplay::Flex)));
    }

    // ── Sizing ────────────────────────────────────────────────────────

    #[test]
    fn width_64() {
        let p = props_from("w-64");
        assert_eq!(p.width.unwrap().0, Val::Px(256.0));
    }

    #[test]
    fn width_full() {
        let p = props_from("w-full");
        assert_eq!(p.width.unwrap().0, Val::Percent(100.0));
    }

    #[test]
    fn width_auto() {
        let p = props_from("w-auto");
        assert_eq!(p.width.unwrap().0, Val::Auto);
    }

    #[test]
    fn width_screen() {
        let p = props_from("w-screen");
        assert_eq!(p.width.unwrap().0, Val::Vw(100.0));
    }

    #[test]
    fn height_screen() {
        let p = props_from("h-screen");
        assert_eq!(p.height.unwrap().0, Val::Vh(100.0));
    }

    #[test]
    fn width_fraction_half() {
        let p = props_from("w-1/2");
        assert_eq!(p.width.unwrap().0, Val::Percent(50.0));
    }

    #[test]
    fn width_fraction_two_thirds() {
        let p = props_from("w-2/3");
        let pct = match p.width.unwrap().0 {
            Val::Percent(v) => v,
            other => panic!("expected Percent, got {other:?}"),
        };
        assert!((pct - 66.7).abs() < 0.1);
    }

    #[test]
    fn height_48() {
        let p = props_from("h-48");
        assert_eq!(p.height.unwrap().0, Val::Px(192.0));
    }

    #[test]
    fn min_width() {
        let p = props_from("min-w-0");
        assert_eq!(p.min_width.unwrap().0, Val::Px(0.0));
    }

    #[test]
    fn max_height() {
        let p = props_from("max-h-full");
        assert_eq!(p.max_height.unwrap().0, Val::Percent(100.0));
    }

    // ── Spacing ───────────────────────────────────────────────────────

    #[test]
    fn padding_4() {
        let p = props_from("p-4");
        assert_eq!(p.padding.unwrap().0, UiRect::all(Val::Px(16.0)));
    }

    #[test]
    fn padding_x_2() {
        let p = props_from("px-2");
        let rect = p.padding.unwrap().0;
        assert_eq!(rect.left, Val::Px(8.0));
        assert_eq!(rect.right, Val::Px(8.0));
    }

    #[test]
    fn padding_y_4() {
        let p = props_from("py-4");
        let rect = p.padding.unwrap().0;
        assert_eq!(rect.top, Val::Px(16.0));
        assert_eq!(rect.bottom, Val::Px(16.0));
    }

    #[test]
    fn padding_top() {
        let p = props_from("pt-8");
        assert_eq!(p.padding.unwrap().0.top, Val::Px(32.0));
    }

    #[test]
    fn margin_4() {
        let p = props_from("m-4");
        assert_eq!(p.margin.unwrap().0, UiRect::all(Val::Px(16.0)));
    }

    #[test]
    fn margin_x_auto() {
        // spacing_scale does not support "auto", so mx-auto won't work
        // via spacing_scale. This is expected — only sizing has auto.
        let p = props_from("mx-2");
        let rect = p.margin.unwrap().0;
        assert_eq!(rect.left, Val::Px(8.0));
        assert_eq!(rect.right, Val::Px(8.0));
    }

    #[test]
    fn directional_padding_combined() {
        let p = props_from("pt-2 pb-4 pl-1 pr-3");
        let rect = p.padding.unwrap().0;
        assert_eq!(rect.top, Val::Px(8.0));
        assert_eq!(rect.bottom, Val::Px(16.0));
        assert_eq!(rect.left, Val::Px(4.0));
        assert_eq!(rect.right, Val::Px(12.0));
    }

    #[test]
    fn gap() {
        let p = props_from("gap-4");
        assert_eq!(p.gap.unwrap().0, Val::Px(16.0));
    }

    #[test]
    fn gap_x_y() {
        let p = props_from("gap-x-2 gap-y-4");
        assert_eq!(p.column_gap.unwrap().0, Val::Px(8.0));
        assert_eq!(p.row_gap.unwrap().0, Val::Px(16.0));
    }

    // ── Colors ────────────────────────────────────────────────────────

    #[test]
    fn bg_red_500() {
        let p = props_from("bg-red-500");
        let color = p.background_color.unwrap().0;
        assert_color_approx(color, Color::srgba_u8(0xef, 0x44, 0x44, 255));
    }

    #[test]
    fn bg_blue_500_with_opacity() {
        let p = props_from("bg-blue-500/50");
        let color = p.background_color.unwrap().0;
        let srgba = color.to_srgba();
        assert!((srgba.red - 59.0 / 255.0).abs() < 0.02);
        assert!((srgba.green - 130.0 / 255.0).abs() < 0.02);
        assert!((srgba.blue - 246.0 / 255.0).abs() < 0.02);
        assert!((srgba.alpha - 0.5).abs() < 0.02);
    }

    #[test]
    fn bg_transparent() {
        let p = props_from("bg-transparent");
        assert_eq!(p.background_color.unwrap().0, Color::NONE);
    }

    #[test]
    fn bg_black() {
        let p = props_from("bg-black");
        assert_color_approx(p.background_color.unwrap().0, Color::srgba_u8(0, 0, 0, 255));
    }

    #[test]
    fn bg_white() {
        let p = props_from("bg-white");
        assert_color_approx(
            p.background_color.unwrap().0,
            Color::srgba_u8(255, 255, 255, 255),
        );
    }

    #[test]
    fn bg_arbitrary_hex() {
        let p = props_from("bg-[#ff0000]");
        assert_color_approx(
            p.background_color.unwrap().0,
            Color::srgba_u8(255, 0, 0, 255),
        );
    }

    #[test]
    fn border_color() {
        let p = props_from("border-zinc-700");
        assert_color_approx(
            p.border_color.unwrap().0,
            Color::srgba_u8(0x3f, 0x3f, 0x46, 255),
        );
    }

    // ── Border ────────────────────────────────────────────────────────

    #[test]
    fn border_bare() {
        let p = props_from("border");
        assert_eq!(p.border_width.unwrap().0, UiRect::all(Val::Px(1.0)));
    }

    #[test]
    fn border_0() {
        let p = props_from("border-0");
        assert_eq!(p.border_width.unwrap().0, UiRect::all(Val::Px(0.0)));
    }

    #[test]
    fn border_2() {
        let p = props_from("border-2");
        assert_eq!(p.border_width.unwrap().0, UiRect::all(Val::Px(2.0)));
    }

    #[test]
    fn border_4() {
        let p = props_from("border-4");
        assert_eq!(p.border_width.unwrap().0, UiRect::all(Val::Px(4.0)));
    }

    #[test]
    fn border_8() {
        let p = props_from("border-8");
        assert_eq!(p.border_width.unwrap().0, UiRect::all(Val::Px(8.0)));
    }

    #[test]
    fn border_directional() {
        let p = props_from("border-t-2 border-b-4");
        let rect = p.border_width.unwrap().0;
        assert_eq!(rect.top, Val::Px(2.0));
        assert_eq!(rect.bottom, Val::Px(4.0));
    }

    // ── Border radius ─────────────────────────────────────────────────

    #[test]
    fn rounded_bare() {
        let p = props_from("rounded");
        assert_eq!(p.border_radius.unwrap().0, BorderRadius::all(Val::Px(4.0)));
    }

    #[test]
    fn rounded_lg() {
        let p = props_from("rounded-lg");
        assert_eq!(p.border_radius.unwrap().0, BorderRadius::all(Val::Px(8.0)));
    }

    #[test]
    fn rounded_full() {
        let p = props_from("rounded-full");
        assert_eq!(
            p.border_radius.unwrap().0,
            BorderRadius::all(Val::Px(9999.0))
        );
    }

    #[test]
    fn rounded_none() {
        let p = props_from("rounded-none");
        assert_eq!(p.border_radius.unwrap().0, BorderRadius::all(Val::Px(0.0)));
    }

    #[test]
    fn rounded_per_corner() {
        let p = props_from("rounded-tl-lg rounded-br-sm");
        let br = p.border_radius.unwrap().0;
        assert_eq!(br.top_left, Val::Px(8.0));
        assert_eq!(br.bottom_right, Val::Px(2.0));
    }

    // ── Flex shorthands ───────────────────────────────────────────────

    #[test]
    fn flex_1() {
        let p = props_from("flex-1");
        assert_eq!(p.flex_grow, Some(1.0));
        assert_eq!(p.flex_shrink, Some(1.0));
        assert_eq!(p.flex_basis.unwrap().0, Val::Px(0.0));
    }

    #[test]
    fn flex_col() {
        let p = props_from("flex-col");
        assert!(matches!(p.flex_direction, Some(ByoFlexDirection::Column)));
    }

    #[test]
    fn flex_auto() {
        let p = props_from("flex-auto");
        assert_eq!(p.flex_grow, Some(1.0));
        assert_eq!(p.flex_shrink, Some(1.0));
        assert_eq!(p.flex_basis.unwrap().0, Val::Auto);
    }

    #[test]
    fn flex_none() {
        let p = props_from("flex-none");
        assert_eq!(p.flex_grow, Some(0.0));
        assert_eq!(p.flex_shrink, Some(0.0));
        assert_eq!(p.flex_basis.unwrap().0, Val::Auto);
    }

    // ── Arbitrary values ──────────────────────────────────────────────

    #[test]
    fn arbitrary_width_px() {
        let p = props_from("w-[100px]");
        assert_eq!(p.width.unwrap().0, Val::Px(100.0));
    }

    #[test]
    fn arbitrary_width_percent() {
        let p = props_from("w-[50%]");
        assert_eq!(p.width.unwrap().0, Val::Percent(50.0));
    }

    #[test]
    fn arbitrary_height_bare() {
        let p = props_from("h-[200]");
        assert_eq!(p.height.unwrap().0, Val::Px(200.0));
    }

    #[test]
    fn arbitrary_padding() {
        let p = props_from("p-[12px]");
        assert_eq!(p.padding.unwrap().0, UiRect::all(Val::Px(12.0)));
    }

    // ── Overflow ──────────────────────────────────────────────────────

    #[test]
    fn overflow_hidden() {
        let p = props_from("overflow-hidden");
        assert!(matches!(p.overflow, Some(ByoOverflow::Hidden)));
    }

    // ── Opacity ───────────────────────────────────────────────────────

    #[test]
    fn opacity_50() {
        let p = props_from("opacity-50");
        assert!((p.opacity.unwrap() - 0.5).abs() < 0.01);
    }

    #[test]
    fn opacity_100() {
        let p = props_from("opacity-100");
        assert!((p.opacity.unwrap() - 1.0).abs() < 0.01);
    }

    // ── Position ──────────────────────────────────────────────────────

    #[test]
    fn position_absolute() {
        let p = props_from("absolute");
        assert!(matches!(p.position, Some(ByoPositionType::Absolute)));
    }

    #[test]
    fn position_offsets() {
        let p = props_from("top-4 left-2");
        assert_eq!(p.top.unwrap().0, Val::Px(16.0));
        assert_eq!(p.left.unwrap().0, Val::Px(8.0));
    }

    // ── Combined classes ──────────────────────────────────────────────

    #[test]
    fn combined_typical_view() {
        let p = props_from("flex flex-col w-64 h-full bg-zinc-800 p-4 gap-2 rounded-lg");
        assert!(matches!(p.display, Some(ByoDisplay::Flex)));
        assert!(matches!(p.flex_direction, Some(ByoFlexDirection::Column)));
        assert_eq!(p.width.unwrap().0, Val::Px(256.0));
        assert_eq!(p.height.unwrap().0, Val::Percent(100.0));
        assert!(p.background_color.is_some());
        assert_eq!(p.padding.unwrap().0, UiRect::all(Val::Px(16.0)));
        assert_eq!(p.gap.unwrap().0, Val::Px(8.0));
        assert_eq!(p.border_radius.unwrap().0, BorderRadius::all(Val::Px(8.0)));
    }

    #[test]
    fn unknown_classes_ignored() {
        let p = props_from("foo-bar baz-qux flex");
        assert!(matches!(p.display, Some(ByoDisplay::Flex)));
        // Nothing else was set incorrectly
        assert!(p.width.is_none());
        assert!(p.background_color.is_none());
    }

    // ── Text classes ──────────────────────────────────────────────────

    fn text_props_from(classes: &str) -> TextProps {
        let mut props = TextProps::default();
        apply_text_classes(&mut props, classes);
        props
    }

    #[test]
    fn text_size_2xl() {
        let p = text_props_from("text-2xl");
        assert_eq!(p.font_size, Some(24.0));
    }

    #[test]
    fn text_size_xs() {
        let p = text_props_from("text-xs");
        assert_eq!(p.font_size, Some(12.0));
    }

    #[test]
    fn text_size_9xl() {
        let p = text_props_from("text-9xl");
        assert_eq!(p.font_size, Some(128.0));
    }

    #[test]
    fn text_color_white() {
        let p = text_props_from("text-white");
        assert_color_approx(p.color.unwrap().0, Color::srgba_u8(255, 255, 255, 255));
    }

    #[test]
    fn text_color_zinc_500() {
        let p = text_props_from("text-zinc-500");
        assert!(p.color.is_some());
    }

    #[test]
    fn text_color_sky_400() {
        let p = text_props_from("text-sky-400");
        assert!(p.color.is_some());
    }

    #[test]
    fn text_align_center() {
        let p = text_props_from("text-center");
        assert!(matches!(p.text_align, Some(ByoTextAlign::Center)));
    }

    #[test]
    fn text_align_right() {
        let p = text_props_from("text-right");
        assert!(matches!(p.text_align, Some(ByoTextAlign::Right)));
    }

    #[test]
    fn text_combined() {
        let p = text_props_from("text-2xl text-white");
        assert_eq!(p.font_size, Some(24.0));
        assert_color_approx(p.color.unwrap().0, Color::srgba_u8(255, 255, 255, 255));
    }

    #[test]
    fn text_color_with_opacity() {
        let p = text_props_from("text-sky-500/50");
        let c = p.color.unwrap().0.to_srgba();
        assert!((c.alpha - 0.5).abs() < 0.02);
    }

    // ── Spacing scale edge cases ──────────────────────────────────────

    #[test]
    fn spacing_px() {
        let p = props_from("w-px");
        assert_eq!(p.width.unwrap().0, Val::Px(1.0));
    }

    #[test]
    fn spacing_half() {
        let p = props_from("p-0.5");
        assert_eq!(p.padding.unwrap().0, UiRect::all(Val::Px(2.0)));
    }

    #[test]
    fn spacing_1_5() {
        let p = props_from("m-1.5");
        assert_eq!(p.margin.unwrap().0, UiRect::all(Val::Px(6.0)));
    }

    // ── 2D Transform classes (views) ─────────────────────────────────

    #[test]
    fn translate_x_4() {
        let p = props_from("translate-x-4");
        assert_eq!(p.translate_x, Some(16.0));
    }

    #[test]
    fn translate_y_8() {
        let p = props_from("translate-y-8");
        assert_eq!(p.translate_y, Some(32.0));
    }

    #[test]
    fn neg_translate_x() {
        let p = props_from("-translate-x-4");
        assert_eq!(p.translate_x, Some(-16.0));
    }

    #[test]
    fn translate_x_arbitrary() {
        let p = props_from("translate-x-[100]");
        assert_eq!(p.translate_x, Some(100.0));
    }

    #[test]
    fn rotate_45() {
        let p = props_from("rotate-45");
        assert_eq!(p.rotate, Some(45.0));
    }

    #[test]
    fn neg_rotate_45() {
        let p = props_from("-rotate-45");
        assert_eq!(p.rotate, Some(-45.0));
    }

    #[test]
    fn rotate_arbitrary() {
        let p = props_from("rotate-[30]");
        assert_eq!(p.rotate, Some(30.0));
    }

    #[test]
    fn scale_50() {
        let p = props_from("scale-50");
        assert_eq!(p.scale, Some(0.5));
    }

    #[test]
    fn scale_150() {
        let p = props_from("scale-150");
        assert_eq!(p.scale, Some(1.5));
    }

    #[test]
    fn scale_x_75() {
        let p = props_from("scale-x-75");
        assert_eq!(p.scale_x, Some(0.75));
    }

    #[test]
    fn scale_y_125() {
        let p = props_from("scale-y-125");
        assert_eq!(p.scale_y, Some(1.25));
    }

    #[test]
    fn scale_arbitrary() {
        let p = props_from("scale-[0.85]");
        assert_eq!(p.scale, Some(0.85));
    }

    // ── 3D Transform + PBR classes ───────────────────────────────────

    fn transform_from(classes: &str) -> TransformStyle {
        let mut style = TransformStyle::default();
        apply_transform_classes(&mut style, classes);
        style
    }

    #[test]
    fn transform_translate_z() {
        let s = transform_from("translate-z-4");
        assert_eq!(s.translate_z, Some(16.0));
    }

    #[test]
    fn transform_rotate_x() {
        let s = transform_from("rotate-x-45");
        assert_eq!(s.rotate_x, Some(45.0));
    }

    #[test]
    fn transform_rotate_y() {
        let s = transform_from("rotate-y-90");
        assert_eq!(s.rotate_y, Some(90.0));
    }

    #[test]
    fn transform_rotate_z() {
        let s = transform_from("rotate-z-180");
        assert_eq!(s.rotate_z, Some(180.0));
    }

    #[test]
    fn transform_scale_z() {
        let s = transform_from("scale-z-50");
        assert_eq!(s.scale_z, Some(0.5));
    }

    #[test]
    fn roughness_25() {
        let s = transform_from("roughness-25");
        assert_eq!(s.perceptual_roughness, Some(0.25));
    }

    #[test]
    fn metallic_100() {
        let s = transform_from("metallic-100");
        assert_eq!(s.metallic, Some(1.0));
    }

    #[test]
    fn roughness_arbitrary() {
        let s = transform_from("roughness-[0.3]");
        assert_eq!(s.perceptual_roughness, Some(0.3));
    }

    #[test]
    fn metallic_arbitrary() {
        let s = transform_from("metallic-[0.8]");
        assert_eq!(s.metallic, Some(0.8));
    }

    #[test]
    fn combined_3d_transforms() {
        let s = transform_from("translate-x-4 rotate-y-90 scale-150 metallic-50 roughness-75");
        assert_eq!(s.translate_x, Some(16.0));
        assert_eq!(s.rotate_y, Some(90.0));
        assert_eq!(s.scale, Some(1.5));
        assert_eq!(s.metallic, Some(0.5));
        assert_eq!(s.perceptual_roughness, Some(0.75));
    }

    // ── PBR color classes ────────────────────────────────────────────

    #[test]
    fn base_color_red_500() {
        let s = transform_from("base-red-500");
        assert!(s.base_color.is_some());
        assert_color_approx(
            s.base_color.unwrap(),
            Color::srgba_u8(0xef, 0x44, 0x44, 255),
        );
    }

    #[test]
    fn base_color_arbitrary() {
        let s = transform_from("base-[#00ff00]");
        assert!(s.base_color.is_some());
        assert_color_approx(s.base_color.unwrap(), Color::srgba_u8(0, 255, 0, 255));
    }

    #[test]
    fn base_color_with_opacity() {
        let s = transform_from("base-blue-500/50");
        let c = s.base_color.unwrap().to_srgba();
        assert!((c.alpha - 0.5).abs() < 0.02);
    }

    #[test]
    fn emissive_color_red_500() {
        let s = transform_from("emissive-red-500");
        assert!(s.emissive_color.is_some());
    }

    #[test]
    fn emissive_color_arbitrary_hdr() {
        let s = transform_from("emissive-[rgb(510,0,0)]");
        assert!(s.emissive_color.is_some());
        let c = s.emissive_color.unwrap().to_srgba();
        assert!(c.red > 1.5); // HDR value
    }

    #[test]
    fn attenuation_color() {
        let s = transform_from("attenuation-sky-400");
        assert!(s.attenuation_color.is_some());
    }

    #[test]
    fn attenuation_distance_arbitrary() {
        let s = transform_from("attenuation-distance-[0.5]");
        assert_eq!(s.attenuation_distance, Some(0.5));
    }

    // ── PBR 0-100 scale classes ──────────────────────────────────────

    #[test]
    fn reflectance_50() {
        let s = transform_from("reflectance-50");
        assert_eq!(s.reflectance, Some(0.5));
    }

    #[test]
    fn clearcoat_75() {
        let s = transform_from("clearcoat-75");
        assert_eq!(s.clearcoat, Some(0.75));
    }

    #[test]
    fn clearcoat_roughness_25() {
        let s = transform_from("clearcoat-roughness-25");
        assert_eq!(s.clearcoat_perceptual_roughness, Some(0.25));
    }

    #[test]
    fn anisotropy_50() {
        let s = transform_from("anisotropy-50");
        assert_eq!(s.anisotropy_strength, Some(0.5));
    }

    #[test]
    fn transmission_75() {
        let s = transform_from("transmission-75");
        assert_eq!(s.specular_transmission, Some(0.75));
    }

    #[test]
    fn diffuse_transmission_25() {
        let s = transform_from("diffuse-transmission-25");
        assert_eq!(s.diffuse_transmission, Some(0.25));
    }

    // ── PBR arbitrary-only floats ────────────────────────────────────

    #[test]
    fn ior_arbitrary() {
        let s = transform_from("ior-[1.5]");
        assert_eq!(s.ior, Some(1.5));
    }

    #[test]
    fn thickness_arbitrary() {
        let s = transform_from("thickness-[0.5]");
        assert_eq!(s.thickness, Some(0.5));
    }

    #[test]
    fn emissive_exposure_arbitrary() {
        let s = transform_from("emissive-exposure-[0.8]");
        assert_eq!(s.emissive_exposure_weight, Some(0.8));
    }

    #[test]
    fn anisotropy_rotation_arbitrary() {
        let s = transform_from("anisotropy-rotation-[1.57]");
        assert_eq!(s.anisotropy_rotation, Some(1.57));
    }

    #[test]
    fn depth_bias_arbitrary() {
        let s = transform_from("depth-bias-[0.01]");
        assert_eq!(s.depth_bias, Some(0.01));
    }

    // ── PBR boolean toggles ──────────────────────────────────────────

    #[test]
    fn unlit_class() {
        let s = transform_from("unlit");
        assert_eq!(s.unlit, Some(true));
    }

    #[test]
    fn lit_class() {
        let s = transform_from("lit");
        assert_eq!(s.unlit, Some(false));
    }

    #[test]
    fn double_sided_class() {
        let s = transform_from("double-sided");
        assert_eq!(s.double_sided, Some(true));
    }

    #[test]
    fn fog_enabled_class() {
        let s = transform_from("fog-enabled");
        assert_eq!(s.fog_enabled, Some(true));
    }

    #[test]
    fn fog_disabled_class() {
        let s = transform_from("fog-disabled");
        assert_eq!(s.fog_enabled, Some(false));
    }

    // ── PBR alpha mode classes ───────────────────────────────────────

    #[test]
    fn alpha_blend_class() {
        let s = transform_from("alpha-blend");
        assert!(matches!(s.alpha_mode, Some(ByoAlphaMode::Blend)));
    }

    #[test]
    fn alpha_opaque_class() {
        let s = transform_from("alpha-opaque");
        assert!(matches!(s.alpha_mode, Some(ByoAlphaMode::Opaque)));
    }

    #[test]
    fn alpha_add_class() {
        let s = transform_from("alpha-add");
        assert!(matches!(s.alpha_mode, Some(ByoAlphaMode::Add)));
    }

    // ── Combined PBR ─────────────────────────────────────────────────

    #[test]
    fn combined_pbr_layer() {
        let s = transform_from(
            "base-zinc-800 emissive-red-500 metallic-100 roughness-25 \
             lit alpha-blend double-sided clearcoat-50 ior-[1.5]",
        );
        assert!(s.base_color.is_some());
        assert!(s.emissive_color.is_some());
        assert_eq!(s.metallic, Some(1.0));
        assert_eq!(s.perceptual_roughness, Some(0.25));
        assert_eq!(s.unlit, Some(false));
        assert!(matches!(s.alpha_mode, Some(ByoAlphaMode::Blend)));
        assert_eq!(s.double_sided, Some(true));
        assert_eq!(s.clearcoat, Some(0.5));
        assert_eq!(s.ior, Some(1.5));
    }

    // ── Format classes ──────────────────────────────────────────────

    #[test]
    fn format_rgba8unorm_srgb() {
        let s = transform_from("format-rgba8unorm-srgb");
        assert!(matches!(s.format, Some(ByoTextureFormat::Rgba8UnormSrgb)));
    }

    #[test]
    fn format_rgba8unorm() {
        let s = transform_from("format-rgba8unorm");
        assert!(matches!(s.format, Some(ByoTextureFormat::Rgba8Unorm)));
    }

    #[test]
    fn format_rgb10a2unorm() {
        let s = transform_from("format-rgb10a2unorm");
        assert!(matches!(s.format, Some(ByoTextureFormat::Rgb10a2Unorm)));
    }

    #[test]
    fn format_rgba16float() {
        let s = transform_from("format-rgba16float");
        assert!(matches!(s.format, Some(ByoTextureFormat::Rgba16Float)));
    }

    #[test]
    fn format_rgba32float() {
        let s = transform_from("format-rgba32float");
        assert!(matches!(s.format, Some(ByoTextureFormat::Rgba32Float)));
    }

    #[test]
    fn format_default_none() {
        let s = transform_from("lit alpha-blend");
        assert!(s.format.is_none());
    }

    // ── Transition classes (view-level) ─────────────────────────────

    #[test]
    fn transition_default() {
        let p = props_from("transition");
        assert!(p.tw_transition_property.is_some());
        assert_eq!(p.tw_transition_duration, Some(0.15));
        assert_eq!(p.tw_transition_easing, Some(EaseFn::SmoothStep));
    }

    #[test]
    fn transition_all() {
        let p = props_from("transition-all");
        assert!(matches!(
            p.tw_transition_property,
            Some(TransitionProperty::All)
        ));
        assert_eq!(p.tw_transition_duration, Some(0.15));
    }

    #[test]
    fn transition_colors() {
        let p = props_from("transition-colors");
        assert!(p.tw_transition_property.is_some());
        assert_eq!(p.tw_transition_duration, Some(0.15));
    }

    #[test]
    fn transition_opacity() {
        let p = props_from("transition-opacity");
        assert!(p.tw_transition_property.is_some());
    }

    #[test]
    fn transition_none() {
        let p = props_from("transition transition-none");
        assert!(p.tw_transition_property.is_none());
        assert!(p.tw_transition_duration.is_none());
        assert!(p.tw_transition_easing.is_none());
        assert!(p.tw_transition_delay.is_none());
    }

    #[test]
    fn ease_linear() {
        let p = props_from("transition ease-linear");
        assert_eq!(p.tw_transition_easing, Some(EaseFn::Linear));
    }

    #[test]
    fn ease_in() {
        let p = props_from("transition ease-in");
        assert_eq!(p.tw_transition_easing, Some(EaseFn::CubicIn));
    }

    #[test]
    fn ease_out() {
        let p = props_from("transition ease-out");
        assert_eq!(p.tw_transition_easing, Some(EaseFn::CubicOut));
    }

    #[test]
    fn ease_in_out() {
        let p = props_from("transition ease-in-out");
        assert_eq!(p.tw_transition_easing, Some(EaseFn::CubicInOut));
    }

    #[test]
    fn duration_150() {
        let p = props_from("transition duration-150");
        assert_eq!(p.tw_transition_duration, Some(0.15));
    }

    #[test]
    fn duration_300() {
        let p = props_from("transition duration-300");
        assert_eq!(p.tw_transition_duration, Some(0.3));
    }

    #[test]
    fn duration_1000() {
        let p = props_from("transition duration-1000");
        assert_eq!(p.tw_transition_duration, Some(1.0));
    }

    #[test]
    fn duration_arbitrary_ms() {
        let p = props_from("transition duration-[200ms]");
        assert_eq!(p.tw_transition_duration, Some(0.2));
    }

    #[test]
    fn duration_arbitrary_s() {
        let p = props_from("transition duration-[0.5s]");
        assert_eq!(p.tw_transition_duration, Some(0.5));
    }

    #[test]
    fn delay_100() {
        let p = props_from("transition delay-100");
        assert_eq!(p.tw_transition_delay, Some(0.1));
    }

    #[test]
    fn delay_arbitrary_ms() {
        let p = props_from("transition delay-[500ms]");
        assert_eq!(p.tw_transition_delay, Some(0.5));
    }

    #[test]
    fn transition_combined() {
        let p = props_from("transition-all duration-500 ease-out delay-100");
        assert!(matches!(
            p.tw_transition_property,
            Some(TransitionProperty::All)
        ));
        assert_eq!(p.tw_transition_duration, Some(0.5));
        assert_eq!(p.tw_transition_easing, Some(EaseFn::CubicOut));
        assert_eq!(p.tw_transition_delay, Some(0.1));
    }

    // ── Transition classes (transform/layer-level) ──────────────────

    #[test]
    fn transform_transition_default() {
        let s = transform_from("transition lit");
        assert!(s.tw_transition_property.is_some());
        assert_eq!(s.tw_transition_duration, Some(0.15));
        assert_eq!(s.tw_transition_easing, Some(EaseFn::SmoothStep));
    }

    #[test]
    fn transform_transition_all_with_duration() {
        let s = transform_from("transition-all duration-700 ease-in");
        assert!(matches!(
            s.tw_transition_property,
            Some(TransitionProperty::All)
        ));
        assert_eq!(s.tw_transition_duration, Some(0.7));
        assert_eq!(s.tw_transition_easing, Some(EaseFn::CubicIn));
    }

    #[test]
    fn transform_transition_none_clears() {
        let s = transform_from("transition-all duration-500 transition-none");
        assert!(s.tw_transition_property.is_none());
        assert!(s.tw_transition_duration.is_none());
    }

    #[test]
    fn transform_delay() {
        let s = transform_from("transition delay-[250ms]");
        assert_eq!(s.tw_transition_delay, Some(0.25));
    }
}
