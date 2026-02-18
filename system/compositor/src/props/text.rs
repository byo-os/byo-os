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
}
