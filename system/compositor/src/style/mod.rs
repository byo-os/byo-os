//! Style reconciliation — watches Changed<ViewProps>/Changed<TextProps>
//! and applies them to Bevy UI components.

pub mod color;
pub mod palette;
pub mod shadow;
pub mod tailwind;

use bevy::prelude::*;

use crate::components::ByoOrder;
use crate::components::ByoTty;
use crate::events::config::EventSubscriptions;
use crate::kitty_gfx::store::KittyGfxImageStore;
use crate::plugin::WorldScale;
use crate::props::layer::LayerProps;
use crate::props::text::TextProps;
use crate::props::tty::TtyProps;
use crate::props::types::{ByoColor, ByoOrderMode, ByoPointerEvents, ByoShadow};
use crate::props::view::ViewProps;
use crate::props::window::WindowProps;
use crate::render::layer::{LayerRender, resize_layer_render};
use crate::transition::config::AnimatableProp;
use crate::transition::state::ActiveTransitions;

/// Default scale factor for order → z/depth-bias mapping.
const DEFAULT_ORDER_SCALE: f32 = 0.001;

/// Resolve class + individual props. Individual props always override class-derived values.
pub(crate) fn resolve_view_props(props: &ViewProps) -> ViewProps {
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
        translate_x,
        translate_y,
        rotate,
        scale,
        scale_x,
        scale_y,
        transition,
        tw_transition_property,
        tw_transition_duration,
        tw_transition_easing,
        tw_transition_delay,
        background_image,
        events,
        pointer_events,
        box_shadow,
        tw_box_shadow,
        tw_shadow_color,
    );
    if props.hidden {
        r.hidden = true;
    }
    r
}

/// Resolve the final box-shadow list from wire prop, TW preset, and TW shadow color.
///
/// Priority: wire `box-shadow` prop > TW preset. Shadow color override is applied
/// to TW-derived shadows only.
pub(crate) fn resolve_box_shadow(resolved: &ViewProps) -> Vec<ByoShadow> {
    // Wire prop takes priority
    if let Some(ref wire) = resolved.box_shadow {
        return shadow::parse_box_shadow(wire);
    }
    // TW-derived preset
    if let Some(ref shadows) = resolved.tw_box_shadow {
        let mut result = shadows.clone();
        // Apply shadow color override — replaces color entirely (Tailwind convention:
        // the preset defines geometry, the color utility defines the full color).
        if let Some(color) = resolved.tw_shadow_color {
            for s in &mut result {
                s.color = color;
            }
        }
        return result;
    }
    Vec::new()
}

