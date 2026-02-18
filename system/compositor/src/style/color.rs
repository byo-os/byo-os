//! CSS color parser — hex, rgb(), rgba(), hsl(), hsla(), and named colors.

use bevy::prelude::*;

/// Parse a CSS color string into a Bevy `Color`.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();

    // Named colors
    if let Some(color) = parse_named_color(s) {
        return Some(color);
    }

    // Hex colors
    if let Some(rest) = s.strip_prefix('#') {
        return parse_hex(rest);
    }

    // rgb()/rgba()
    if let Some(rest) = s.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')) {
        return parse_rgb_values(rest);
    }
    if let Some(rest) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        return parse_rgb_values(rest);
    }

    // hsl()/hsla()
    if let Some(rest) = s.strip_prefix("hsla(").and_then(|r| r.strip_suffix(')')) {
        return parse_hsl_values(rest);
    }
    if let Some(rest) = s.strip_prefix("hsl(").and_then(|r| r.strip_suffix(')')) {
        return parse_hsl_values(rest);
    }

    None
}

fn parse_hex(hex: &str) -> Option<Color> {
    match hex.len() {
        // #rgb
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some(Color::srgba_u8(r * 17, g * 17, b * 17, 255))
        }
        // #rgba
        4 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()?;
            Some(Color::srgba_u8(r * 17, g * 17, b * 17, a * 17))
        }
        // #rrggbb
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::srgba_u8(r, g, b, 255))
        }
        // #rrggbbaa
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::srgba_u8(r, g, b, a))
        }
        _ => None,
    }
}

