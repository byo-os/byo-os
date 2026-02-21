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
#[derive(Debug, Clone, Default, PartialEq)]
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
// ByoAngle — stores rotation as radians, parses deg/rad/turn/pi/tau
// ---------------------------------------------------------------------------

/// Stores a rotation in radians. Parses unit suffixes; bare numbers default to degrees.
///
/// Supported units: `deg`, `rad`, `grad`, `turn`, `pi`, `tau` (= `turn`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ByoAngle(pub f32);

impl ReadProp for ByoAngle {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(radians) = parse_angle(value.as_ref()) {
                    self.0 = radians;
                }
            }
            Prop::Remove { .. } => self.0 = 0.0,
            _ => {}
        }
    }
}

impl WriteProp for ByoAngle {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        // Encode as degrees for wire readability
        let deg = self.0.to_degrees();
        out.push(Prop::val(key, format!("{deg}")));
    }
}

/// Parse an angle string, returning radians. Bare numbers default to degrees.
pub fn parse_angle(s: &str) -> Option<f32> {
    if let Some(rest) = s.strip_suffix("tau") {
        return rest.parse::<f32>().ok().map(|v| v * std::f32::consts::TAU);
    }
    if let Some(rest) = s.strip_suffix("turn") {
        return rest.parse::<f32>().ok().map(|v| v * std::f32::consts::TAU);
    }
    if let Some(rest) = s.strip_suffix("grad") {
        return rest
            .parse::<f32>()
            .ok()
            .map(|v| v * std::f32::consts::TAU / 400.0);
    }
    if let Some(rest) = s.strip_suffix("rad") {
        return rest.parse::<f32>().ok();
    }
    if let Some(rest) = s.strip_suffix("pi") {
        return rest.parse::<f32>().ok().map(|v| v * std::f32::consts::PI);
    }
    if let Some(rest) = s.strip_suffix("deg") {
        return rest.parse::<f32>().ok().map(|v| v.to_radians());
    }
    // Bare number → degrees (Tailwind/CSS convention)
    s.parse::<f32>().ok().map(|v| v.to_radians())
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
// ByoPointerEvents — maps to CSS pointer-events: auto/none
// ---------------------------------------------------------------------------

/// Controls whether an element participates in hit testing.
/// - `auto` → normal hit testing (default)
/// - `none` → transparent to pointer events (like CSS `pointer-events: none`)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ByoPointerEvents {
    #[default]
    Auto,
    None,
}

impl ReadProp for ByoPointerEvents {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => match value.as_ref() {
                "auto" => *self = Self::Auto,
                "none" => *self = Self::None,
                _ => {}
            },
            Prop::Remove { .. } => *self = Self::Auto,
            _ => {}
        }
    }
}

impl WriteProp for ByoPointerEvents {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        let val = match self {
            Self::Auto => "auto",
            Self::None => "none",
        };
        out.push(Prop::val(key, val));
    }
}

// ---------------------------------------------------------------------------
// ByoShadow — parsed CSS box-shadow value
// ---------------------------------------------------------------------------

/// A single parsed CSS box-shadow value.
#[derive(Debug, Clone, PartialEq)]
pub struct ByoShadow {
    pub color: Color,
    pub x_offset: Val,
    pub y_offset: Val,
    pub blur_radius: Val,
    pub spread_radius: Val,
    pub inset: bool,
}

impl Default for ByoShadow {
    fn default() -> Self {
        Self {
            color: Color::srgba(0.0, 0.0, 0.0, 1.0),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(0.0),
            blur_radius: Val::Px(0.0),
            spread_radius: Val::Px(0.0),
            inset: false,
        }
    }
}

impl ByoShadow {
    /// Transparent zero-offset shadow, used for CSS list padding during transitions.
    pub const TRANSPARENT_ZERO: Self = Self {
        color: Color::NONE,
        x_offset: Val::Px(0.0),
        y_offset: Val::Px(0.0),
        blur_radius: Val::Px(0.0),
        spread_radius: Val::Px(0.0),
        inset: false,
    };

