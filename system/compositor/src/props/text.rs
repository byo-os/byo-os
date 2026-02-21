//! TextProps — persistent Bevy component for BYO `text` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;

/// Persistent component storing all BYO props for a `text` entity.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct TextProps {
    pub content: Option<String>,
    pub class: Option<String>,
    pub color: Option<ByoColor>,
    pub font_size: Option<f32>,
    pub text_align: Option<ByoTextAlign>,
    pub line_height: Option<f32>,
    /// CSS font-family list (e.g. `"Fira Code", monospace`).
    pub font_family: Option<String>,
    /// CSS font-weight (keyword or 100-1000).
    pub font_weight: Option<ByoFontWeight>,
    /// CSS font-style: `normal`, `italic`, `oblique`, `oblique Xdeg`.
    pub font_style: Option<ByoFontStyle>,
    /// CSS font-stretch keyword or percentage.
    pub font_stretch: Option<ByoFontStretch>,
    /// CSS-like event subscription string (e.g. `"click, pointerdown verbose"`).
    pub events: Option<String>,
    /// CSS pointer-events behavior (`auto` or `none`).
    pub pointer_events: Option<ByoPointerEvents>,
}
