//! ViewProps — persistent Bevy component for BYO `view` type.

use bevy::prelude::*;
use byo::{FromProps, ToProps};

use super::types::*;
use crate::transition::config::{EaseFn, TransitionProperty};

/// Persistent component storing all BYO props for a `view` entity.
/// Reconciliation systems watch `Changed<ViewProps>` to update Bevy UI.
#[derive(Component, Debug, Default, Clone, FromProps, ToProps)]
pub struct ViewProps {
    pub class: Option<String>,
    pub width: Option<ByoVal>,
    pub height: Option<ByoVal>,
    pub min_width: Option<ByoVal>,
    pub max_width: Option<ByoVal>,
    pub min_height: Option<ByoVal>,
    pub max_height: Option<ByoVal>,
    pub background_color: Option<ByoColor>,
    pub border_color: Option<ByoColor>,
    pub border_width: Option<ByoRect>,
    pub border_radius: Option<ByoBorderRadius>,
    pub padding: Option<ByoRect>,
    pub margin: Option<ByoRect>,
    pub gap: Option<ByoVal>,
    pub column_gap: Option<ByoVal>,
    pub row_gap: Option<ByoVal>,
    pub display: Option<ByoDisplay>,
    pub flex_direction: Option<ByoFlexDirection>,
    pub align_items: Option<ByoAlignItems>,
    pub align_self: Option<ByoAlignSelf>,
    pub justify_content: Option<ByoJustifyContent>,
    pub flex_wrap: Option<ByoFlexWrap>,
    pub overflow: Option<ByoOverflow>,
    pub position: Option<ByoPositionType>,
    pub left: Option<ByoVal>,
    pub right: Option<ByoVal>,
    pub top: Option<ByoVal>,
    pub bottom: Option<ByoVal>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<ByoVal>,
    pub order: Option<i32>,
    pub hidden: bool,
    pub opacity: Option<f32>,
    // 2D transforms
    pub translate_x: Option<f32>,
    pub translate_y: Option<f32>,
    pub rotate: Option<f32>,
    pub scale: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    // Input events
    pub events: Option<String>,
    pub pointer_events: Option<ByoPointerEvents>,
    // Transition
    pub transition: Option<String>,
    #[prop(skip)]
    pub tw_transition_property: Option<TransitionProperty>,
    #[prop(skip)]
    pub tw_transition_duration: Option<f32>,
    #[prop(skip)]
    pub tw_transition_easing: Option<EaseFn>,
    #[prop(skip)]
    pub tw_transition_delay: Option<f32>,
}
