//! Style reconciliation — watches Changed<ViewProps>/Changed<TextProps>
//! and applies them to Bevy UI components.

pub mod color;
pub mod palette;
pub mod tailwind;

use bevy::prelude::*;

use crate::components::ByoOrder;
use crate::components::ByoTty;
use crate::events::config::EventSubscriptions;
use crate::plugin::WorldScale;
use crate::props::layer::LayerProps;
use crate::props::text::TextProps;
use crate::props::tty::TtyProps;
use crate::props::types::{ByoColor, ByoOrderMode, ByoPointerEvents};
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
        events,
        pointer_events,
    );
    if props.hidden {
        r.hidden = true;
    }
    r
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
            Option<&mut Visibility>,
            Option<&ActiveTransitions>,
        ),
        Changed<ViewProps>,
    >,
) {
    for (props, mut node, mut bg, mut border_color, visibility, active) in &mut query {
        let resolved = resolve_view_props(props);
        let has = |p: AnimatableProp| active.is_some_and(|a| a.has(p));

        // Sizing
        if let Some(ref v) = resolved.width
            && !has(AnimatableProp::Width)
        {
            node.width = v.0;
        }
        if let Some(ref v) = resolved.height
            && !has(AnimatableProp::Height)
        {
            node.height = v.0;
        }
        if let Some(ref v) = resolved.min_width
            && !has(AnimatableProp::MinWidth)
        {
            node.min_width = v.0;
        }
        if let Some(ref v) = resolved.max_width
            && !has(AnimatableProp::MaxWidth)
        {
            node.max_width = v.0;
        }
        if let Some(ref v) = resolved.min_height
            && !has(AnimatableProp::MinHeight)
        {
            node.min_height = v.0;
        }
        if let Some(ref v) = resolved.max_height
            && !has(AnimatableProp::MaxHeight)
        {
            node.max_height = v.0;
        }

        // Colors
        if let Some(ref c) = resolved.background_color
            && !has(AnimatableProp::BackgroundColor)
        {
            *bg = BackgroundColor(c.0);
        }
        if let Some(ref c) = resolved.border_color
            && !has(AnimatableProp::BorderColor)
        {
            *border_color = BorderColor::all(c.0);
        }

        // Border
        if let Some(ref r) = resolved.border_width {
            if !has(AnimatableProp::BorderWidthTop) {
                node.border.top = r.0.top;
            }
            if !has(AnimatableProp::BorderWidthRight) {
                node.border.right = r.0.right;
            }
            if !has(AnimatableProp::BorderWidthBottom) {
                node.border.bottom = r.0.bottom;
            }
            if !has(AnimatableProp::BorderWidthLeft) {
                node.border.left = r.0.left;
            }
        }
        if let Some(ref r) = resolved.border_radius {
            if !has(AnimatableProp::BorderRadiusTopLeft) {
                node.border_radius.top_left = r.0.top_left;
            }
            if !has(AnimatableProp::BorderRadiusTopRight) {
                node.border_radius.top_right = r.0.top_right;
            }
            if !has(AnimatableProp::BorderRadiusBottomRight) {
                node.border_radius.bottom_right = r.0.bottom_right;
            }
            if !has(AnimatableProp::BorderRadiusBottomLeft) {
                node.border_radius.bottom_left = r.0.bottom_left;
            }
        }

        // Spacing
        if let Some(ref r) = resolved.padding {
            if !has(AnimatableProp::PaddingTop) {
                node.padding.top = r.0.top;
            }
            if !has(AnimatableProp::PaddingRight) {
                node.padding.right = r.0.right;
            }
            if !has(AnimatableProp::PaddingBottom) {
                node.padding.bottom = r.0.bottom;
            }
            if !has(AnimatableProp::PaddingLeft) {
                node.padding.left = r.0.left;
            }
        }
        if let Some(ref r) = resolved.margin {
            if !has(AnimatableProp::MarginTop) {
                node.margin.top = r.0.top;
            }
            if !has(AnimatableProp::MarginRight) {
                node.margin.right = r.0.right;
            }
            if !has(AnimatableProp::MarginBottom) {
                node.margin.bottom = r.0.bottom;
            }
            if !has(AnimatableProp::MarginLeft) {
                node.margin.left = r.0.left;
            }
        }

        // Gap
        if let Some(ref v) = resolved.gap {
            if !has(AnimatableProp::ColumnGap) {
                node.column_gap = v.0;
            }
            if !has(AnimatableProp::RowGap) {
                node.row_gap = v.0;
            }
        }
        if let Some(ref v) = resolved.column_gap
            && !has(AnimatableProp::ColumnGap)
        {
            node.column_gap = v.0;
        }
        if let Some(ref v) = resolved.row_gap
            && !has(AnimatableProp::RowGap)
        {
            node.row_gap = v.0;
        }

        // Layout (not animatable — always write immediately)
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
        if let Some(ref v) = resolved.left
            && !has(AnimatableProp::Left)
        {
            node.left = v.0;
        }
        if let Some(ref v) = resolved.right
            && !has(AnimatableProp::Right)
        {
            node.right = v.0;
        }
        if let Some(ref v) = resolved.top
            && !has(AnimatableProp::Top)
        {
            node.top = v.0;
        }
        if let Some(ref v) = resolved.bottom
            && !has(AnimatableProp::Bottom)
        {
            node.bottom = v.0;
        }

        // Flex
        if let Some(v) = resolved.flex_grow
            && !has(AnimatableProp::FlexGrow)
        {
            node.flex_grow = v;
        }
        if let Some(v) = resolved.flex_shrink
            && !has(AnimatableProp::FlexShrink)
        {
            node.flex_shrink = v;
        }
        if let Some(ref v) = resolved.flex_basis
            && !has(AnimatableProp::FlexBasis)
        {
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

/// Resolved tty-specific properties (class merged with wire props, defaults applied).
pub struct ResolvedTtyProps {
    pub font_size: f32,
    pub scrollback: u32,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
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
    }
}

/// Reconcile `TtyProps` class onto the tty's `Node`, `BackgroundColor`, and `BorderColor`.
///
/// Parses the tailwind class string from `TtyProps.class` using the same parser as views.
/// Only fields specified by the class are overridden; tty defaults (flex column, overflow clip,
/// 100% size) set at spawn time are preserved for unspecified fields.
pub fn reconcile_tty_style(
    mut query: Query<
        (&TtyProps, &mut Node, &mut BackgroundColor, &mut BorderColor),
        (Changed<TtyProps>, With<ByoTty>),
    >,
) {
    for (props, mut node, mut bg, mut border_color) in &mut query {
        let Some(ref class) = props.class else {
            continue;
        };
        let mut resolved = ViewProps::default();
        tailwind::apply_classes(&mut resolved, class);

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
        if let Ok(child_props) = all_views.get(child) {
            if child_props.pointer_events.is_some() {
                // Child has explicit setting — don't override, but if it's also
                // none, it will propagate when its own reconciliation runs
                continue;
            }
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
