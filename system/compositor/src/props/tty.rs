//! TtyProps — persistent Bevy component for BYO `tty` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

/// Persistent component storing all BYO props for a `tty` entity.
///
/// Wire props override class-derived values. Defaults are applied at
/// resolution time, not here (so class values can take effect).
#[derive(Component, Debug, Clone, Default, FromProps, ToProps)]
pub struct TtyProps {
    pub font_size: Option<f32>,
    pub scrollback: Option<u32>,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub class: Option<String>,
}

/// Defaults applied at resolution time.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
pub const DEFAULT_SCROLLBACK: u32 = 1000;
