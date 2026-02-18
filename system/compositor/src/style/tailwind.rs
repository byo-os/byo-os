//! Tailwind CSS class parser for BYO/OS compositor.
//!
//! Splits a `class` string on whitespace and applies each recognized utility
//! class by setting the corresponding field on [`ViewProps`]. Unknown classes
//! are silently ignored.

use bevy::prelude::*;

use crate::props::types::*;
use crate::props::view::ViewProps;
use crate::style::color::parse_color;
use crate::style::palette::tailwind_color;

/// Apply all Tailwind utility classes in `class_str` to `props`.
pub fn apply_classes(props: &mut ViewProps, class_str: &str) {
    for class in class_str.split_whitespace() {
        apply_class(props, class);
    }
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
    }

    // Unknown classes — silently ignored
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
}