/// Reconcile `ViewProps` changes onto Bevy `Node` + `BackgroundColor` etc.
/// Skips fields that have active transitions (those are driven by tick_view_transitions).
#[allow(clippy::type_complexity)]
pub fn reconcile_views(
    mut query: Query<
        (
            &ViewProps,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
            Option<&mut Visibility>,
            Option<&ActiveTransitions>,
        ),
        Changed<ViewProps>,
    >,
) {
    for (props, mut node, mut bg, mut border_color, mut box_shadow, visibility, active) in
        &mut query
    {
        let resolved = resolve_view_props(props);
        let has = |p: AnimatableProp| active.is_some_and(|a| a.has(p));
        let defaults = Node::default();

        // Sizing
        if !has(AnimatableProp::Width) {
            node.width = resolved.width.as_ref().map_or(defaults.width, |v| v.0);
        }
        if !has(AnimatableProp::Height) {
            node.height = resolved.height.as_ref().map_or(defaults.height, |v| v.0);
        }
        if !has(AnimatableProp::MinWidth) {
            node.min_width = resolved
                .min_width
                .as_ref()
                .map_or(defaults.min_width, |v| v.0);
        }
        if !has(AnimatableProp::MaxWidth) {
            node.max_width = resolved
                .max_width
                .as_ref()
                .map_or(defaults.max_width, |v| v.0);
        }
        if !has(AnimatableProp::MinHeight) {
            node.min_height = resolved
                .min_height
                .as_ref()
                .map_or(defaults.min_height, |v| v.0);
        }
        if !has(AnimatableProp::MaxHeight) {
            node.max_height = resolved
                .max_height
                .as_ref()
                .map_or(defaults.max_height, |v| v.0);
        }

        // Colors
        if !has(AnimatableProp::BackgroundColor) {
            *bg = resolved
                .background_color
                .as_ref()
                .map_or(BackgroundColor::default(), |c| BackgroundColor(c.0));
        }
        if !has(AnimatableProp::BorderColor) {
            *border_color = resolved
                .border_color
                .as_ref()
                .map_or(BorderColor::default(), |c| BorderColor::all(c.0));
        }

        // Border
        {
            let border = resolved.border_width.as_ref().map(|r| &r.0);
            if !has(AnimatableProp::BorderWidthTop) {
                node.border.top = border.map_or(defaults.border.top, |r| r.top);
            }
            if !has(AnimatableProp::BorderWidthRight) {
                node.border.right = border.map_or(defaults.border.right, |r| r.right);
            }
            if !has(AnimatableProp::BorderWidthBottom) {
                node.border.bottom = border.map_or(defaults.border.bottom, |r| r.bottom);
            }
            if !has(AnimatableProp::BorderWidthLeft) {
                node.border.left = border.map_or(defaults.border.left, |r| r.left);
            }
        }
        {
            let radius = resolved.border_radius.as_ref().map(|r| &r.0);
            if !has(AnimatableProp::BorderRadiusTopLeft) {
                node.border_radius.top_left =
                    radius.map_or(defaults.border_radius.top_left, |r| r.top_left);
            }
            if !has(AnimatableProp::BorderRadiusTopRight) {
                node.border_radius.top_right =
                    radius.map_or(defaults.border_radius.top_right, |r| r.top_right);
            }
            if !has(AnimatableProp::BorderRadiusBottomRight) {
                node.border_radius.bottom_right =
                    radius.map_or(defaults.border_radius.bottom_right, |r| r.bottom_right);
            }
            if !has(AnimatableProp::BorderRadiusBottomLeft) {
                node.border_radius.bottom_left =
                    radius.map_or(defaults.border_radius.bottom_left, |r| r.bottom_left);
            }
        }

        // Spacing
        {
            let padding = resolved.padding.as_ref().map(|r| &r.0);
            if !has(AnimatableProp::PaddingTop) {
                node.padding.top = padding.map_or(defaults.padding.top, |r| r.top);
            }
            if !has(AnimatableProp::PaddingRight) {
                node.padding.right = padding.map_or(defaults.padding.right, |r| r.right);
            }
            if !has(AnimatableProp::PaddingBottom) {
                node.padding.bottom = padding.map_or(defaults.padding.bottom, |r| r.bottom);
            }
            if !has(AnimatableProp::PaddingLeft) {
                node.padding.left = padding.map_or(defaults.padding.left, |r| r.left);
            }
        }
        {
            let margin = resolved.margin.as_ref().map(|r| &r.0);
            if !has(AnimatableProp::MarginTop) {
                node.margin.top = margin.map_or(defaults.margin.top, |r| r.top);
            }
            if !has(AnimatableProp::MarginRight) {
                node.margin.right = margin.map_or(defaults.margin.right, |r| r.right);
            }
            if !has(AnimatableProp::MarginBottom) {
                node.margin.bottom = margin.map_or(defaults.margin.bottom, |r| r.bottom);
            }
            if !has(AnimatableProp::MarginLeft) {
                node.margin.left = margin.map_or(defaults.margin.left, |r| r.left);
            }
        }

        // Gap
        if !has(AnimatableProp::ColumnGap) {
            node.column_gap = resolved
                .column_gap
                .as_ref()
                .or(resolved.gap.as_ref())
                .map_or(defaults.column_gap, |v| v.0);
        }
        if !has(AnimatableProp::RowGap) {
            node.row_gap = resolved
                .row_gap
                .as_ref()
                .or(resolved.gap.as_ref())
                .map_or(defaults.row_gap, |v| v.0);
        }

        // Layout (not animatable — always write immediately)
        node.display = resolved
            .display
            .as_ref()
            .map_or(defaults.display, |d| d.to_bevy());
        node.flex_direction = resolved
            .flex_direction
            .as_ref()
            .map_or(defaults.flex_direction, |d| d.to_bevy());
        node.align_items = resolved
            .align_items
            .as_ref()
            .map_or(defaults.align_items, |a| a.to_bevy());
        node.align_self = resolved
            .align_self
            .as_ref()
            .map_or(defaults.align_self, |a| a.to_bevy());
        node.justify_content = resolved
            .justify_content
            .as_ref()
            .map_or(defaults.justify_content, |j| j.to_bevy());
        node.flex_wrap = resolved
            .flex_wrap
            .as_ref()
            .map_or(defaults.flex_wrap, |w| w.to_bevy());
        node.position_type = resolved
            .position
            .as_ref()
            .map_or(defaults.position_type, |p| p.to_bevy());

        // Overflow
        node.overflow = resolved.overflow.as_ref().map_or(defaults.overflow, |o| {
            let axis = o.to_bevy();
            Overflow { x: axis, y: axis }
        });

        // Position
        if !has(AnimatableProp::Left) {
            node.left = resolved.left.as_ref().map_or(defaults.left, |v| v.0);
        }
        if !has(AnimatableProp::Right) {
            node.right = resolved.right.as_ref().map_or(defaults.right, |v| v.0);
        }
        if !has(AnimatableProp::Top) {
            node.top = resolved.top.as_ref().map_or(defaults.top, |v| v.0);
        }
        if !has(AnimatableProp::Bottom) {
            node.bottom = resolved.bottom.as_ref().map_or(defaults.bottom, |v| v.0);
        }

        // Flex
        if !has(AnimatableProp::FlexGrow) {
            node.flex_grow = resolved.flex_grow.unwrap_or(defaults.flex_grow);
        }
        if !has(AnimatableProp::FlexShrink) {
            node.flex_shrink = resolved.flex_shrink.unwrap_or(defaults.flex_shrink);
        }
        if !has(AnimatableProp::FlexBasis) {
            node.flex_basis = resolved
                .flex_basis
                .as_ref()
                .map_or(defaults.flex_basis, |v| v.0);
        }

        // Box Shadow
        if !has(AnimatableProp::BoxShadow) {
            let shadows = resolve_box_shadow(&resolved);
            box_shadow.0 = shadows
                .iter()
                .filter(|s| !s.inset) // Bevy doesn't support inset shadows
                .map(|s| s.to_shadow_style())
                .collect();
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

/// Reconcile `background-image` prop → `ImageNode` component.
///
/// Runs over all views with a `background_image` prop (not just Changed),
/// because images may be uploaded after the view is created. Only does
/// actual work when the state needs updating.
pub fn reconcile_view_images(
    mut commands: Commands,
    query: Query<(Entity, &ViewProps, Option<&ImageNode>)>,
    store: Res<KittyGfxImageStore>,
) {
    for (entity, props, existing_image) in &query {
        let resolved = resolve_view_props(props);
        match resolved.background_image {
            Some(ref value) => {
                // Scan for $img(N)
                let refs = byo::vars::scan(value);
                let img_ref = refs.iter().find(|r| r.name == "img");
                if let Some(var) = img_ref
                    && let Ok(id) = var.args.parse::<u32>()
                    && let Some(kitty_img) = store.get(id)
                {
                    // Check if ImageNode already has the correct handle
                    let needs_update =
                        existing_image.is_none_or(|img| img.image != kitty_img.handle);
                    if needs_update {
                        commands.entity(entity).insert(ImageNode {
                            image: kitty_img.handle.clone(),
                            ..default()
                        });
                    }
                }
                // If image not found yet, do nothing — will reconcile when uploaded
            }
            None => {
                // Remove ImageNode if present
                if existing_image.is_some() {
                    commands.entity(entity).remove::<ImageNode>();
                }
            }
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
    if props.font_family.is_some() {
        r.font_family = props.font_family.clone();
    }
    if props.font_weight.is_some() {
        r.font_weight = props.font_weight.clone();
    }
    if props.font_style.is_some() {
        r.font_style = props.font_style.clone();
    }
    if props.font_stretch.is_some() {
        r.font_stretch = props.font_stretch.clone();
    }
    r
}

/// Reconcile `TextProps` changes onto Bevy `Text` + `TextFont` + `TextColor`.
pub fn reconcile_text(
    mut query: Query<(&TextProps, &mut Text, &mut TextFont, &mut TextColor), Changed<TextProps>>,
    mut font_store: ResMut<crate::font::FontStore>,
    asset_server: Res<AssetServer>,
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

        // Resolve font family + weight + style + stretch
        let weight = resolved.font_weight.as_ref().map_or(400, |w| w.0);
        let style = resolved
            .font_style
            .as_ref()
            .map_or(crate::font::FontStyleRequest::Normal, |s| s.0);
        let stretch = resolved.font_stretch.as_ref().map_or(100.0, |s| s.0);

        // Resolve the font-family specifier. Tailwind class names (e.g. "font-mono")
        // are resolved through the FontStore's tailwind map first.
        let family_spec = resolved.font_family.as_deref().and_then(|spec| {
            font_store
                .resolve_tailwind_class(spec)
                .or_else(|| font_store.resolve_font_family_list(spec))
        });

        let default = font_store.default_text_family.clone();
        if let Some(handle) = font_store.resolve_font(
            family_spec.as_deref(),
            weight,
            style,
            stretch,
            &default,
            &asset_server,
        ) {
            font.font = handle;
        }
        font.weight = bevy::text::FontWeight(weight);
    }
}

/// Resolved tty-specific properties (class merged with wire props, defaults applied).
pub struct ResolvedTtyProps {
    pub font_size: f32,
    pub scrollback: u32,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub font_family: Option<String>,
}

/// Resolve tty-specific props: class values are the base, wire props override, defaults fill gaps.
pub fn resolve_tty_props(props: &TtyProps) -> ResolvedTtyProps {
    use crate::props::tty::{DEFAULT_FONT_SIZE, DEFAULT_SCROLLBACK};

    let class_props = props
        .class
        .as_ref()
        .map(|c| tailwind::apply_tty_classes(c))
        .unwrap_or_default();

    ResolvedTtyProps {
        font_size: props
            .font_size
            .or(class_props.font_size)
            .unwrap_or(DEFAULT_FONT_SIZE),
        scrollback: props
            .scrollback
            .or(class_props.scrollback)
            .unwrap_or(DEFAULT_SCROLLBACK),
        cols: props.cols.or(class_props.cols),
        rows: props.rows.or(class_props.rows),
        font_family: props.font_family.clone().or(class_props.font_family),
    }
}

/// Reconcile `TtyProps` class onto the tty's `Node`, `BackgroundColor`, `BorderColor`,
/// and `BoxShadow`.
///
/// Parses the tailwind class string from `TtyProps.class` using the same parser as views.
/// Only fields specified by the class are overridden; tty defaults (flex column, overflow clip,
/// 100% size) set at spawn time are preserved for unspecified fields.
#[allow(clippy::type_complexity)]
pub fn reconcile_tty_style(
    mut query: Query<
        (
            &TtyProps,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
            Option<&ActiveTransitions>,
        ),
        (Changed<TtyProps>, With<ByoTty>),
    >,
) {
    for (props, mut node, mut bg, mut border_color, mut box_shadow, active) in &mut query {
        let has = |p: AnimatableProp| active.is_some_and(|a| a.has(p));
        let defaults = Node::default();

        // Parse TW classes into a temporary ViewProps
        let mut resolved = ViewProps::default();
        if let Some(ref class) = props.class {
            tailwind::apply_classes(&mut resolved, class);
        }
        // Merge wire-level shadow/transition props from TtyProps
        if props.box_shadow.is_some() {
            resolved.box_shadow.clone_from(&props.box_shadow);
        }
        if props.transition.is_some() {
            resolved.transition.clone_from(&props.transition);
        }
        // TW-derived shadow fields are already on `resolved` from apply_classes

        // Sizing
        node.width = resolved.width.as_ref().map_or(defaults.width, |v| v.0);
        node.height = resolved.height.as_ref().map_or(defaults.height, |v| v.0);
        node.min_width = resolved
            .min_width
            .as_ref()
            .map_or(defaults.min_width, |v| v.0);
        node.max_width = resolved
            .max_width
            .as_ref()
            .map_or(defaults.max_width, |v| v.0);
        node.min_height = resolved
            .min_height
            .as_ref()
            .map_or(defaults.min_height, |v| v.0);
        node.max_height = resolved
            .max_height
            .as_ref()
            .map_or(defaults.max_height, |v| v.0);

        // Colors
        if !has(AnimatableProp::BackgroundColor) {
            *bg = resolved
                .background_color
                .as_ref()
                .map_or(BackgroundColor::default(), |c| BackgroundColor(c.0));
        }
        if !has(AnimatableProp::BorderColor) {
            *border_color = resolved
                .border_color
                .as_ref()
                .map_or(BorderColor::default(), |c| BorderColor::all(c.0));
        }

        // Border
        node.border = resolved
            .border_width
            .as_ref()
            .map_or(defaults.border, |r| r.0);
        node.border_radius = resolved
            .border_radius
            .as_ref()
            .map_or(defaults.border_radius, |r| r.0);

        // Spacing
        node.padding = resolved.padding.as_ref().map_or(defaults.padding, |r| r.0);
        node.margin = resolved.margin.as_ref().map_or(defaults.margin, |r| r.0);

        // Gap
        node.column_gap = resolved
            .column_gap
            .as_ref()
            .or(resolved.gap.as_ref())
            .map_or(defaults.column_gap, |v| v.0);
        node.row_gap = resolved
            .row_gap
            .as_ref()
            .or(resolved.gap.as_ref())
            .map_or(defaults.row_gap, |v| v.0);

        // Layout — TTY defaults differ from Node::default(): rows must stack
        // vertically (Column), left-align (Start), and clip overflow.
        node.display = resolved
            .display
            .as_ref()
            .map_or(defaults.display, |d| d.to_bevy());
        node.flex_direction = resolved
            .flex_direction
            .as_ref()
            .map_or(FlexDirection::Column, |d| d.to_bevy());
        node.align_items = resolved
            .align_items
            .as_ref()
            .map_or(AlignItems::Start, |a| a.to_bevy());
        node.align_self = resolved
            .align_self
            .as_ref()
            .map_or(defaults.align_self, |a| a.to_bevy());
        node.justify_content = resolved
            .justify_content
            .as_ref()
            .map_or(defaults.justify_content, |j| j.to_bevy());
        node.flex_wrap = resolved
            .flex_wrap
            .as_ref()
            .map_or(defaults.flex_wrap, |w| w.to_bevy());
        node.position_type = resolved
            .position
            .as_ref()
            .map_or(defaults.position_type, |p| p.to_bevy());

        // Overflow — TTY default is clip (terminal content shouldn't overflow).
        node.overflow = resolved.overflow.as_ref().map_or(Overflow::clip(), |o| {
            let axis = o.to_bevy();
            Overflow { x: axis, y: axis }
        });

        // Position
        node.left = resolved.left.as_ref().map_or(defaults.left, |v| v.0);
        node.right = resolved.right.as_ref().map_or(defaults.right, |v| v.0);
        node.top = resolved.top.as_ref().map_or(defaults.top, |v| v.0);
        node.bottom = resolved.bottom.as_ref().map_or(defaults.bottom, |v| v.0);

        // Flex
        node.flex_grow = resolved.flex_grow.unwrap_or(defaults.flex_grow);
        node.flex_shrink = resolved.flex_shrink.unwrap_or(defaults.flex_shrink);
        node.flex_basis = resolved
            .flex_basis
            .as_ref()
            .map_or(defaults.flex_basis, |v| v.0);

        // Box Shadow (resolved from TW classes + wire prop, same as views)
        if !has(AnimatableProp::BoxShadow) {
            let shadows = resolve_box_shadow(&resolved);
            box_shadow.0 = shadows
                .iter()
                .filter(|s| !s.inset)
                .map(|s| s.to_shadow_style())
                .collect();
        }
    }
}

/// Reconcile `ViewProps` 2D transform changes onto `UiTransform`.
/// Skips fields with active transitions.
pub fn reconcile_view_transforms(
    mut query: Query<
        (&ViewProps, &mut UiTransform, Option<&ActiveTransitions>),
        Changed<ViewProps>,
    >,
) {
    for (props, mut ui_transform, active) in &mut query {
        // If any 2D transform prop is transitioning, skip the entire transform write
        // (tick_view_transitions handles per-field writes)
        let has = |p: AnimatableProp| active.is_some_and(|a| a.has(p));
        if has(AnimatableProp::TranslateX)
            || has(AnimatableProp::TranslateY)
            || has(AnimatableProp::Rotate)
            || has(AnimatableProp::ScaleX)
            || has(AnimatableProp::ScaleY)
        {
            continue;
        }

        let resolved = resolve_view_props(props);
        let tx = resolved.translate_x.unwrap_or(0.0);
        let ty = resolved.translate_y.unwrap_or(0.0);
        let uniform_scale = resolved.scale.unwrap_or(1.0);
        let sx = resolved.scale_x.unwrap_or(uniform_scale);
        let sy = resolved.scale_y.unwrap_or(uniform_scale);
        let rot_deg = resolved.rotate.unwrap_or(0.0);

        *ui_transform = UiTransform {
            translation: Val2::px(tx, ty),
            scale: Vec2::new(sx, sy),
            rotation: Rot2::radians(rot_deg.to_radians()),
        };
    }
}

/// Reconcile `WindowProps` 3D transform changes onto `Transform`.
/// Skips when 3D transform props are actively transitioning.
pub fn reconcile_windows(
    mut query: Query<
        (&WindowProps, &mut Transform, Option<&ActiveTransitions>),
        Changed<WindowProps>,
    >,
    world_scale: Res<WorldScale>,
) {
    for (props, mut transform, active) in &mut query {
        // Skip if any 3D transform is transitioning (tick handles it)
        if active.is_some_and(|a| a.has_any_3d_transform()) {
            continue;
        }

        let ts = resolve_transform_style(props.class.as_deref());
        let (order, mode, scale) =
            resolve_order(&ts, props.order, &props.order_mode, props.order_scale);
        let order_z = match mode {
            ByoOrderMode::TranslateZ => order * scale,
            _ => 0.0,
        };
        *transform = resolve_3d_transform(
            &ts,
            props.translate_x,
            props.translate_y,
            props.translate_z,
            props.rotate,
            props.rotate_x,
            props.rotate_y,
            props.rotate_z,
            props.scale,
            props.scale_x,
            props.scale_y,
            props.scale_z,
            order_z,
            world_scale.0,
        );
    }
}

/// Reconcile `LayerProps` 3D transform + PBR material + size changes.
/// Skips transform/PBR fields that are actively transitioning.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn reconcile_layers(
    mut layer_query: Query<
        (&LayerProps, &mut LayerRender, Option<&ActiveTransitions>),
        Changed<LayerProps>,
    >,
    mut transforms: Query<&mut Transform>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mesh_handles: Query<&Mesh3d>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    world_scale: Res<WorldScale>,
) {
    for (props, mut render, active) in &mut layer_query {
        // Parse class once for shared use across sizing, order, transforms, PBR
        let cs = resolve_transform_style(props.class.as_deref());

        // Resize render texture + plane mesh if width/height changed
        let new_width =
            props
                .width
                .as_ref()
                .or(cs.width.as_ref())
                .map_or(render.width, |v| match v.0 {
                    Val::Px(px) => px as u32,
                    _ => render.width,
                });
        let new_height = props
            .height
            .as_ref()
            .or(cs.height.as_ref())
            .map_or(render.height, |v| match v.0 {
                Val::Px(px) => px as u32,
                _ => render.height,
            });
        resize_layer_render(
            &mut render,
            new_width,
            new_height,
            world_scale.0,
            &mut images,
            &mut meshes,
            &mesh_handles,
        );

        let (order, mode, scale) =
            resolve_order(&cs, props.order, &props.order_mode, props.order_scale);

        // Apply transform to the 3D plane entity (skip if transitioning)
        if !active.is_some_and(|a| a.has_any_3d_transform()) {
            let order_z = match mode {
                ByoOrderMode::TranslateZ => order * scale,
                _ => 0.0,
            };
            let t = resolve_3d_transform(
                &cs,
                props.translate_x,
                props.translate_y,
                props.translate_z,
                props.rotate,
                props.rotate_x,
                props.rotate_y,
                props.rotate_z,
                props.scale,
                props.scale_x,
                props.scale_y,
                props.scale_z,
                order_z,
                world_scale.0,
            );

            if let Ok(mut plane_transform) = transforms.get_mut(render.plane) {
                *plane_transform = t;
            }
        }

        // Apply PBR material props + order-based depth-bias
        if let Ok(mat_handle) = material_handles.get(render.plane)
            && let Some(mat) = materials.get_mut(&mat_handle.0)
        {
            apply_pbr_props(mat, props, &cs, world_scale.0);

            // Order-based depth-bias (explicit depth_bias prop already applied by apply_pbr_props)
            if matches!(mode, ByoOrderMode::DepthBias) && props.depth_bias.is_none() {
                mat.depth_bias = order * scale;
            }
        }
    }
}

/// Parse class string into a TransformStyle once, for shared use across
/// transform, PBR, order, and sizing resolution.
fn resolve_transform_style(class: Option<&str>) -> tailwind::TransformStyle {
    let mut ts = tailwind::TransformStyle::default();
    if let Some(class_str) = class {
        tailwind::apply_transform_classes(&mut ts, class_str);
    }
    ts
}

/// Shared 3D transform builder — uses pre-parsed TransformStyle for class values.
/// Individual wire props always override class-derived values.
/// `order_z` is the z contribution from order (pre-computed by caller based on order-mode).
/// `world_scale` converts pixel translations to meters (= 1.0 / pixels_per_meter).
#[allow(clippy::too_many_arguments)]
fn resolve_3d_transform(
    ts: &tailwind::TransformStyle,
    prop_tx: Option<f32>,
    prop_ty: Option<f32>,
    prop_tz: Option<f32>,
    prop_rotate: Option<f32>,
    prop_rx: Option<f32>,
    prop_ry: Option<f32>,
    prop_rz: Option<f32>,
    prop_scale: Option<f32>,
    prop_sx: Option<f32>,
    prop_sy: Option<f32>,
    prop_sz: Option<f32>,
    order_z: f32,
    world_scale: f32,
) -> Transform {
    // Individual props override class-derived values.
    // Translations are in protocol pixels — convert to world meters.
    let tx = prop_tx.or(ts.translate_x).unwrap_or(0.0) * world_scale;
    let ty = prop_ty.or(ts.translate_y).unwrap_or(0.0) * world_scale;
    let tz = prop_tz.or(ts.translate_z).unwrap_or(0.0) * world_scale + order_z;
    let rot = prop_rotate.or(ts.rotate).unwrap_or(0.0);
    let rx = prop_rx.or(ts.rotate_x).unwrap_or(0.0);
    let ry = prop_ry.or(ts.rotate_y).unwrap_or(0.0);
    // rotate-z overrides rotate shorthand
    let rz = prop_rz.or(ts.rotate_z).unwrap_or(rot);
    let uniform = prop_scale.or(ts.scale).unwrap_or(1.0);
    let sx = prop_sx.or(ts.scale_x).unwrap_or(uniform);
    let sy = prop_sy.or(ts.scale_y).unwrap_or(uniform);
    let sz = prop_sz.or(ts.scale_z).unwrap_or(uniform);

    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        rx.to_radians(),
        ry.to_radians(),
        rz.to_radians(),
    );

    Transform {
        translation: Vec3::new(tx, ty, tz),
        rotation,
        scale: Vec3::new(sx, sy, sz),
    }
}

/// Resolve order parameters from class + wire props.
/// Returns (order_f32, order_mode, order_scale).
fn resolve_order<'a>(
    ts: &'a tailwind::TransformStyle,
    prop_order: Option<i32>,
    prop_order_mode: &'a Option<ByoOrderMode>,
    prop_order_scale: Option<f32>,
) -> (f32, &'a ByoOrderMode, f32) {
    static DEFAULT_MODE: ByoOrderMode = ByoOrderMode::TranslateZ;
    let order = prop_order.or(ts.order).unwrap_or(0) as f32;
    let mode = prop_order_mode
        .as_ref()
        .or(ts.order_mode.as_ref())
        .unwrap_or(&DEFAULT_MODE);
    let scale = prop_order_scale
        .or(ts.order_scale)
        .unwrap_or(DEFAULT_ORDER_SCALE);
    (order, mode, scale)
}

/// Apply PBR material props from LayerProps onto a StandardMaterial.
/// Uses pre-parsed TransformStyle for class-derived values. Individual wire props override.
/// `world_scale` converts pixel-based spatial values (thickness, attenuation_distance) to meters.
fn apply_pbr_props(
    mat: &mut StandardMaterial,
    props: &LayerProps,
    cs: &tailwind::TransformStyle,
    world_scale: f32,
) {
    // Helper: convert ByoColor to sRGB Color for base_color
    fn resolve_color(prop: &Option<ByoColor>, class: Option<Color>) -> Option<Color> {
        prop.as_ref().map(|c| c.0).or(class)
    }

    // Helper: convert ByoColor to LinearRgba for emissive
    fn resolve_emissive(prop: &Option<ByoColor>, class: Option<Color>) -> Option<LinearRgba> {
        let color = prop.as_ref().map(|c| c.0).or(class)?;
        let srgba = color.to_srgba();
        Some(LinearRgba::new(
            srgba.red,
            srgba.green,
            srgba.blue,
            srgba.alpha,
        ))
    }

    // Colors
    if let Some(c) = resolve_color(&props.base_color, cs.base_color) {
        mat.base_color = c;
    }
    if let Some(e) = resolve_emissive(&props.emissive_color, cs.emissive_color) {
        mat.emissive = e;
    }
    if let Some(c) = resolve_color(&props.attenuation_color, cs.attenuation_color) {
        mat.attenuation_color = c;
    }

    // 0-100 scale properties (prop overrides class)
    if let Some(v) = props.perceptual_roughness.or(cs.perceptual_roughness) {
        mat.perceptual_roughness = v;
    }
    if let Some(v) = props.metallic.or(cs.metallic) {
        mat.metallic = v;
    }
    if let Some(v) = props.reflectance.or(cs.reflectance) {
        mat.reflectance = v;
    }
    if let Some(v) = props.clearcoat.or(cs.clearcoat) {
        mat.clearcoat = v;
    }
    if let Some(v) = props
        .clearcoat_perceptual_roughness
        .or(cs.clearcoat_perceptual_roughness)
    {
        mat.clearcoat_perceptual_roughness = v;
    }
    if let Some(v) = props.anisotropy_strength.or(cs.anisotropy_strength) {
        mat.anisotropy_strength = v;
    }
    if let Some(v) = props.specular_transmission.or(cs.specular_transmission) {
        mat.specular_transmission = v;
    }
    if let Some(v) = props.diffuse_transmission.or(cs.diffuse_transmission) {
        mat.diffuse_transmission = v;
    }

    // Arbitrary float properties (prop overrides class)
    if let Some(v) = props
        .emissive_exposure_weight
        .or(cs.emissive_exposure_weight)
    {
        mat.emissive_exposure_weight = v;
    }
    if let Some(v) = props.ior.or(cs.ior) {
        mat.ior = v;
    }
    if let Some(v) = props.thickness.or(cs.thickness) {
        mat.thickness = v * world_scale;
    }
    if let Some(v) = props.attenuation_distance.or(cs.attenuation_distance) {
        mat.attenuation_distance = v * world_scale;
    }
    if let Some(v) = props.anisotropy_rotation.or(cs.anisotropy_rotation) {
        mat.anisotropy_rotation = v;
    }
    if let Some(v) = props.depth_bias.or(cs.depth_bias) {
        mat.depth_bias = v;
    }

    // Booleans (prop overrides class)
    if let Some(v) = props.unlit.or(cs.unlit) {
        mat.unlit = v;
    }
    if let Some(v) = props.double_sided.or(cs.double_sided) {
        mat.double_sided = v;
    }
    let cull_mode = props.cull_mode.as_ref().or(cs.cull_mode.as_ref());
    if let Some(mode) = cull_mode {
        mat.cull_mode = mode.to_face();
    }
    if let Some(v) = props.fog_enabled.or(cs.fog_enabled) {
        mat.fog_enabled = v;
    }

    // Enum (prop overrides class)
    let alpha_mode = props.alpha_mode.as_ref().or(cs.alpha_mode.as_ref());
    if let Some(mode) = alpha_mode {
        mat.alpha_mode = mode.to_bevy();
    }
}

/// Reconcile `events` and `pointer-events` props onto `EventSubscriptions`
/// and `Pickable` components for views.
pub fn reconcile_view_picking(
    mut commands: Commands,
    query: Query<(Entity, &ViewProps), Changed<ViewProps>>,
    children_query: Query<&Children>,
    all_views: Query<&ViewProps>,
) {
    for (entity, props) in &query {
        let resolved = resolve_view_props(props);
        reconcile_picking_for_entity(
            &mut commands,
            entity,
            resolved.events.as_deref(),
            resolved.pointer_events.as_ref(),
            &children_query,
            &all_views,
        );
    }
}

/// Reconcile `events` and `pointer-events` props for text elements.
pub fn reconcile_text_picking(
    mut commands: Commands,
    query: Query<(Entity, &TextProps), Changed<TextProps>>,
) {
    for (entity, props) in &query {
        if let Some(ref events_str) = props.events {
            commands
                .entity(entity)
                .insert(EventSubscriptions::parse(events_str));
        } else {
            commands.entity(entity).remove::<EventSubscriptions>();
        }

        let pe = props
            .pointer_events
            .as_ref()
            .unwrap_or(&ByoPointerEvents::Auto);
        match pe {
            ByoPointerEvents::Auto => {
                commands.entity(entity).remove::<Pickable>();
            }
            ByoPointerEvents::None => {
                commands.entity(entity).insert(Pickable::IGNORE);
            }
        }
    }
}

/// Reconcile `events` and `pointer-events` props for layers.
pub fn reconcile_layer_picking(
    mut commands: Commands,
    query: Query<(Entity, &LayerProps), Changed<LayerProps>>,
) {
    for (entity, props) in &query {
        // Layers: set EventSubscriptions + Pickable directly (no class resolution needed)
        if let Some(ref events_str) = props.events {
            commands
                .entity(entity)
                .insert(EventSubscriptions::parse(events_str));
        } else {
            commands.entity(entity).remove::<EventSubscriptions>();
        }

        let pe = props
            .pointer_events
            .as_ref()
            .unwrap_or(&ByoPointerEvents::Auto);
        match pe {
            ByoPointerEvents::Auto => {
                commands.entity(entity).remove::<Pickable>();
            }
            ByoPointerEvents::None => {
                commands.entity(entity).insert(Pickable::IGNORE);
            }
        }
    }
}

/// Reconcile `events` and `pointer-events` props for windows.
pub fn reconcile_window_picking(
    mut commands: Commands,
    query: Query<(Entity, &WindowProps), Changed<WindowProps>>,
) {
    for (entity, props) in &query {
        if let Some(ref events_str) = props.events {
            commands
                .entity(entity)
                .insert(EventSubscriptions::parse(events_str));
        } else {
            commands.entity(entity).remove::<EventSubscriptions>();
        }

        let pe = props
            .pointer_events
            .as_ref()
            .unwrap_or(&ByoPointerEvents::Auto);
        match pe {
            ByoPointerEvents::Auto => {
                commands.entity(entity).remove::<Pickable>();
            }
            ByoPointerEvents::None => {
                commands.entity(entity).insert(Pickable::IGNORE);
            }
        }
    }
}

/// Shared logic: set EventSubscriptions + Pickable on an entity,
/// and propagate pointer-events=none to children (CSS inheritance).
fn reconcile_picking_for_entity(
    commands: &mut Commands,
    entity: Entity,
    events: Option<&str>,
    pointer_events: Option<&ByoPointerEvents>,
    children_query: &Query<&Children>,
    all_views: &Query<&ViewProps>,
) {
    // EventSubscriptions
    if let Some(events_str) = events {
        commands
            .entity(entity)
            .insert(EventSubscriptions::parse(events_str));
    } else {
        commands.entity(entity).remove::<EventSubscriptions>();
    }

    // Pickable (with CSS-like inheritance for pointer-events: none)
    let pe = pointer_events.unwrap_or(&ByoPointerEvents::Auto);
    match pe {
        ByoPointerEvents::Auto => {
            // Restore default pickability
            commands.entity(entity).remove::<Pickable>();
        }
        ByoPointerEvents::None => {
            commands.entity(entity).insert(Pickable::IGNORE);
            // Propagate to children that don't have their own explicit pointer-events
            propagate_pointer_events_none(commands, entity, children_query, all_views);
        }
    }
}

/// Recursively propagate pointer-events=none to descendants.
/// Stops at children that have an explicit `pointer_events` prop set.
fn propagate_pointer_events_none(
    commands: &mut Commands,
    parent: Entity,
    children_query: &Query<&Children>,
    all_views: &Query<&ViewProps>,
) {
    let Ok(children) = children_query.get(parent) else {
        return;
    };
    for child in children.iter() {
        // If this child has its own explicit pointer-events prop, respect it
        if let Ok(child_props) = all_views.get(child)
            && child_props.pointer_events.is_some()
        {
            // Child has explicit setting — don't override, but if it's also
            // none, it will propagate when its own reconciliation runs
            continue;
        }
        // Inherit: set IGNORE on this child too
        commands.entity(child).insert(Pickable::IGNORE);
        // Recurse
        propagate_pointer_events_none(commands, child, children_query, all_views);
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
