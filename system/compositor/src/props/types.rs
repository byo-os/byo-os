//! Compositor-local newtypes wrapping Bevy types with ReadProp/WriteProp.
//!
//! We can't impl `ReadProp` on `bevy::Val` directly (orphan rule), so each
//! Bevy type gets a newtype wrapper that knows how to parse from BYO props.

use bevy::prelude::*;
use byo::Prop;
use byo::props::{ReadProp, WriteProp};

// ---------------------------------------------------------------------------
// ByoVal — wraps bevy::Val
// ---------------------------------------------------------------------------

/// Parses `"256"` -> `Px(256.0)`, `"50%"` -> `Percent(50.0)`, `"auto"` -> `Auto`.
#[derive(Debug, Clone, Default)]
pub struct ByoVal(pub Val);

impl ReadProp for ByoVal {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(parsed) = parse_val(value.as_ref()) {
                    self.0 = parsed;
                }
            }
            Prop::Remove { .. } => self.0 = Val::Auto,
            _ => {}
        }
    }
}

impl WriteProp for ByoVal {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        let s = match self.0 {
            Val::Auto => "auto".to_string(),
            Val::Px(v) => format!("{v}"),
            Val::Percent(v) => format!("{v}%"),
            Val::Vw(v) => format!("{v}vw"),
            Val::Vh(v) => format!("{v}vh"),
            Val::VMin(v) => format!("{v}vmin"),
            Val::VMax(v) => format!("{v}vmax"),
        };
        out.push(Prop::val(key, s));
    }
}

pub fn parse_val(s: &str) -> Option<Val> {
    match s {
        "auto" => return Some(Val::Auto),
        "0" => return Some(Val::Px(0.0)),
        _ => {}
    }
    if let Some(rest) = s.strip_suffix("vmin") {
        return rest.parse::<f32>().ok().map(Val::VMin);
    }
    if let Some(rest) = s.strip_suffix("vmax") {
        return rest.parse::<f32>().ok().map(Val::VMax);
    }
    if let Some(rest) = s.strip_suffix("vw") {
        return rest.parse::<f32>().ok().map(Val::Vw);
    }
    if let Some(rest) = s.strip_suffix("vh") {
        return rest.parse::<f32>().ok().map(Val::Vh);
    }
    if let Some(rest) = s.strip_suffix('%') {
        return rest.parse::<f32>().ok().map(Val::Percent);
    }
    if let Some(rest) = s.strip_suffix("px") {
        return rest.parse::<f32>().ok().map(Val::Px);
    }
    // Bare number -> Px
    s.parse::<f32>().ok().map(Val::Px)
}

// ---------------------------------------------------------------------------
// ByoColor — wraps bevy::Color
// ---------------------------------------------------------------------------

/// Parses `"#ff0000"`, `"rgb(255,0,0)"`, `"red"`, `"transparent"`, etc.
#[derive(Debug, Clone)]
pub struct ByoColor(pub Color);

impl Default for ByoColor {
    fn default() -> Self {
        Self(Color::NONE)
    }
}

impl ReadProp for ByoColor {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(color) = crate::style::color::parse_color(value.as_ref()) {
                    self.0 = color;
                }
            }
            Prop::Remove { .. } => self.0 = Color::NONE,
            _ => {}
        }
    }
}

impl WriteProp for ByoColor {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        let Srgba {
            red,
            green,
            blue,
            alpha,
        } = self.0.to_srgba();
        let r = (red * 255.0) as u8;
        let g = (green * 255.0) as u8;
        let b = (blue * 255.0) as u8;
        if alpha < 1.0 {
            let a = (alpha * 255.0) as u8;
            out.push(Prop::val(key, format!("#{r:02x}{g:02x}{b:02x}{a:02x}")));
        } else {
            out.push(Prop::val(key, format!("#{r:02x}{g:02x}{b:02x}")));
        }
    }
}

// ---------------------------------------------------------------------------
// ByoRect — wraps bevy::UiRect
// ---------------------------------------------------------------------------

/// Parses shorthand: `"4"`, `"4 8"`, `"4 8 12 16"` (px values).
#[derive(Debug, Clone, Default)]
pub struct ByoRect(pub UiRect);