    /// Convert to Bevy's `ShadowStyle` for rendering.
    ///
    /// Bevy's blur_radius is the Gaussian standard deviation (sigma),
    /// while CSS `blur-radius` is 2*sigma. We halve the value here to
    /// match CSS visual expectations.
    pub fn to_shadow_style(&self) -> bevy::ui::ShadowStyle {
        bevy::ui::ShadowStyle {
            color: self.color,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            blur_radius: halve_val(self.blur_radius),
            spread_radius: self.spread_radius,
        }
    }

    /// Create a `ByoShadow` from a Bevy `ShadowStyle`, doubling the blur
    /// radius to convert from Bevy's sigma back to CSS convention.
    pub fn from_shadow_style(s: &bevy::ui::ShadowStyle) -> Self {
        Self {
            color: s.color,
            x_offset: s.x_offset,
            y_offset: s.y_offset,
            blur_radius: double_val(s.blur_radius),
            spread_radius: s.spread_radius,
            inset: false,
        }
    }

    /// Interpolate between two shadows at parameter `t` in [0, 1].
    /// Mismatched `inset` flags snap to `b` (CSS spec: discrete interpolation).
    pub fn interpolate(&self, b: &ByoShadow, t: f32) -> ByoShadow {
        if self.inset != b.inset {
            // CSS spec: mismatched inset → discrete snap
            return if t < 0.5 { self.clone() } else { b.clone() };
        }
        ByoShadow {
            color: lerp_color(self.color, b.color, t),
            x_offset: lerp_val(self.x_offset, b.x_offset, t),
            y_offset: lerp_val(self.y_offset, b.y_offset, t),
            blur_radius: lerp_val(self.blur_radius, b.blur_radius, t),
            spread_radius: lerp_val(self.spread_radius, b.spread_radius, t),
            inset: self.inset,
        }
    }
}

/// Halve a Val (CSS blur-radius → Bevy sigma).
fn halve_val(v: Val) -> Val {
    match v {
        Val::Px(x) => Val::Px(x * 0.5),
        Val::Percent(x) => Val::Percent(x * 0.5),
        Val::Vw(x) => Val::Vw(x * 0.5),
        Val::Vh(x) => Val::Vh(x * 0.5),
        Val::VMin(x) => Val::VMin(x * 0.5),
        Val::VMax(x) => Val::VMax(x * 0.5),
        other => other,
    }
}

/// Double a Val (Bevy sigma → CSS blur-radius).
fn double_val(v: Val) -> Val {
    match v {
        Val::Px(x) => Val::Px(x * 2.0),
        Val::Percent(x) => Val::Percent(x * 2.0),
        Val::Vw(x) => Val::Vw(x * 2.0),
        Val::Vh(x) => Val::Vh(x * 2.0),
        Val::VMin(x) => Val::VMin(x * 2.0),
        Val::VMax(x) => Val::VMax(x * 2.0),
        other => other,
    }
}

/// Linearly interpolate between two `Val` values (same-variant only, else snap).
fn lerp_val(a: Val, b: Val, t: f32) -> Val {
    match (a, b) {
        (Val::Px(a), Val::Px(b)) => Val::Px(a + (b - a) * t),
        (Val::Percent(a), Val::Percent(b)) => Val::Percent(a + (b - a) * t),
        (Val::Vw(a), Val::Vw(b)) => Val::Vw(a + (b - a) * t),
        (Val::Vh(a), Val::Vh(b)) => Val::Vh(a + (b - a) * t),
        (Val::VMin(a), Val::VMin(b)) => Val::VMin(a + (b - a) * t),
        (Val::VMax(a), Val::VMax(b)) => Val::VMax(a + (b - a) * t),
        (_, b) => b,
    }
}

/// Linearly interpolate between two colors in sRGB space.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a = a.to_srgba();
    let b = b.to_srgba();
    Color::srgba(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    )
}

