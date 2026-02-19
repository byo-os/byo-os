//! CSS `box-shadow` parser.
//!
//! Parses values like `"0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1)"`.
//! Returns a `Vec<ByoShadow>` (empty for `"none"`).

use bevy::prelude::*;

use crate::props::types::{ByoShadow, parse_val};
use crate::style::color::parse_color;

/// Parse a CSS `box-shadow` value into a list of shadows.
///
/// Supports:
/// - `none` → empty vec
/// - Multiple shadows separated by commas (respects parentheses)
/// - Each shadow: `[inset] <lengths> [color]` or `[color] [inset] <lengths>`
/// - 2 lengths → x, y (blur=0, spread=0)
/// - 3 lengths → x, y, blur (spread=0)
/// - 4 lengths → x, y, blur, spread
/// - Color can appear before or after the lengths
/// - Default color: `rgba(0,0,0,1)` (opaque black)
pub fn parse_box_shadow(value: &str) -> Vec<ByoShadow> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
        return Vec::new();
    }

    split_respecting_parens(trimmed)
        .iter()
        .filter_map(|part| parse_single_shadow(part.trim()))
        .collect()
}

/// Split by comma, but not inside parentheses.
fn split_respecting_parens(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Parse a single shadow entry (one item from the comma-separated list).
fn parse_single_shadow(s: &str) -> Option<ByoShadow> {
    let tokens = tokenize_shadow(s);
    if tokens.is_empty() {
        return None;
    }

    let mut inset = false;
    let mut color: Option<Color> = None;
    let mut lengths: Vec<Val> = Vec::new();

    for token in &tokens {
        if token.eq_ignore_ascii_case("inset") {
            inset = true;
        } else if let Some(val) = try_parse_length(token) {
            lengths.push(val);
        } else if let Some(c) = parse_color(token) {
            color = Some(c);
        }
        // Unknown tokens silently ignored
    }

    // Need at least x and y offsets
    if lengths.len() < 2 {
        return None;
    }

    Some(ByoShadow {
        x_offset: lengths[0],
        y_offset: lengths[1],
        blur_radius: lengths.get(2).copied().unwrap_or(Val::Px(0.0)),
        spread_radius: lengths.get(3).copied().unwrap_or(Val::Px(0.0)),
        color: color.unwrap_or(Color::srgba(0.0, 0.0, 0.0, 1.0)),
        inset,
    })
}

/// Tokenize a shadow string, keeping function calls like `rgb(...)` as single tokens.
fn tokenize_shadow(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;
        // Check if this is a function call (contains '(')
        let mut depth = 0u32;
        while i < len {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                b' ' | b'\t' if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }

        let token = &s[start..i];
        if !token.is_empty() {
            tokens.push(token);
        }
    }

    tokens
}

/// Try to parse a token as a CSS length value.
/// Returns None if it looks like a color or keyword.
fn try_parse_length(token: &str) -> Option<Val> {
    let first = token.as_bytes().first()?;
    // Must start with digit, minus sign, or dot
    if first.is_ascii_digit() || *first == b'-' || *first == b'.' {
        parse_val(token)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_none() {
        assert!(parse_box_shadow("none").is_empty());
        assert!(parse_box_shadow("").is_empty());
    }

    #[test]
    fn parse_two_lengths() {
        let shadows = parse_box_shadow("2px 4px");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].x_offset, Val::Px(2.0));
        assert_eq!(shadows[0].y_offset, Val::Px(4.0));
        assert_eq!(shadows[0].blur_radius, Val::Px(0.0));
        assert_eq!(shadows[0].spread_radius, Val::Px(0.0));
        assert!(!shadows[0].inset);
    }

    #[test]
    fn parse_three_lengths() {
        let shadows = parse_box_shadow("0 4px 6px");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].x_offset, Val::Px(0.0));
        assert_eq!(shadows[0].y_offset, Val::Px(4.0));
        assert_eq!(shadows[0].blur_radius, Val::Px(6.0));
        assert_eq!(shadows[0].spread_radius, Val::Px(0.0));
    }

    #[test]
    fn parse_four_lengths() {
        let shadows = parse_box_shadow("0 4px 6px -1px");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].spread_radius, Val::Px(-1.0));
    }

    #[test]
    fn parse_with_color_last() {
        let shadows = parse_box_shadow("0 4px 6px rgba(0,0,0,0.1)");
        assert_eq!(shadows.len(), 1);
        let c = shadows[0].color.to_srgba();
        assert!((c.alpha - 0.1).abs() < 0.02);
    }

    #[test]
    fn parse_with_color_first() {
        let shadows = parse_box_shadow("rgba(0,0,0,0.1) 0 4px 6px");
        assert_eq!(shadows.len(), 1);
        let c = shadows[0].color.to_srgba();
        assert!((c.alpha - 0.1).abs() < 0.02);
        assert_eq!(shadows[0].blur_radius, Val::Px(6.0));
    }

    #[test]
    fn parse_with_hex_color() {
        let shadows = parse_box_shadow("0 1px 3px #ff0000");
        assert_eq!(shadows.len(), 1);
        let c = shadows[0].color.to_srgba();
        assert!((c.red - 1.0).abs() < 0.02);
    }

    #[test]
    fn parse_inset() {
        let shadows = parse_box_shadow("inset 0 2px 4px rgba(0,0,0,0.06)");
        assert_eq!(shadows.len(), 1);
        assert!(shadows[0].inset);
    }

    #[test]
    fn parse_multiple_shadows() {
        let shadows =
            parse_box_shadow("0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1)");
        assert_eq!(shadows.len(), 2);
        assert_eq!(shadows[0].spread_radius, Val::Px(-1.0));
        assert_eq!(shadows[1].spread_radius, Val::Px(-2.0));
    }

    #[test]
    fn parse_negative_spread() {
        let shadows = parse_box_shadow("0 10px 15px -3px black");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].spread_radius, Val::Px(-3.0));
    }

    #[test]
    fn parse_no_color_defaults_to_black() {
        let shadows = parse_box_shadow("0 4px 6px");
        assert_eq!(shadows.len(), 1);
        let c = shadows[0].color.to_srgba();
        assert!((c.red).abs() < 0.01);
        assert!((c.green).abs() < 0.01);
        assert!((c.blue).abs() < 0.01);
        assert!((c.alpha - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_hsl_color() {
        let shadows = parse_box_shadow("0 4px 6px hsl(0,100%,50%)");
        assert_eq!(shadows.len(), 1);
        let c = shadows[0].color.to_srgba();
        assert!((c.red - 1.0).abs() < 0.02);
    }
}