fn split_color_args(inner: &str) -> Vec<&str> {
    inner
        .split([',', ' ', '/'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_rgb_values(inner: &str) -> Option<Color> {
    let parts = split_color_args(inner);

    match parts.len() {
        3 => {
            let r = parse_color_component(parts[0])?;
            let g = parse_color_component(parts[1])?;
            let b = parse_color_component(parts[2])?;
            Some(Color::srgba(r, g, b, 1.0))
        }
        4 => {
            let r = parse_color_component(parts[0])?;
            let g = parse_color_component(parts[1])?;
            let b = parse_color_component(parts[2])?;
            let a = parse_alpha_component(parts[3])?;
            Some(Color::srgba(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_hsl_values(inner: &str) -> Option<Color> {
    let parts = split_color_args(inner);

    let (h, s, l, a) = match parts.len() {
        3 => {
            let h = parse_hue(parts[0])?;
            let s = parse_percent_or_fraction(parts[1])?;
            let l = parse_percent_or_fraction(parts[2])?;
            (h, s, l, 1.0)
        }
        4 => {
            let h = parse_hue(parts[0])?;
            let s = parse_percent_or_fraction(parts[1])?;
            let l = parse_percent_or_fraction(parts[2])?;
            let a = parse_alpha_component(parts[3])?;
            (h, s, l, a)
        }
        _ => return None,
    };

    let (r, g, b) = hsl_to_rgb(h, s, l);
    Some(Color::srgba(r, g, b, a))
}

/// Parse hue: bare number (degrees), or with `deg`/`turn`/`rad` suffix.
fn parse_hue(s: &str) -> Option<f32> {
    if let Some(rest) = s.strip_suffix("deg") {
        return rest.parse::<f32>().ok();
    }
    if let Some(rest) = s.strip_suffix("turn") {
        return rest.parse::<f32>().ok().map(|v| v * 360.0);
    }
    if let Some(rest) = s.strip_suffix("rad") {
        return rest.parse::<f32>().ok().map(|v| v.to_degrees());
    }
    s.parse::<f32>().ok()
}

/// Parse `"50%"` -> 0.5, or bare `"0.5"` -> 0.5
fn parse_percent_or_fraction(s: &str) -> Option<f32> {
    if let Some(pct) = s.strip_suffix('%') {
        return pct.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

/// Parse `"255"` as 0..=255 -> 0.0..=1.0, or `"50%"` -> 0.5
fn parse_color_component(s: &str) -> Option<f32> {
    if let Some(pct) = s.strip_suffix('%') {
        return pct.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0));
    }
    s.parse::<f32>().ok().map(|v| (v / 255.0).clamp(0.0, 1.0))
}

/// Parse alpha: `"0.5"` -> 0.5, `"50%"` -> 0.5
fn parse_alpha_component(s: &str) -> Option<f32> {
    if let Some(pct) = s.strip_suffix('%') {
        return pct.parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0));
    }
    s.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

/// HSL to RGB conversion. h in degrees, s and l in 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }

    let h = ((h % 360.0) + 360.0) % 360.0 / 360.0;
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);

    (r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

fn parse_named_color(name: &str) -> Option<Color> {
    // Full CSS named colors (148 colors)
    Some(match name {
        "transparent" => Color::NONE,
        "aliceblue" => Color::srgba_u8(240, 248, 255, 255),
        "antiquewhite" => Color::srgba_u8(250, 235, 215, 255),
        "aqua" | "cyan" => Color::srgba_u8(0, 255, 255, 255),
        "aquamarine" => Color::srgba_u8(127, 255, 212, 255),
        "azure" => Color::srgba_u8(240, 255, 255, 255),
        "beige" => Color::srgba_u8(245, 245, 220, 255),
        "bisque" => Color::srgba_u8(255, 228, 196, 255),
        "black" => Color::srgba_u8(0, 0, 0, 255),
        "blanchedalmond" => Color::srgba_u8(255, 235, 205, 255),
        "blue" => Color::srgba_u8(0, 0, 255, 255),
        "blueviolet" => Color::srgba_u8(138, 43, 226, 255),
        "brown" => Color::srgba_u8(165, 42, 42, 255),
        "burlywood" => Color::srgba_u8(222, 184, 135, 255),
        "cadetblue" => Color::srgba_u8(95, 158, 160, 255),
        "chartreuse" => Color::srgba_u8(127, 255, 0, 255),
        "chocolate" => Color::srgba_u8(210, 105, 30, 255),
        "coral" => Color::srgba_u8(255, 127, 80, 255),
        "cornflowerblue" => Color::srgba_u8(100, 149, 237, 255),
        "cornsilk" => Color::srgba_u8(255, 248, 220, 255),
        "crimson" => Color::srgba_u8(220, 20, 60, 255),
        "darkblue" => Color::srgba_u8(0, 0, 139, 255),
        "darkcyan" => Color::srgba_u8(0, 139, 139, 255),
        "darkgoldenrod" => Color::srgba_u8(184, 134, 11, 255),
        "darkgray" | "darkgrey" => Color::srgba_u8(169, 169, 169, 255),
        "darkgreen" => Color::srgba_u8(0, 100, 0, 255),
        "darkkhaki" => Color::srgba_u8(189, 183, 107, 255),
        "darkmagenta" => Color::srgba_u8(139, 0, 139, 255),
        "darkolivegreen" => Color::srgba_u8(85, 107, 47, 255),
        "darkorange" => Color::srgba_u8(255, 140, 0, 255),
        "darkorchid" => Color::srgba_u8(153, 50, 204, 255),
        "darkred" => Color::srgba_u8(139, 0, 0, 255),
        "darksalmon" => Color::srgba_u8(233, 150, 122, 255),
        "darkseagreen" => Color::srgba_u8(143, 188, 143, 255),
        "darkslateblue" => Color::srgba_u8(72, 61, 139, 255),
        "darkslategray" | "darkslategrey" => Color::srgba_u8(47, 79, 79, 255),
        "darkturquoise" => Color::srgba_u8(0, 206, 209, 255),
        "darkviolet" => Color::srgba_u8(148, 0, 211, 255),
        "deeppink" => Color::srgba_u8(255, 20, 147, 255),
        "deepskyblue" => Color::srgba_u8(0, 191, 255, 255),
        "dimgray" | "dimgrey" => Color::srgba_u8(105, 105, 105, 255),
        "dodgerblue" => Color::srgba_u8(30, 144, 255, 255),
        "firebrick" => Color::srgba_u8(178, 34, 34, 255),
        "floralwhite" => Color::srgba_u8(255, 250, 240, 255),
        "forestgreen" => Color::srgba_u8(34, 139, 34, 255),
        "fuchsia" | "magenta" => Color::srgba_u8(255, 0, 255, 255),
        "gainsboro" => Color::srgba_u8(220, 220, 220, 255),
        "ghostwhite" => Color::srgba_u8(248, 248, 255, 255),
        "gold" => Color::srgba_u8(255, 215, 0, 255),
        "goldenrod" => Color::srgba_u8(218, 165, 32, 255),
        "gray" | "grey" => Color::srgba_u8(128, 128, 128, 255),
        "green" => Color::srgba_u8(0, 128, 0, 255),
        "greenyellow" => Color::srgba_u8(173, 255, 47, 255),
        "honeydew" => Color::srgba_u8(240, 255, 240, 255),
        "hotpink" => Color::srgba_u8(255, 105, 180, 255),
        "indianred" => Color::srgba_u8(205, 92, 92, 255),
        "indigo" => Color::srgba_u8(75, 0, 130, 255),
        "ivory" => Color::srgba_u8(255, 255, 240, 255),
        "khaki" => Color::srgba_u8(240, 230, 140, 255),
        "lavender" => Color::srgba_u8(230, 230, 250, 255),
        "lavenderblush" => Color::srgba_u8(255, 240, 245, 255),
        "lawngreen" => Color::srgba_u8(124, 252, 0, 255),
        "lemonchiffon" => Color::srgba_u8(255, 250, 205, 255),
        "lightblue" => Color::srgba_u8(173, 216, 230, 255),
        "lightcoral" => Color::srgba_u8(240, 128, 128, 255),
        "lightcyan" => Color::srgba_u8(224, 255, 255, 255),
        "lightgoldenrodyellow" => Color::srgba_u8(250, 250, 210, 255),
        "lightgray" | "lightgrey" => Color::srgba_u8(211, 211, 211, 255),
        "lightgreen" => Color::srgba_u8(144, 238, 144, 255),
        "lightpink" => Color::srgba_u8(255, 182, 193, 255),
        "lightsalmon" => Color::srgba_u8(255, 160, 122, 255),
        "lightseagreen" => Color::srgba_u8(32, 178, 170, 255),
        "lightskyblue" => Color::srgba_u8(135, 206, 250, 255),
        "lightslategray" | "lightslategrey" => Color::srgba_u8(119, 136, 153, 255),
        "lightsteelblue" => Color::srgba_u8(176, 196, 222, 255),
        "lightyellow" => Color::srgba_u8(255, 255, 224, 255),
        "lime" => Color::srgba_u8(0, 255, 0, 255),
        "limegreen" => Color::srgba_u8(50, 205, 50, 255),
        "linen" => Color::srgba_u8(250, 240, 230, 255),
        "maroon" => Color::srgba_u8(128, 0, 0, 255),
        "mediumaquamarine" => Color::srgba_u8(102, 205, 170, 255),
        "mediumblue" => Color::srgba_u8(0, 0, 205, 255),
        "mediumorchid" => Color::srgba_u8(186, 85, 211, 255),
        "mediumpurple" => Color::srgba_u8(147, 112, 219, 255),
        "mediumseagreen" => Color::srgba_u8(60, 179, 113, 255),
        "mediumslateblue" => Color::srgba_u8(123, 104, 238, 255),
        "mediumspringgreen" => Color::srgba_u8(0, 250, 154, 255),
        "mediumturquoise" => Color::srgba_u8(72, 209, 204, 255),
        "mediumvioletred" => Color::srgba_u8(199, 21, 133, 255),
        "midnightblue" => Color::srgba_u8(25, 25, 112, 255),
        "mintcream" => Color::srgba_u8(245, 255, 250, 255),
        "mistyrose" => Color::srgba_u8(255, 228, 225, 255),
        "moccasin" => Color::srgba_u8(255, 228, 181, 255),
        "navajowhite" => Color::srgba_u8(255, 222, 173, 255),
        "navy" => Color::srgba_u8(0, 0, 128, 255),
        "oldlace" => Color::srgba_u8(253, 245, 230, 255),
        "olive" => Color::srgba_u8(128, 128, 0, 255),
        "olivedrab" => Color::srgba_u8(107, 142, 35, 255),
        "orange" => Color::srgba_u8(255, 165, 0, 255),
        "orangered" => Color::srgba_u8(255, 69, 0, 255),
        "orchid" => Color::srgba_u8(218, 112, 214, 255),
        "palegoldenrod" => Color::srgba_u8(238, 232, 170, 255),
        "palegreen" => Color::srgba_u8(152, 251, 152, 255),
        "paleturquoise" => Color::srgba_u8(175, 238, 238, 255),
        "palevioletred" => Color::srgba_u8(219, 112, 147, 255),
        "papayawhip" => Color::srgba_u8(255, 239, 213, 255),
        "peachpuff" => Color::srgba_u8(255, 218, 185, 255),
        "peru" => Color::srgba_u8(205, 133, 63, 255),
        "pink" => Color::srgba_u8(255, 192, 203, 255),
        "plum" => Color::srgba_u8(221, 160, 221, 255),
        "powderblue" => Color::srgba_u8(176, 224, 230, 255),
        "purple" => Color::srgba_u8(128, 0, 128, 255),
        "rebeccapurple" => Color::srgba_u8(102, 51, 153, 255),
        "red" => Color::srgba_u8(255, 0, 0, 255),
        "rosybrown" => Color::srgba_u8(188, 143, 143, 255),
        "royalblue" => Color::srgba_u8(65, 105, 225, 255),
        "saddlebrown" => Color::srgba_u8(139, 69, 19, 255),
        "salmon" => Color::srgba_u8(250, 128, 114, 255),
        "sandybrown" => Color::srgba_u8(244, 164, 96, 255),
        "seagreen" => Color::srgba_u8(46, 139, 87, 255),
        "seashell" => Color::srgba_u8(255, 245, 238, 255),
        "sienna" => Color::srgba_u8(160, 82, 45, 255),
        "silver" => Color::srgba_u8(192, 192, 192, 255),
        "skyblue" => Color::srgba_u8(135, 206, 235, 255),
        "slateblue" => Color::srgba_u8(106, 90, 205, 255),
        "slategray" | "slategrey" => Color::srgba_u8(112, 128, 144, 255),
        "snow" => Color::srgba_u8(255, 250, 250, 255),
        "springgreen" => Color::srgba_u8(0, 255, 127, 255),
        "steelblue" => Color::srgba_u8(70, 130, 180, 255),
        "tan" => Color::srgba_u8(210, 180, 140, 255),
        "teal" => Color::srgba_u8(0, 128, 128, 255),
        "thistle" => Color::srgba_u8(216, 191, 216, 255),
        "tomato" => Color::srgba_u8(255, 99, 71, 255),
        "turquoise" => Color::srgba_u8(64, 224, 208, 255),
        "violet" => Color::srgba_u8(238, 130, 238, 255),
        "wheat" => Color::srgba_u8(245, 222, 179, 255),
        "white" => Color::srgba_u8(255, 255, 255, 255),
        "whitesmoke" => Color::srgba_u8(245, 245, 245, 255),
        "yellow" => Color::srgba_u8(255, 255, 0, 255),
        "yellowgreen" => Color::srgba_u8(154, 205, 50, 255),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_color_eq(a: Color, b: Color) {
        let a = a.to_srgba();
        let b = b.to_srgba();
        assert!(
            (a.red - b.red).abs() < 0.02
                && (a.green - b.green).abs() < 0.02
                && (a.blue - b.blue).abs() < 0.02
                && (a.alpha - b.alpha).abs() < 0.02,
            "Colors differ: {a:?} vs {b:?}"
        );
    }

    #[test]
    fn hex_6() {
        assert_color_eq(
            parse_color("#ff0000").unwrap(),
            Color::srgba_u8(255, 0, 0, 255),
        );
    }

    #[test]
    fn hex_3() {
        assert_color_eq(
            parse_color("#f00").unwrap(),
            Color::srgba_u8(255, 0, 0, 255),
        );
    }

    #[test]
    fn hex_8() {
        assert_color_eq(
            parse_color("#ff000080").unwrap(),
            Color::srgba_u8(255, 0, 0, 128),
        );
    }

    #[test]
    fn hex_4() {
        assert_color_eq(
            parse_color("#f008").unwrap(),
            Color::srgba_u8(255, 0, 0, 136),
        );
    }

    #[test]
    fn rgb_comma() {
        assert_color_eq(
            parse_color("rgb(255, 128, 0)").unwrap(),
            Color::srgba_u8(255, 128, 0, 255),
        );
    }

    #[test]
    fn rgb_space_separated() {
        assert_color_eq(
            parse_color("rgb(255 128 0)").unwrap(),
            Color::srgba_u8(255, 128, 0, 255),
        );
    }

    #[test]
    fn rgba_comma() {
        assert_color_eq(
            parse_color("rgba(255, 0, 0, 0.5)").unwrap(),
            Color::srgba(1.0, 0.0, 0.0, 0.5),
        );
    }

    #[test]
    fn rgb_with_alpha_slash() {
        assert_color_eq(
            parse_color("rgb(255 0 0 / 0.5)").unwrap(),
            Color::srgba(1.0, 0.0, 0.0, 0.5),
        );
    }

    #[test]
    fn hsl_basic() {
        // hsl(0, 100%, 50%) = pure red
        assert_color_eq(
            parse_color("hsl(0, 100%, 50%)").unwrap(),
            Color::srgba_u8(255, 0, 0, 255),
        );
    }

    #[test]
    fn hsl_green() {
        // hsl(120, 100%, 50%) = pure green
        assert_color_eq(
            parse_color("hsl(120, 100%, 50%)").unwrap(),
            Color::srgba_u8(0, 255, 0, 255),
        );
    }

    #[test]
    fn hsl_blue() {
        // hsl(240, 100%, 50%) = pure blue
        assert_color_eq(
            parse_color("hsl(240, 100%, 50%)").unwrap(),
            Color::srgba_u8(0, 0, 255, 255),
        );
    }

    #[test]
    fn hsla_with_alpha() {
        assert_color_eq(
            parse_color("hsla(0, 100%, 50%, 0.5)").unwrap(),
            Color::srgba(1.0, 0.0, 0.0, 0.5),
        );
    }

    #[test]
    fn hsl_space_separated() {
        assert_color_eq(
            parse_color("hsl(0 100% 50%)").unwrap(),
            Color::srgba_u8(255, 0, 0, 255),
        );
    }

    #[test]
    fn hsl_gray() {
        // hsl(0, 0%, 50%) = gray
        assert_color_eq(
            parse_color("hsl(0, 0%, 50%)").unwrap(),
            Color::srgba_u8(128, 128, 128, 255),
        );
    }

    #[test]
    fn named_transparent() {
        assert_eq!(parse_color("transparent"), Some(Color::NONE));
    }

    #[test]
    fn named_white() {
        assert_color_eq(
            parse_color("white").unwrap(),
            Color::srgba_u8(255, 255, 255, 255),
        );
    }

    #[test]
    fn named_cornflowerblue() {
        assert_color_eq(
            parse_color("cornflowerblue").unwrap(),
            Color::srgba_u8(100, 149, 237, 255),
        );
    }

    #[test]
    fn named_rebeccapurple() {
        assert_color_eq(
            parse_color("rebeccapurple").unwrap(),
            Color::srgba_u8(102, 51, 153, 255),
        );
    }

    #[test]
    fn unknown_returns_none() {
        assert_eq!(parse_color("notacolor"), None);
    }
}