/// Approximate equality for shadow lists (used to detect changes).
pub fn shadow_list_approx_eq(a: &[ByoShadow], b: &[ByoShadow]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(sa, sb)| {
        sa.inset == sb.inset
            && val_approx_eq(sa.x_offset, sb.x_offset)
            && val_approx_eq(sa.y_offset, sb.y_offset)
            && val_approx_eq(sa.blur_radius, sb.blur_radius)
            && val_approx_eq(sa.spread_radius, sb.spread_radius)
            && color_approx_eq(sa.color, sb.color)
    })
}

fn val_approx_eq(a: Val, b: Val) -> bool {
    match (a, b) {
        (Val::Auto, Val::Auto) => true,
        (Val::Px(a), Val::Px(b)) => (a - b).abs() < 0.01,
        (Val::Percent(a), Val::Percent(b)) => (a - b).abs() < 0.01,
        (Val::Vw(a), Val::Vw(b)) => (a - b).abs() < 0.01,
        (Val::Vh(a), Val::Vh(b)) => (a - b).abs() < 0.01,
        (Val::VMin(a), Val::VMin(b)) => (a - b).abs() < 0.01,
        (Val::VMax(a), Val::VMax(b)) => (a - b).abs() < 0.01,
        _ => false,
    }
}

fn color_approx_eq(a: Color, b: Color) -> bool {
    let a = a.to_srgba();
    let b = b.to_srgba();
    (a.red - b.red).abs() < 0.005
        && (a.green - b.green).abs() < 0.005
        && (a.blue - b.blue).abs() < 0.005
        && (a.alpha - b.alpha).abs() < 0.005
}

// ---------------------------------------------------------------------------
// ByoFontWeight — wraps u16 font weight (100-1000)
// ---------------------------------------------------------------------------

/// Parses CSS font-weight keywords and numeric values (100-1000).
#[derive(Debug, Clone)]
pub struct ByoFontWeight(pub u16);

impl Default for ByoFontWeight {
    fn default() -> Self {
        Self(400)
    }
}

impl ReadProp for ByoFontWeight {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(w) = crate::font::parse_font_weight(value.as_ref()) {
                    self.0 = w;
                }
            }
            Prop::Remove { .. } => self.0 = 400,
            _ => {}
        }
    }
}

impl WriteProp for ByoFontWeight {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        out.push(Prop::val(key, self.0.to_string()));
    }
}

// ---------------------------------------------------------------------------
// ByoFontStyle — wraps FontStyleRequest
// ---------------------------------------------------------------------------

/// Parses CSS font-style: `normal`, `italic`, `oblique`, `oblique Xdeg`.
#[derive(Debug, Clone)]
pub struct ByoFontStyle(pub crate::font::FontStyleRequest);

impl Default for ByoFontStyle {
    fn default() -> Self {
        Self(crate::font::FontStyleRequest::Normal)
    }
}

impl ReadProp for ByoFontStyle {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(s) = crate::font::parse_font_style(value.as_ref()) {
                    self.0 = s;
                }
            }
            Prop::Remove { .. } => self.0 = crate::font::FontStyleRequest::Normal,
            _ => {}
        }
    }
}

impl WriteProp for ByoFontStyle {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        let val = match self.0 {
            crate::font::FontStyleRequest::Normal => "normal".to_string(),
            crate::font::FontStyleRequest::Italic => "italic".to_string(),
            crate::font::FontStyleRequest::Oblique(deg) => {
                if (deg - 14.0).abs() < 0.01 {
                    "oblique".to_string()
                } else {
                    format!("oblique {deg}deg")
                }
            }
        };
        out.push(Prop::val(key, val));
    }
}

// ---------------------------------------------------------------------------
// ByoFontStretch — wraps f32 percentage (50-200)
// ---------------------------------------------------------------------------

/// Parses CSS font-stretch keywords and percentage values.
#[derive(Debug, Clone)]
pub struct ByoFontStretch(pub f32);

impl Default for ByoFontStretch {
    fn default() -> Self {
        Self(100.0)
    }
}

impl ReadProp for ByoFontStretch {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => {
                if let Some(s) = crate::font::parse_font_stretch(value.as_ref()) {
                    self.0 = s;
                }
            }
            Prop::Remove { .. } => self.0 = 100.0,
            _ => {}
        }
    }
}

