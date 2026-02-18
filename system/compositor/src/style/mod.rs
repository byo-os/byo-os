//! Style reconciliation — watches Changed<ViewProps>/Changed<TextProps>
//! and applies them to Bevy UI components.

pub mod color;
pub mod palette;
pub mod tailwind;

use bevy::prelude::*;

use crate::components::ByoOrder;
use crate::props::text::TextProps;
use crate::props::view::ViewProps;

/// Resolve class + individual props. Individual props always override class-derived values.
fn resolve_view_props(props: &ViewProps) -> ViewProps {
    let mut r = ViewProps::default();
    if let Some(ref class) = props.class {
        tailwind::apply_classes(&mut r, class);
    }
    // Individual props override class-derived values
    macro_rules! merge {
        ($($field:ident),* $(,)?) => {
            $(if props.$field.is_some() { r.$field = props.$field.clone(); })*
        };
    }
    merge!(
        width,
        height,
        min_width,
        max_width,
        min_height,
        max_height,
        background_color,
        border_color,
        border_width,
        border_radius,
        padding,
        margin,
        gap,
        column_gap,
        row_gap,
        display,
        flex_direction,
        align_items,
        align_self,
        justify_content,
        flex_wrap,
        overflow,
        position,
        left,
        right,
        top,
        bottom,
        flex_grow,
        flex_shrink,
        flex_basis,
        order,
        opacity,
    );
    if props.hidden {
        r.hidden = true;
    }
    r
}

/// Reconcile `ViewProps` changes onto Bevy `Node` + `BackgroundColor` etc.
#[allow(clippy::type_complexity)]
pub fn reconcile_views(
    mut query: Query<
        (
            &ViewProps,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            Option<&mut Visibility>,
        ),
        Changed<ViewProps>,
    >,
) {
    for (props, mut node, mut bg, mut border_color, visibility) in &mut query {
        let resolved = resolve_view_props(props);

        // Sizing
        if let Some(ref v) = resolved.width {
            node.width = v.0;
        }
        if let Some(ref v) = resolved.height {
            node.height = v.0;
        }
        if let Some(ref v) = resolved.min_width {
            node.min_width = v.0;
        }
        if let Some(ref v) = resolved.max_width {
            node.max_width = v.0;
        }
        if let Some(ref v) = resolved.min_height {
            node.min_height = v.0;
        }
        if let Some(ref v) = resolved.max_height {
            node.max_height = v.0;
        }

        // Colors
        if let Some(ref c) = resolved.background_color {
            *bg = BackgroundColor(c.0);
        }
        if let Some(ref c) = resolved.border_color {
            *border_color = BorderColor::all(c.0);
        }

        // Border
        if let Some(ref r) = resolved.border_width {
            node.border = r.0;
        }
        if let Some(ref r) = resolved.border_radius {
            node.border_radius = r.0;
        }

        // Spacing
        if let Some(ref r) = resolved.padding {
            node.padding = r.0;
        }
        if let Some(ref r) = resolved.margin {
            node.margin = r.0;
        }

        // Gap
        if let Some(ref v) = resolved.gap {
            node.column_gap = v.0;
            node.row_gap = v.0;
        }
        if let Some(ref v) = resolved.column_gap {
            node.column_gap = v.0;
        }
        if let Some(ref v) = resolved.row_gap {
            node.row_gap = v.0;
        }

        // Layout
        if let Some(ref d) = resolved.display {
            node.display = d.to_bevy();
        }
        if let Some(ref d) = resolved.flex_direction {
            node.flex_direction = d.to_bevy();
        }
        if let Some(ref a) = resolved.align_items {
            node.align_items = a.to_bevy();
        }
        if let Some(ref a) = resolved.align_self {
            node.align_self = a.to_bevy();
        }
        if let Some(ref j) = resolved.justify_content {
            node.justify_content = j.to_bevy();
        }
        if let Some(ref w) = resolved.flex_wrap {
            node.flex_wrap = w.to_bevy();
        }
        if let Some(ref p) = resolved.position {
            node.position_type = p.to_bevy();
        }

        // Overflow
        if let Some(ref o) = resolved.overflow {
            let axis = o.to_bevy();
            node.overflow = Overflow { x: axis, y: axis };
        }

        // Position
        if let Some(ref v) = resolved.left {
            node.left = v.0;
        }
        if let Some(ref v) = resolved.right {
            node.right = v.0;
        }
        if let Some(ref v) = resolved.top {
            node.top = v.0;
        }
        if let Some(ref v) = resolved.bottom {
            node.bottom = v.0;
        }

        // Flex
        if let Some(v) = resolved.flex_grow {
            node.flex_grow = v;
        }
        if let Some(v) = resolved.flex_shrink {
            node.flex_shrink = v;
        }
        if let Some(ref v) = resolved.flex_basis {
            node.flex_basis = v.0;
        }

        // Visibility
        if let Some(mut vis) = visibility {
            *vis = if resolved.hidden {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

/// Resolve class + individual props for text. Individual props always override class-derived values.
fn resolve_text_props(props: &TextProps) -> TextProps {
    let mut r = TextProps::default();
    if let Some(ref class) = props.class {
        tailwind::apply_text_classes(&mut r, class);
    }
    // Individual props override class-derived values
    if props.content.is_some() {
        r.content = props.content.clone();
    }
    if props.color.is_some() {
        r.color = props.color.clone();
    }
    if props.font_size.is_some() {
        r.font_size = props.font_size;
    }
    if props.text_align.is_some() {
        r.text_align = props.text_align.clone();
    }
    if props.line_height.is_some() {
        r.line_height = props.line_height;
    }
    r
}

/// Reconcile `TextProps` changes onto Bevy `Text` + `TextFont` + `TextColor`.
pub fn reconcile_text(
    mut query: Query<(&TextProps, &mut Text, &mut TextFont, &mut TextColor), Changed<TextProps>>,
) {
    for (props, mut text, mut font, mut color) in &mut query {
        let resolved = resolve_text_props(props);

        if let Some(ref content) = resolved.content {
            **text = content.clone();
        }
        if let Some(size) = resolved.font_size {
            font.font_size = size;
        }
        if let Some(ref c) = resolved.color {
            *color = TextColor(c.0);
        }
    }
}

/// Reorder children of a parent when any child's `ByoOrder` changes.
pub fn reorder_children(
    changed: Query<Entity, Changed<ByoOrder>>,
    children_query: Query<&Children>,
    parent_query: Query<&ChildOf>,
    orders: Query<&ByoOrder>,
    mut commands: Commands,
) {
    // Collect unique parents that need reordering
    let mut parents_to_reorder = Vec::new();
    for entity in &changed {
        if let Ok(child_of) = parent_query.get(entity) {
            let parent = child_of.parent();
            if !parents_to_reorder.contains(&parent) {
                parents_to_reorder.push(parent);
            }
        }
    }

    for parent in parents_to_reorder {
        let Ok(children) = children_query.get(parent) else {
            continue;
        };

        let mut child_order: Vec<(Entity, i32)> = children
            .iter()
            .map(|e| {
                let order = orders.get(e).map(|o| o.0).unwrap_or(0);
                (e, order)
            })
            .collect();

        // Stable sort by order
        child_order.sort_by_key(|&(_, order)| order);

        let sorted: Vec<Entity> = child_order.iter().map(|(e, _)| *e).collect();
        commands.entity(parent).replace_children(&sorted);
    }
}
