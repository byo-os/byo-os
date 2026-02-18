//! BYO marker components for compositor entities.

use bevy::prelude::*;

/// Marker for entities created from BYO `view` type.
#[derive(Component)]
pub struct ByoView;

/// Marker for entities created from BYO `text` type.
#[derive(Component)]
pub struct ByoText;

/// Marker for entities created from BYO `layer` type.
#[derive(Component)]
pub struct ByoLayer;

/// Marker for entities created from BYO `window` type.
#[derive(Component)]
pub struct ByoWindow;

/// Ordering component extracted from the `order` prop. Used for
/// child reordering within flex layouts.
#[derive(Component, Default)]
pub struct ByoOrder(pub i32);