impl ReadProp for ByoRect {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(rect) = parse_rect(value.as_ref()) {
                    self.0 = rect;
                }
            }
            Prop::Remove { .. } => self.0 = UiRect::DEFAULT,
            _ => {}
        }
    }
}

impl WriteProp for ByoRect {
    fn encode(&self, _key: &str, _out: &mut Vec<Prop>) {
        // Not needed for compositor (write goes outbound, compositor is read-only)
    }
}

fn parse_rect(s: &str) -> Option<UiRect> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.len() {
        1 => {
            let v = parse_val(parts[0])?;
            Some(UiRect::all(v))
        }
        2 => {
            let vertical = parse_val(parts[0])?;
            let horizontal = parse_val(parts[1])?;
            Some(UiRect::axes(horizontal, vertical))
        }
        4 => {
            let top = parse_val(parts[0])?;
            let right = parse_val(parts[1])?;
            let bottom = parse_val(parts[2])?;
            let left = parse_val(parts[3])?;
            Some(UiRect {
                left,
                right,
                top,
                bottom,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ByoBorderRadius — wraps bevy::BorderRadius
// ---------------------------------------------------------------------------

/// Parses `"8"`, `"8 4"`, `"8 4 8 4"` (px corner radii).
#[derive(Debug, Clone, Default)]
pub struct ByoBorderRadius(pub BorderRadius);

impl ReadProp for ByoBorderRadius {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(br) = parse_border_radius(value.as_ref()) {
                    self.0 = br;
                }
            }
            Prop::Remove { .. } => self.0 = BorderRadius::DEFAULT,
            _ => {}
        }
    }
}

impl WriteProp for ByoBorderRadius {
    fn encode(&self, _key: &str, _out: &mut Vec<Prop>) {}
}

fn parse_border_radius(s: &str) -> Option<BorderRadius> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.len() {
        1 => {
            let v = parse_val(parts[0])?;
            Some(BorderRadius::all(v))
        }
        4 => {
            let tl = parse_val(parts[0])?;
            let tr = parse_val(parts[1])?;
            let br = parse_val(parts[2])?;
            let bl = parse_val(parts[3])?;
            Some(BorderRadius {
                top_left: tl,
                top_right: tr,
                bottom_right: br,
                bottom_left: bl,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Layout enums — mirror Bevy enums with ReadProp/WriteProp
// ---------------------------------------------------------------------------

macro_rules! byo_enum {
    (
        $(#[$meta:meta])*
        $name:ident => $bevy_ty:ty {
            $($variant:ident => $wire:literal => $bevy_variant:expr),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default)]
        pub enum $name {
            #[default]
            $($variant),+
        }

        impl $name {
            #[allow(dead_code)]
            pub fn to_bevy(&self) -> $bevy_ty {
                match self {
                    $(Self::$variant => $bevy_variant),+
                }
            }
        }

        impl ReadProp for $name {
            fn apply(&mut self, prop: &Prop) {
                if let Prop::Value { value, .. } = prop {
                    match value.as_ref() {
                        $($wire => *self = Self::$variant,)+
                        _ => {}
                    }
                }
            }
        }

        impl WriteProp for $name {
            fn encode(&self, key: &str, out: &mut Vec<Prop>) {
                let val = match self {
                    $(Self::$variant => $wire),+
                };
                out.push(Prop::val(key, val));
            }
        }
    };
}

byo_enum! {
    ByoDisplay => Display {
        Flex => "flex" => Display::Flex,
        Block => "block" => Display::Block,
        Grid => "grid" => Display::Grid,
        None => "none" => Display::None,
    }
}

byo_enum! {
    ByoFlexDirection => FlexDirection {
        Row => "row" => FlexDirection::Row,
        Column => "column" => FlexDirection::Column,
        RowReverse => "row-reverse" => FlexDirection::RowReverse,
        ColumnReverse => "column-reverse" => FlexDirection::ColumnReverse,
    }
}

byo_enum! {
    ByoAlignItems => AlignItems {
        Default => "default" => AlignItems::Default,
        Start => "start" => AlignItems::Start,
        End => "end" => AlignItems::End,
        FlexStart => "flex-start" => AlignItems::FlexStart,
        FlexEnd => "flex-end" => AlignItems::FlexEnd,
        Center => "center" => AlignItems::Center,
        Baseline => "baseline" => AlignItems::Baseline,
        Stretch => "stretch" => AlignItems::Stretch,
    }
}

byo_enum! {
    ByoJustifyContent => JustifyContent {
        Default => "default" => JustifyContent::Default,
        Start => "start" => JustifyContent::Start,
        End => "end" => JustifyContent::End,
        FlexStart => "flex-start" => JustifyContent::FlexStart,
        FlexEnd => "flex-end" => JustifyContent::FlexEnd,
        Center => "center" => JustifyContent::Center,
        Stretch => "stretch" => JustifyContent::Stretch,
        SpaceBetween => "space-between" => JustifyContent::SpaceBetween,
        SpaceEvenly => "space-evenly" => JustifyContent::SpaceEvenly,
        SpaceAround => "space-around" => JustifyContent::SpaceAround,
    }
}

byo_enum! {
    ByoFlexWrap => FlexWrap {
        NoWrap => "nowrap" => FlexWrap::NoWrap,
        Wrap => "wrap" => FlexWrap::Wrap,
        WrapReverse => "wrap-reverse" => FlexWrap::WrapReverse,
    }
}

byo_enum! {
    ByoOverflow => OverflowAxis {
        Visible => "visible" => OverflowAxis::Visible,
        Hidden => "hidden" => OverflowAxis::Hidden,
        Scroll => "scroll" => OverflowAxis::Scroll,
        Clip => "clip" => OverflowAxis::Clip,
    }
}

byo_enum! {
    ByoPositionType => PositionType {
        Relative => "relative" => PositionType::Relative,
        Absolute => "absolute" => PositionType::Absolute,
    }
}

byo_enum! {
    ByoAlignSelf => AlignSelf {
        Auto => "auto" => AlignSelf::Auto,
        Start => "start" => AlignSelf::Start,
        End => "end" => AlignSelf::End,
        FlexStart => "flex-start" => AlignSelf::FlexStart,
        FlexEnd => "flex-end" => AlignSelf::FlexEnd,
        Center => "center" => AlignSelf::Center,
        Baseline => "baseline" => AlignSelf::Baseline,
        Stretch => "stretch" => AlignSelf::Stretch,
    }
}

byo_enum! {
    ByoTextAlign => bevy::text::Justify {
        Left => "left" => bevy::text::Justify::Left,
        Center => "center" => bevy::text::Justify::Center,
        Right => "right" => bevy::text::Justify::Right,
        Justified => "justify" => bevy::text::Justify::Justified,
    }
}

// Note: ByoOrderMode doesn't map to a Bevy type, so we use a dummy target.
// The to_bevy() method isn't used; we match on the enum directly.
byo_enum! {
    /// Controls how `order` is applied in 3D space.
    ByoOrderMode => u8 {
        TranslateZ => "translate-z" => 0,
        DepthBias => "depth-bias" => 1,
        None => "none" => 2,
    }
}

byo_enum! {
    ByoAlphaMode => AlphaMode {
        Opaque => "opaque" => AlphaMode::Opaque,
        Blend => "blend" => AlphaMode::Blend,
        Premultiplied => "premultiplied" => AlphaMode::Premultiplied,
        Add => "add" => AlphaMode::Add,
        Multiply => "multiply" => AlphaMode::Multiply,
    }
}

// ---------------------------------------------------------------------------
// ByoTextureFormat — layer render texture pixel format
// ---------------------------------------------------------------------------

/// Controls the pixel format of a layer's render texture.
/// Wire values use kebab-cased WebGPU format names (auto-derived from PascalCase).
#[derive(Debug, Clone, Default, byo::ReadProp, byo::WriteProp)]
pub enum ByoTextureFormat {
    #[default]
    Rgba8UnormSrgb,
    Rgba8Unorm,
    Rgb10a2Unorm,
    Rgba16Float,
    Rgba32Float,
}

impl ByoTextureFormat {
    pub fn to_wgpu(&self) -> bevy::render::render_resource::TextureFormat {
        use bevy::render::render_resource::TextureFormat;
        match self {
            Self::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
            Self::Rgba8Unorm => TextureFormat::Rgba8Unorm,
            Self::Rgb10a2Unorm => TextureFormat::Rgb10a2Unorm,
            Self::Rgba16Float => TextureFormat::Rgba16Float,
            Self::Rgba32Float => TextureFormat::Rgba32Float,
        }
    }

    /// Bytes-per-pixel for the format (used for zero-fill at creation).
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Rgba8UnormSrgb | Self::Rgba8Unorm | Self::Rgb10a2Unorm => 4,
            Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
        }
    }

    /// Whether this format supports HDR values (>1.0 per channel).
    pub fn is_hdr(&self) -> bool {
        matches!(self, Self::Rgba16Float | Self::Rgba32Float)
    }
}

// ---------------------------------------------------------------------------
// ByoCullMode — maps to Option<bevy::render::render_resource::Face>
// ---------------------------------------------------------------------------

/// Controls face culling on layer materials.
/// - `none` → render both sides (no culling)
/// - `front` → cull front faces
/// - `back` → cull back faces (default)
#[derive(Debug, Clone, Default)]
pub enum ByoCullMode {
    None,
    Front,
    #[default]
    Back,
}

impl ByoCullMode {
    pub fn to_face(&self) -> Option<bevy::render::render_resource::Face> {
        match self {
            Self::None => None,
            Self::Front => Some(bevy::render::render_resource::Face::Front),
            Self::Back => Some(bevy::render::render_resource::Face::Back),
        }
    }
}

impl ReadProp for ByoCullMode {
    fn apply(&mut self, prop: &Prop) {
        if let Prop::Value { value, .. } = prop {
            match value.as_ref() {
                "none" => *self = Self::None,
                "front" => *self = Self::Front,
                "back" => *self = Self::Back,
                _ => {}
            }
        }
    }
}

impl WriteProp for ByoCullMode {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        let val = match self {
            Self::None => "none",
            Self::Front => "front",
            Self::Back => "back",
        };
        out.push(Prop::val(key, val));
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_val_bare_number() {
        assert_eq!(parse_val("256"), Some(Val::Px(256.0)));
    }

    #[test]
    fn parse_val_px_suffix() {
        assert_eq!(parse_val("100px"), Some(Val::Px(100.0)));
    }

    #[test]
    fn parse_val_percent() {
        assert_eq!(parse_val("50%"), Some(Val::Percent(50.0)));
    }

    #[test]
    fn parse_val_auto() {
        assert_eq!(parse_val("auto"), Some(Val::Auto));
    }

    #[test]
    fn parse_val_vw() {
        assert_eq!(parse_val("100vw"), Some(Val::Vw(100.0)));
    }

    #[test]
    fn parse_val_zero() {
        assert_eq!(parse_val("0"), Some(Val::Px(0.0)));
    }

    #[test]
    fn parse_val_invalid() {
        assert_eq!(parse_val("abc"), None);
    }

    #[test]
    fn parse_rect_single() {
        let r = parse_rect("4").unwrap();
        assert_eq!(r, UiRect::all(Val::Px(4.0)));
    }

    #[test]
    fn parse_rect_two() {
        let r = parse_rect("4 8").unwrap();
        assert_eq!(r.top, Val::Px(4.0));
        assert_eq!(r.right, Val::Px(8.0));
        assert_eq!(r.bottom, Val::Px(4.0));
        assert_eq!(r.left, Val::Px(8.0));
    }

    #[test]
    fn parse_rect_four() {
        let r = parse_rect("1 2 3 4").unwrap();
        assert_eq!(r.top, Val::Px(1.0));
        assert_eq!(r.right, Val::Px(2.0));
        assert_eq!(r.bottom, Val::Px(3.0));
        assert_eq!(r.left, Val::Px(4.0));
    }

    #[test]
    fn byo_val_read_write_round_trip() {
        let mut v = ByoVal::default();
        v.apply(&Prop::val("width", "100"));
        assert_eq!(v.0, Val::Px(100.0));

        let mut out = Vec::new();
        v.encode("width", &mut out);
        assert_eq!(out[0], Prop::val("width", "100"));
    }

    #[test]
    fn byo_display_read() {
        let mut d = ByoDisplay::default();
        d.apply(&Prop::val("display", "grid"));
        assert!(matches!(d, ByoDisplay::Grid));
        assert!(matches!(d.to_bevy(), Display::Grid));
    }
}
