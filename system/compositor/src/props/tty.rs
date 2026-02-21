//! TtyProps — persistent Bevy component for BYO `tty` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

/// Persistent component storing all BYO props for a `tty` entity.
///
/// Wire props override class-derived values. Defaults are applied at
/// resolution time, not here (so class values can take effect).
///
/// Shadow and transition TW-derived fields (tw_box_shadow, tw_shadow_color,
/// tw_transition_*) are not stored here — they flow through a temporary
/// `ViewProps` during reconciliation/transition handling via `apply_classes`.
#[derive(Component, Debug, Clone, Default, FromProps, ToProps)]
pub struct TtyProps {
    pub font_size: Option<f32>,
    pub scrollback: Option<u32>,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub class: Option<String>,
    pub box_shadow: Option<String>,
    pub transition: Option<String>,
    /// CSS font-family list (e.g. `"Fira Code", monospace`).
    pub font_family: Option<String>,
}

/// Defaults applied at resolution time.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
pub const DEFAULT_SCROLLBACK: u32 = 1000;