impl WriteProp for ByoFontStretch {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        out.push(Prop::val(key, format!("{}%", self.0)));
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

    // ── ByoShadow blur radius conversion (CSS 2*sigma → Bevy sigma) ────

    #[test]
    fn to_shadow_style_halves_blur_radius() {
        let shadow = ByoShadow {
            blur_radius: Val::Px(20.0),
            ..ByoShadow::default()
        };
        let style = shadow.to_shadow_style();
        // CSS blur-radius 20px → Bevy sigma 10px
        assert_eq!(style.blur_radius, Val::Px(10.0));
    }

    #[test]
    fn to_shadow_style_preserves_other_fields() {
        let shadow = ByoShadow {
            x_offset: Val::Px(4.0),
            y_offset: Val::Px(8.0),
            blur_radius: Val::Px(12.0),
            spread_radius: Val::Px(-2.0),
            color: Color::srgba(1.0, 0.0, 0.0, 0.5),
            inset: false,
        };
        let style = shadow.to_shadow_style();
        assert_eq!(style.x_offset, Val::Px(4.0));
        assert_eq!(style.y_offset, Val::Px(8.0));
        assert_eq!(style.blur_radius, Val::Px(6.0)); // halved
        assert_eq!(style.spread_radius, Val::Px(-2.0));
        let c = style.color.to_srgba();
        assert!((c.red - 1.0).abs() < 0.01);
        assert!((c.alpha - 0.5).abs() < 0.01);
    }

    #[test]
    fn from_shadow_style_doubles_blur_radius() {
        let style = bevy::ui::ShadowStyle {
            color: Color::srgba(0.0, 0.0, 0.0, 0.5),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(4.0),
            blur_radius: Val::Px(10.0), // Bevy sigma
            spread_radius: Val::Px(0.0),
        };
        let shadow = ByoShadow::from_shadow_style(&style);
        // Bevy sigma 10px → CSS blur-radius 20px
        assert_eq!(shadow.blur_radius, Val::Px(20.0));
        assert_eq!(shadow.y_offset, Val::Px(4.0));
    }

    #[test]
    fn shadow_roundtrip_preserves_blur() {
        let original = ByoShadow {
            blur_radius: Val::Px(16.0),
            ..ByoShadow::default()
        };
        let style = original.to_shadow_style();
        let roundtrip = ByoShadow::from_shadow_style(&style);
        assert_eq!(roundtrip.blur_radius, Val::Px(16.0));
    }

    #[test]
    fn halve_val_percent() {
        assert_eq!(halve_val(Val::Percent(50.0)), Val::Percent(25.0));
    }

    #[test]
    fn double_val_auto_unchanged() {
        assert_eq!(double_val(Val::Auto), Val::Auto);
    }

    // ── ByoAngle / parse_angle ──────────────────────────────────────

    #[test]
    fn parse_angle_bare_degrees() {
        let r = parse_angle("45").unwrap();
        assert!((r - 45.0_f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn parse_angle_deg_suffix() {
        let r = parse_angle("90deg").unwrap();
        assert!((r - 90.0_f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn parse_angle_rad_suffix() {
        let r = parse_angle("1.5708rad").unwrap();
        assert!((r - 1.5708).abs() < 1e-4);
    }

    #[test]
    fn parse_angle_turn_suffix() {
        let r = parse_angle("0.25turn").unwrap();
        assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn parse_angle_tau_suffix() {
        let r = parse_angle("0.5tau").unwrap();
        assert!((r - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn parse_angle_pi_suffix() {
        let r = parse_angle("1pi").unwrap();
        assert!((r - std::f32::consts::PI).abs() < 1e-5);
    }

    #[test]
    fn parse_angle_grad_suffix() {
        // 100grad = 90deg = π/2
        let r = parse_angle("100grad").unwrap();
        assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn parse_angle_negative() {
        let r = parse_angle("-45").unwrap();
        assert!((r - (-45.0_f32).to_radians()).abs() < 1e-6);
    }
}
