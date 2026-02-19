//! ECS systems for transition handling: start transitions, tick animations, write values.

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use crate::components::{ByoLayer, ByoTty, ByoView, ByoWindow};
use crate::plugin::WorldScale;
use crate::props::layer::LayerProps;
use crate::props::tty::TtyProps;
use crate::props::types::{ByoOrderMode, ByoShadow, shadow_list_approx_eq};
use crate::props::view::ViewProps;
use crate::props::window::WindowProps;
use crate::render::layer::LayerRender;
use crate::style;
use crate::style::tailwind;

use super::config::*;
use super::state::*;

/// Maximum delta time per tick frame. Prevents transitions from jumping or
/// completing in a single frame when the app wakes after a long idle in
/// `desktop_app()` reactive mode.
const MAX_TICK_DELTA: f32 = 1.0 / 30.0; // ~33ms, ensures at least ~30 visual steps

// ===========================================================================
// Handle transitions (PostUpdate, before reconcile)
// ===========================================================================

/// Combined extract + start for view transitions.
/// Reads Changed<ViewProps>, builds TransitionConfig, starts ActiveTransitions.
#[allow(clippy::type_complexity)]
pub fn handle_view_transitions(
    mut query: Query<
        (
            Entity,
            &ViewProps,
            &Node,
            &ComputedNode,
            &BackgroundColor,
            &BorderColor,
            &BoxShadow,
            &UiTransform,
            &mut TransitionConfig,
            &mut ActiveTransitions,
        ),
        Changed<ViewProps>,
    >,
    new_views: Query<Entity, Added<ViewProps>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut started_new = false;

    for (
        entity,
        props,
        node,
        computed,
        bg,
        border_color,
        box_shadow,
        ui_transform,
        mut config,
        mut active,
    ) in &mut query
    {
        let resolved = style::resolve_view_props(props);

        // Build transition config from resolved props
        *config = build_transition_config(
            &resolved.transition,
            &resolved.tw_transition_property,
            resolved.tw_transition_duration,
            resolved.tw_transition_easing,
            resolved.tw_transition_delay,
        );

        // Skip first appearance (no animation on creation)
        if new_views.contains(entity) {
            continue;
        }

        // If no transitions configured, clear any in-progress ones
        if config.rules.is_empty() {
            active.transitions.clear();
            continue;
        }

        let mut transitions = std::mem::take(&mut active.transitions);
        let count_before = transitions.len();

        // --- Val fields ---
        // Width/height: we transition declared style values (not computed layout from
        // ComputedNode) so that Bevy's flex layout recalculates each frame from the
        // interpolated style value — flex grow/shrink effects should NOT be baked into
        // the transition endpoints. However, we apply min/max clamping explicitly:
        // interpolating beyond the constraints is visually indistinguishable (Bevy clamps
        // the layout result anyway) and would waste animation time on an imperceptible range.
        let cur_min_w = node.min_width;
        let cur_max_w = node.max_width;
        let tgt_min_w = resolved
            .min_width
            .as_ref()
            .map(|v| v.0)
            .unwrap_or(cur_min_w);
        let tgt_max_w = resolved
            .max_width
            .as_ref()
            .map(|v| v.0)
            .unwrap_or(cur_max_w);
        check_val_clamped(
            &mut transitions,
            &config,
            AnimatableProp::Width,
            node.width,
            resolved.width.as_ref().map(|v| v.0),
            cur_min_w,
            cur_max_w,
            tgt_min_w,
            tgt_max_w,
        );

        let cur_min_h = node.min_height;
        let cur_max_h = node.max_height;
        let tgt_min_h = resolved
            .min_height
            .as_ref()
            .map(|v| v.0)
            .unwrap_or(cur_min_h);
        let tgt_max_h = resolved
            .max_height
            .as_ref()
            .map(|v| v.0)
            .unwrap_or(cur_max_h);
        check_val_clamped(
            &mut transitions,
            &config,
            AnimatableProp::Height,
            node.height,
            resolved.height.as_ref().map(|v| v.0),
            cur_min_h,
            cur_max_h,
            tgt_min_h,
            tgt_max_h,
        );

        check_val(
            &mut transitions,
            &config,
            AnimatableProp::MinWidth,
            node.min_width,
            resolved.min_width.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::MaxWidth,
            node.max_width,
            resolved.max_width.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::MinHeight,
            node.min_height,
            resolved.min_height.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::MaxHeight,
            node.max_height,
            resolved.max_height.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::Left,
            node.left,
            resolved.left.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::Right,
            node.right,
            resolved.right.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::Top,
            node.top,
            resolved.top.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::Bottom,
            node.bottom,
            resolved.bottom.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::FlexBasis,
            node.flex_basis,
            resolved.flex_basis.as_ref().map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::ColumnGap,
            node.column_gap,
            resolved
                .gap
                .as_ref()
                .or(resolved.column_gap.as_ref())
                .map(|v| v.0),
        );
        check_val(
            &mut transitions,
            &config,
            AnimatableProp::RowGap,
            node.row_gap,
            resolved
                .gap
                .as_ref()
                .or(resolved.row_gap.as_ref())
                .map(|v| v.0),
        );

        // Padding sides
        if let Some(ref target) = resolved.padding {
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::PaddingTop,
                node.padding.top,
                Some(target.0.top),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::PaddingRight,
                node.padding.right,
                Some(target.0.right),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::PaddingBottom,
                node.padding.bottom,
                Some(target.0.bottom),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::PaddingLeft,
                node.padding.left,
                Some(target.0.left),
            );
        }

        // Margin sides
        if let Some(ref target) = resolved.margin {
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::MarginTop,
                node.margin.top,
                Some(target.0.top),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::MarginRight,
                node.margin.right,
                Some(target.0.right),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::MarginBottom,
                node.margin.bottom,
                Some(target.0.bottom),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::MarginLeft,
                node.margin.left,
                Some(target.0.left),
            );
        }

        // Border width sides
        if let Some(ref target) = resolved.border_width {
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::BorderWidthTop,
                node.border.top,
                Some(target.0.top),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::BorderWidthRight,
                node.border.right,
                Some(target.0.right),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::BorderWidthBottom,
                node.border.bottom,
                Some(target.0.bottom),
            );
            check_val(
                &mut transitions,
                &config,
                AnimatableProp::BorderWidthLeft,
                node.border.left,
                Some(target.0.left),
            );
        }

        // Border radius corners — use ComputedNode's resolved border radius as the "from"
        // value (already clamped per-corner by Bevy's layout), and resolve the target Val
        // with the same formula, so transitions span only the perceptible visual range.
        if let Some(ref target) = resolved.border_radius {
            let resolved_br = computed.border_radius();
            let sz = computed.size;
            let isf = computed.inverse_scale_factor;
            check_border_radius(
                &mut transitions,
                &config,
                AnimatableProp::BorderRadiusTopLeft,
                resolved_br.top_left,
                target.0.top_left,
                sz,
                isf,
            );
            check_border_radius(
                &mut transitions,
                &config,
                AnimatableProp::BorderRadiusTopRight,
                resolved_br.top_right,
                target.0.top_right,
                sz,
                isf,
            );
            check_border_radius(
                &mut transitions,
                &config,
                AnimatableProp::BorderRadiusBottomRight,
                resolved_br.bottom_right,
                target.0.bottom_right,
                sz,
                isf,
            );
            check_border_radius(
                &mut transitions,
                &config,
                AnimatableProp::BorderRadiusBottomLeft,
                resolved_br.bottom_left,
                target.0.bottom_left,
                sz,
                isf,
            );
        }

        // --- Color fields ---
        check_color(
            &mut transitions,
            &config,
            AnimatableProp::BackgroundColor,
            bg.0,
            resolved.background_color.as_ref().map(|c| c.0),
        );
        // BorderColor: read top (we always set via ::all, so all sides are the same)
        check_color(
            &mut transitions,
            &config,
            AnimatableProp::BorderColor,
            border_color.top,
            resolved.border_color.as_ref().map(|c| c.0),
        );

        // --- BoxShadow ---
        check_shadow_list(
            &mut transitions,
            &config,
            &box_shadow.0,
            &style::resolve_box_shadow(&resolved),
        );

        // --- f32 fields ---
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::FlexGrow,
            node.flex_grow,
            resolved.flex_grow,
        );
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::FlexShrink,
            node.flex_shrink,
            resolved.flex_shrink,
        );

        // --- 2D transforms ---
        let current_tx = match ui_transform.translation.x {
            Val::Px(v) => v,
            _ => 0.0,
        };
        let current_ty = match ui_transform.translation.y {
            Val::Px(v) => v,
            _ => 0.0,
        };
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::TranslateX,
            current_tx,
            resolved.translate_x,
        );
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::TranslateY,
            current_ty,
            resolved.translate_y,
        );
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::Rotate,
            ui_transform.rotation.as_radians().to_degrees(),
            resolved.rotate,
        );

        let uniform = resolved.scale.unwrap_or(1.0);
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::ScaleX,
            ui_transform.scale.x,
            resolved.scale_x.or(Some(uniform)),
        );
        check_f32(
            &mut transitions,
            &config,
            AnimatableProp::ScaleY,
            ui_transform.scale.y,
            resolved.scale_y.or(Some(uniform)),
        );

        if transitions.len() > count_before {
            started_new = true;
            for t in &transitions[count_before..] {
                match &t.state {
                    TransitionState::Eased {
                        from,
                        to,
                        duration_secs,
                        ..
                    } => {
                        debug!(
                            "transition started: {:?} from {:?} to {:?} over {:.3}s",
                            t.prop, from, to, duration_secs
                        );
                    }
                    TransitionState::PhysicsSpring {
                        current,
                        target,
                        stiffness,
                        damping,
                        ..
                    } => {
                        debug!(
                            "physics spring started: {:?} from {:.3} to {:.3} (k={}, d={})",
                            t.prop, current, target, stiffness, damping
                        );
                    }
                }
            }
        }

        active.transitions = transitions;
    }

    // Request redraw if new transitions were created (tick systems run in Update,
    // which already happened this frame — without this, new transitions would not
    // get their first tick until the next external wake)
    if started_new {
        redraw.write(RequestRedraw);
    }
}

/// Combined extract + start for window transitions.
#[allow(clippy::type_complexity)]
pub fn handle_window_transitions(
    mut query: Query<
        (
            Entity,
            &WindowProps,
            &Transform,
            &mut TransitionConfig,
            &mut ActiveTransitions,
        ),
        Changed<WindowProps>,
    >,
    new_windows: Query<Entity, Added<WindowProps>>,
    world_scale: Res<WorldScale>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut started_new = false;

    for (entity, props, transform, mut config, mut active) in &mut query {
        *config = build_transition_config(
            &props.transition,
            &props.tw_transition_property,
            props.tw_transition_duration,
            props.tw_transition_easing,
            props.tw_transition_delay,
        );

        if new_windows.contains(entity) {
            continue;
        }
        if config.rules.is_empty() {
            active.transitions.clear();
            continue;
        }

        let mut transitions = std::mem::take(&mut active.transitions);
        let count_before = transitions.len();

        // Resolve target values from class + individual props
        let mut ts = tailwind::TransformStyle::default();
        if let Some(ref class) = props.class {
            tailwind::apply_transform_classes(&mut ts, class);
        }

        let ws = world_scale.0;
        start_3d_transform_transitions(
            &mut transitions,
            &config,
            transform,
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
            ws,
        );

        if transitions.len() > count_before {
            started_new = true;
        }

        active.transitions = transitions;
    }

    if started_new {
        redraw.write(RequestRedraw);
    }
}

/// Combined extract + start for layer transitions.
#[allow(clippy::type_complexity)]
pub fn handle_layer_transitions(
    mut query: Query<
        (
            Entity,
            &LayerProps,
            &LayerRender,
            &mut TransitionConfig,
            &mut ActiveTransitions,
        ),
        Changed<LayerProps>,
    >,
    new_layers: Query<Entity, Added<LayerProps>>,
    transforms: Query<&Transform>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    world_scale: Res<WorldScale>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut started_new = false;

    for (entity, props, render, mut config, mut active) in &mut query {
        *config = build_transition_config(
            &props.transition,
            &props.tw_transition_property,
            props.tw_transition_duration,
            props.tw_transition_easing,
            props.tw_transition_delay,
        );

        if new_layers.contains(entity) {
            continue;
        }
        if config.rules.is_empty() {
            active.transitions.clear();
            continue;
        }

        let mut transitions = std::mem::take(&mut active.transitions);
        let count_before = transitions.len();

        // 3D transform transitions (on plane entity)
        if let Ok(plane_transform) = transforms.get(render.plane) {
            let mut ts = tailwind::TransformStyle::default();
            if let Some(ref class) = props.class {
                tailwind::apply_transform_classes(&mut ts, class);
            }

            start_3d_transform_transitions(
                &mut transitions,
                &config,
                plane_transform,
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
                world_scale.0,
            );
        }

        // PBR material transitions (on plane entity)
        if let Ok(mat_handle) = material_handles.get(render.plane)
            && let Some(mat) = materials.get(&mat_handle.0)
        {
            // PBR f32 fields
            let mut cs = tailwind::TransformStyle::default();
            if let Some(ref class) = props.class {
                tailwind::apply_transform_classes(&mut cs, class);
            }

            check_f32(
                &mut transitions,
                &config,
                AnimatableProp::PerceptualRoughness,
                mat.perceptual_roughness,
                props.perceptual_roughness.or(cs.perceptual_roughness),
            );
            check_f32(
                &mut transitions,
                &config,
                AnimatableProp::Metallic,
                mat.metallic,
                props.metallic.or(cs.metallic),
            );
            check_f32(
                &mut transitions,
                &config,
                AnimatableProp::Reflectance,
                mat.reflectance,
                props.reflectance.or(cs.reflectance),
            );

            // PBR color fields
            check_color(
                &mut transitions,
                &config,
                AnimatableProp::BaseColor,
                mat.base_color,
                props.base_color.as_ref().map(|c| c.0).or(cs.base_color),
            );

            // Emissive: stored as LinearRgba, convert for comparison
            let current_emissive = Color::LinearRgba(mat.emissive);
            let target_emissive = props
                .emissive_color
                .as_ref()
                .map(|c| c.0)
                .or(cs.emissive_color);
            check_color(
                &mut transitions,
                &config,
                AnimatableProp::EmissiveColor,
                current_emissive,
                target_emissive,
            );
        }

        if transitions.len() > count_before {
            started_new = true;
        }

        active.transitions = transitions;
    }

    if started_new {
        redraw.write(RequestRedraw);
    }
}

// ===========================================================================
// Tick transitions (Update)
// ===========================================================================

/// Advance view transitions and write interpolated values to Bevy components.
#[allow(clippy::type_complexity)]
pub fn tick_view_transitions(
    mut query: Query<
        (
            &mut ActiveTransitions,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
            &mut UiTransform,
        ),
        (With<ByoView>, Without<ByoWindow>, Without<ByoLayer>),
    >,
    time: Res<Time>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    // Cap delta to prevent huge jumps after idle (desktop_app mode can report
    // large deltas on the first frame after the app was sleeping)
    let raw_delta = time.delta_secs();
    let delta = raw_delta.min(MAX_TICK_DELTA);
    let mut needs_redraw = false;

    for (mut active, mut node, mut bg, mut border_color, mut box_shadow, mut ui_transform) in
        &mut query
    {
        if active.transitions.is_empty() {
            continue;
        }

        if raw_delta > 0.1 {
            debug!("tick_view: capped delta {:.3}s → {:.3}s", raw_delta, delta);
        }

        // Advance all and collect interpolated values
        active.transitions.retain_mut(|t| {
            t.tick(delta);
            let value = t.current_value();

            write_view_value(
                t.prop,
                &value,
                &mut node,
                &mut bg,
                &mut border_color,
                &mut box_shadow,
                &mut ui_transform,
            );

            if t.is_complete() {
                // Write exact final value
                let final_val = t.final_value();
                write_view_value(
                    t.prop,
                    &final_val,
                    &mut node,
                    &mut bg,
                    &mut border_color,
                    &mut box_shadow,
                    &mut ui_transform,
                );
                debug!("transition complete: {:?}", t.prop);
                false
            } else {
                true
            }
        });

        if !active.transitions.is_empty() {
            needs_redraw = true;
        }
    }

    if needs_redraw {
        redraw.write(RequestRedraw);
    }
}

/// Advance window transitions and write interpolated values.
#[allow(clippy::type_complexity)]
pub fn tick_window_transitions(
    mut query: Query<
        (&mut ActiveTransitions, &WindowProps, &mut Transform),
        (With<ByoWindow>, Without<ByoView>, Without<ByoLayer>),
    >,
    world_scale: Res<WorldScale>,
    time: Res<Time>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let delta = time.delta_secs().min(MAX_TICK_DELTA);
    let mut needs_redraw = false;

    for (mut active, props, mut transform) in &mut query {
        if active.transitions.is_empty() {
            continue;
        }

        // Advance all transitions
        for t in &mut active.transitions {
            t.tick(delta);
        }

        // Resolve base values from props
        let mut ts = tailwind::TransformStyle::default();
        if let Some(ref class) = props.class {
            tailwind::apply_transform_classes(&mut ts, class);
        }

        let order = props.order.unwrap_or(0) as f32;
        let mode = props
            .order_mode
            .as_ref()
            .unwrap_or(&ByoOrderMode::TranslateZ);
        let order_scale = props.order_scale.unwrap_or(0.001);
        let order_z = match mode {
            ByoOrderMode::TranslateZ => order * order_scale,
            _ => 0.0,
        };

        let ws = world_scale.0;
        let tx = get_override_f32(&active, AnimatableProp::TranslateX)
            .unwrap_or_else(|| props.translate_x.or(ts.translate_x).unwrap_or(0.0));
        let ty = get_override_f32(&active, AnimatableProp::TranslateY)
            .unwrap_or_else(|| props.translate_y.or(ts.translate_y).unwrap_or(0.0));
        let tz = get_override_f32(&active, AnimatableProp::TranslateZ)
            .unwrap_or_else(|| props.translate_z.or(ts.translate_z).unwrap_or(0.0));
        let rot = get_override_f32(&active, AnimatableProp::Rotate)
            .unwrap_or_else(|| props.rotate.or(ts.rotate).unwrap_or(0.0));
        let rx = get_override_f32(&active, AnimatableProp::RotateX)
            .unwrap_or_else(|| props.rotate_x.or(ts.rotate_x).unwrap_or(0.0));
        let ry = get_override_f32(&active, AnimatableProp::RotateY)
            .unwrap_or_else(|| props.rotate_y.or(ts.rotate_y).unwrap_or(0.0));
        let rz = get_override_f32(&active, AnimatableProp::RotateZ)
            .unwrap_or_else(|| props.rotate_z.or(ts.rotate_z).unwrap_or(rot));
        let uniform = get_override_f32(&active, AnimatableProp::Scale)
            .unwrap_or_else(|| props.scale.or(ts.scale).unwrap_or(1.0));
        let sx = get_override_f32(&active, AnimatableProp::ScaleX)
            .unwrap_or_else(|| props.scale_x.or(ts.scale_x).unwrap_or(uniform));
        let sy = get_override_f32(&active, AnimatableProp::ScaleY)
            .unwrap_or_else(|| props.scale_y.or(ts.scale_y).unwrap_or(uniform));
        let sz = get_override_f32(&active, AnimatableProp::ScaleZ)
            .unwrap_or_else(|| props.scale_z.or(ts.scale_z).unwrap_or(uniform));

        *transform = compose_3d_transform(tx, ty, tz, rx, ry, rz, sx, sy, sz, order_z, ws);

        // Remove completed
        active.transitions.retain(|t| !t.is_complete());

        if !active.transitions.is_empty() {
            needs_redraw = true;
        }
    }

    if needs_redraw {
        redraw.write(RequestRedraw);
    }
}

/// Advance layer transitions and write interpolated values.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn tick_layer_transitions(
    mut query: Query<
        (&mut ActiveTransitions, &LayerProps, &LayerRender),
        (With<ByoLayer>, Without<ByoView>, Without<ByoWindow>),
    >,
    mut transforms: Query<&mut Transform>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world_scale: Res<WorldScale>,
    time: Res<Time>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let delta = time.delta_secs().min(MAX_TICK_DELTA);
    let mut needs_redraw = false;

    for (mut active, props, render) in &mut query {
        if active.transitions.is_empty() {
            continue;
        }

        // Advance all transitions
        for t in &mut active.transitions {
            t.tick(delta);
        }

        // --- 3D Transform on plane entity ---
        if active.has_any_3d_transform()
            && let Ok(mut plane_transform) = transforms.get_mut(render.plane)
        {
            let mut ts = tailwind::TransformStyle::default();
            if let Some(ref class) = props.class {
                tailwind::apply_transform_classes(&mut ts, class);
            }

            let order = props.order.unwrap_or(0) as f32;
            let mode = props
                .order_mode
                .as_ref()
                .unwrap_or(&ByoOrderMode::TranslateZ);
            let order_scale = props.order_scale.unwrap_or(0.001);
            let order_z = match mode {
                ByoOrderMode::TranslateZ => order * order_scale,
                _ => 0.0,
            };

            let ws = world_scale.0;
            let tx = get_override_f32(&active, AnimatableProp::TranslateX)
                .unwrap_or_else(|| props.translate_x.or(ts.translate_x).unwrap_or(0.0));
            let ty = get_override_f32(&active, AnimatableProp::TranslateY)
                .unwrap_or_else(|| props.translate_y.or(ts.translate_y).unwrap_or(0.0));
            let tz = get_override_f32(&active, AnimatableProp::TranslateZ)
                .unwrap_or_else(|| props.translate_z.or(ts.translate_z).unwrap_or(0.0));
            let rot = get_override_f32(&active, AnimatableProp::Rotate)
                .unwrap_or_else(|| props.rotate.or(ts.rotate).unwrap_or(0.0));
            let rx = get_override_f32(&active, AnimatableProp::RotateX)
                .unwrap_or_else(|| props.rotate_x.or(ts.rotate_x).unwrap_or(0.0));
            let ry = get_override_f32(&active, AnimatableProp::RotateY)
                .unwrap_or_else(|| props.rotate_y.or(ts.rotate_y).unwrap_or(0.0));
            let rz = get_override_f32(&active, AnimatableProp::RotateZ)
                .unwrap_or_else(|| props.rotate_z.or(ts.rotate_z).unwrap_or(rot));
            let uniform = get_override_f32(&active, AnimatableProp::Scale)
                .unwrap_or_else(|| props.scale.or(ts.scale).unwrap_or(1.0));
            let sx = get_override_f32(&active, AnimatableProp::ScaleX)
                .unwrap_or_else(|| props.scale_x.or(ts.scale_x).unwrap_or(uniform));
            let sy = get_override_f32(&active, AnimatableProp::ScaleY)
                .unwrap_or_else(|| props.scale_y.or(ts.scale_y).unwrap_or(uniform));
            let sz = get_override_f32(&active, AnimatableProp::ScaleZ)
                .unwrap_or_else(|| props.scale_z.or(ts.scale_z).unwrap_or(uniform));

            *plane_transform =
                compose_3d_transform(tx, ty, tz, rx, ry, rz, sx, sy, sz, order_z, ws);
        }

        // --- PBR Material on plane entity ---
        if let Ok(mat_handle) = material_handles.get(render.plane)
            && let Some(mat) = materials.get_mut(&mat_handle.0)
        {
            if let Some(v) = get_override_f32(&active, AnimatableProp::PerceptualRoughness) {
                mat.perceptual_roughness = v;
            }
            if let Some(v) = get_override_f32(&active, AnimatableProp::Metallic) {
                mat.metallic = v;
            }
            if let Some(v) = get_override_f32(&active, AnimatableProp::Reflectance) {
                mat.reflectance = v;
            }
            if let Some(c) = get_override_color(&active, AnimatableProp::BaseColor) {
                mat.base_color = c;
            }
            if let Some(c) = get_override_color(&active, AnimatableProp::EmissiveColor) {
                let srgba = c.to_srgba();
                mat.emissive = LinearRgba::new(srgba.red, srgba.green, srgba.blue, srgba.alpha);
            }
        }

        // Remove completed
        active.transitions.retain(|t| !t.is_complete());

        if !active.transitions.is_empty() {
            needs_redraw = true;
        }
    }

    if needs_redraw {
        redraw.write(RequestRedraw);
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Check a border radius corner for transition using effective (resolved) values.
///
/// "from" comes from `ComputedNode::border_radius()` (physical px, already clamped by Bevy).
/// "to" is the target `Val` from resolved props, clamped the same way Bevy does:
///   `val_physical.clamp(0, 0.5 * min(node_width, node_height))`.
///
/// Both values are converted to logical pixels (`Val::Px`) for interpolation,
/// ensuring transitions span only the perceptible visual range (not 0→9999px).
fn check_border_radius(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    prop: AnimatableProp,
    current_physical: f32,
    target_val: Val,
    node_size_physical: Vec2,
    inverse_scale_factor: f32,
) {
    let min_dim = node_size_physical.x.min(node_size_physical.y);
    let max_physical = 0.5 * min_dim;

    // "from": resolved physical → logical
    let from = (current_physical.clamp(0.0, max_physical)) * inverse_scale_factor;

    // "to": resolve target Val to physical, clamp, convert to logical
    let to = match target_val {
        Val::Px(r) => (r / inverse_scale_factor).clamp(0.0, max_physical) * inverse_scale_factor,
        Val::Percent(p) => (p / 100.0 * min_dim).clamp(0.0, max_physical) * inverse_scale_factor,
        Val::Vw(v) | Val::Vh(v) | Val::VMin(v) | Val::VMax(v) => {
            // Viewport-relative: approximate — just clamp to max_physical
            (v / inverse_scale_factor).clamp(0.0, max_physical) * inverse_scale_factor
        }
        Val::Auto => 0.0,
    };

    if (from - to).abs() > 0.01
        && let Some(rule) = config.find_rule(prop)
    {
        // Physics springs only support f32; border radius uses Val → skip
        if let TransitionType::Eased {
            duration_secs,
            delay_secs,
            easing,
        } = &rule.transition_type
        {
            upsert_transition(
                transitions,
                prop,
                AnimatableValue::Val(Val::Px(from)),
                AnimatableValue::Val(Val::Px(to)),
                *duration_secs,
                *delay_secs,
                *easing,
            );
        }
    }
}

/// Check a Val field for transition.
fn check_val(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    prop: AnimatableProp,
    current: Val,
    target: Option<Val>,
) {
    if let Some(target) = target
        && !val_approx_eq(current, target)
        && let Some(rule) = config.find_rule(prop)
    {
        // Physics springs only support f32; Val properties use eased transitions only
        if let TransitionType::Eased {
            duration_secs,
            delay_secs,
            easing,
        } = &rule.transition_type
        {
            upsert_transition(
                transitions,
                prop,
                AnimatableValue::Val(current),
                AnimatableValue::Val(target),
                *duration_secs,
                *delay_secs,
                *easing,
            );
        }
    }
}

/// Check a Val field for transition, applying min/max clamping to both endpoints.
///
/// Used for width/height: we transition declared style values (so flex grow/shrink
/// recalculates each frame) but clamp by min/max constraints so the interpolation
/// range stays within the perceptible visual range.
#[allow(clippy::too_many_arguments)]
fn check_val_clamped(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    prop: AnimatableProp,
    current: Val,
    target: Option<Val>,
    current_min: Val,
    current_max: Val,
    target_min: Val,
    target_max: Val,
) {
    if let Some(target) = target {
        let clamped_from = clamp_val(current, current_min, current_max);
        let clamped_to = clamp_val(target, target_min, target_max);
        if !val_approx_eq(clamped_from, clamped_to)
            && let Some(rule) = config.find_rule(prop)
        {
            // Physics springs only support f32; Val properties use eased transitions only
            if let TransitionType::Eased {
                duration_secs,
                delay_secs,
                easing,
            } = &rule.transition_type
            {
                upsert_transition(
                    transitions,
                    prop,
                    AnimatableValue::Val(clamped_from),
                    AnimatableValue::Val(clamped_to),
                    *duration_secs,
                    *delay_secs,
                    *easing,
                );
            }
        }
    }
}

/// Clamp a Val by min and max constraints. Only clamps when the constraint is the
/// same variant (e.g. Px clamps Px, Percent clamps Percent). `Val::Auto` and
/// mismatched variants are treated as "no constraint".
fn clamp_val(val: Val, min: Val, max: Val) -> Val {
    match val {
        Val::Px(v) => {
            let lo = if let Val::Px(m) = min {
                m
            } else {
                f32::NEG_INFINITY
            };
            let hi = if let Val::Px(m) = max {
                m
            } else {
                f32::INFINITY
            };
            Val::Px(v.clamp(lo, hi))
        }
        Val::Percent(v) => {
            let lo = if let Val::Percent(m) = min {
                m
            } else {
                f32::NEG_INFINITY
            };
            let hi = if let Val::Percent(m) = max {
                m
            } else {
                f32::INFINITY
            };
            Val::Percent(v.clamp(lo, hi))
        }
        other => other,
    }
}

/// Check an f32 field for transition (supports both eased and physics spring).
fn check_f32(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    prop: AnimatableProp,
    current: f32,
    target: Option<f32>,
) {
    if let Some(target) = target
        && let Some(rule) = config.find_rule(prop)
    {
        match &rule.transition_type {
            TransitionType::Eased {
                duration_secs,
                delay_secs,
                easing,
            } => {
                if (current - target).abs() > 0.001 {
                    upsert_transition(
                        transitions,
                        prop,
                        AnimatableValue::F32(current),
                        AnimatableValue::F32(target),
                        *duration_secs,
                        *delay_secs,
                        *easing,
                    );
                }
            }
            TransitionType::PhysicsSpring { stiffness, damping } => {
                let has_active = transitions.iter().any(|t| t.prop == prop);
                if has_active || (current - target).abs() > 0.001 {
                    upsert_physics_spring(transitions, prop, current, target, *stiffness, *damping);
                }
            }
        }
    }
}

/// Check shadow list for transition.
fn check_shadow_list(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    current_bevy: &[bevy::ui::ShadowStyle],
    target: &[ByoShadow],
) {
    // Convert current Bevy ShadowStyle list to ByoShadow for comparison
    // (doubles blur_radius back from Bevy sigma to CSS convention)
    let current: Vec<ByoShadow> = current_bevy
        .iter()
        .map(ByoShadow::from_shadow_style)
        .collect();
    // Filter out inset from target (Bevy doesn't support inset)
    let filtered_target: Vec<ByoShadow> = target.iter().filter(|s| !s.inset).cloned().collect();
    if !shadow_list_approx_eq(&current, &filtered_target)
        && let Some(rule) = config.find_rule(AnimatableProp::BoxShadow)
    {
        if let TransitionType::Eased {
            duration_secs,
            delay_secs,
            easing,
        } = &rule.transition_type
        {
            upsert_transition(
                transitions,
                AnimatableProp::BoxShadow,
                AnimatableValue::ShadowList(current),
                AnimatableValue::ShadowList(filtered_target),
                *duration_secs,
                *delay_secs,
                *easing,
            );
        }
    }
}

/// Check a Color field for transition.
fn check_color(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    prop: AnimatableProp,
    current: Color,
    target: Option<Color>,
) {
    if let Some(target) = target
        && !color_approx_eq(current, target)
        && let Some(rule) = config.find_rule(prop)
    {
        // Physics springs only support f32; color properties use eased transitions only
        if let TransitionType::Eased {
            duration_secs,
            delay_secs,
            easing,
        } = &rule.transition_type
        {
            upsert_transition(
                transitions,
                prop,
                AnimatableValue::Color(current),
                AnimatableValue::Color(target),
                *duration_secs,
                *delay_secs,
                *easing,
            );
        }
    }
}

/// Start 3D transform transitions by comparing current Transform against resolved props.
#[allow(clippy::too_many_arguments)]
fn start_3d_transform_transitions(
    transitions: &mut Vec<ActiveTransition>,
    config: &TransitionConfig,
    current: &Transform,
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
    world_scale: f32,
) {
    // Decompose current transform to individual values
    let current_tx = current.translation.x / world_scale;
    let current_ty = current.translation.y / world_scale;
    // Note: tz includes order_z contribution, but we compare against target which also includes it
    let current_tz = current.translation.z / world_scale;
    let (crx, cry, crz) = current.rotation.to_euler(EulerRot::XYZ);
    let current_rx = crx.to_degrees();
    let current_ry = cry.to_degrees();
    let current_rz = crz.to_degrees();
    let current_sx = current.scale.x;
    let current_sy = current.scale.y;
    let current_sz = current.scale.z;

    // Target values (class as fallback, prop overrides)
    let target_tx = prop_tx.or(ts.translate_x).unwrap_or(0.0);
    let target_ty = prop_ty.or(ts.translate_y).unwrap_or(0.0);
    let target_tz = prop_tz.or(ts.translate_z).unwrap_or(0.0);
    let target_rot = prop_rotate.or(ts.rotate).unwrap_or(0.0);
    let target_rx = prop_rx.or(ts.rotate_x).unwrap_or(0.0);
    let target_ry = prop_ry.or(ts.rotate_y).unwrap_or(0.0);
    let target_rz = prop_rz.or(ts.rotate_z).unwrap_or(target_rot);
    let uniform = prop_scale.or(ts.scale).unwrap_or(1.0);
    let target_sx = prop_sx.or(ts.scale_x).unwrap_or(uniform);
    let target_sy = prop_sy.or(ts.scale_y).unwrap_or(uniform);
    let target_sz = prop_sz.or(ts.scale_z).unwrap_or(uniform);

    check_f32(
        transitions,
        config,
        AnimatableProp::TranslateX,
        current_tx,
        Some(target_tx),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::TranslateY,
        current_ty,
        Some(target_ty),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::TranslateZ,
        current_tz,
        Some(target_tz),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::RotateX,
        current_rx,
        Some(target_rx),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::RotateY,
        current_ry,
        Some(target_ry),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::RotateZ,
        current_rz,
        Some(target_rz),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::ScaleX,
        current_sx,
        Some(target_sx),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::ScaleY,
        current_sy,
        Some(target_sy),
    );
    check_f32(
        transitions,
        config,
        AnimatableProp::ScaleZ,
        current_sz,
        Some(target_sz),
    );
}

/// Get the interpolated f32 override for a property from active transitions.
fn get_override_f32(active: &ActiveTransitions, prop: AnimatableProp) -> Option<f32> {
    active
        .transitions
        .iter()
        .find(|t| t.prop == prop)
        .and_then(|t| t.current_value().as_f32())
}

/// Get the interpolated Color override for a property from active transitions.
fn get_override_color(active: &ActiveTransitions, prop: AnimatableProp) -> Option<Color> {
    active
        .transitions
        .iter()
        .find(|t| t.prop == prop)
        .and_then(|t| t.current_value().as_color())
}

/// Compose a 3D Transform from individual values.
#[allow(clippy::too_many_arguments)]
fn compose_3d_transform(
    tx: f32,
    ty: f32,
    tz: f32,
    rx: f32,
    ry: f32,
    rz: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    order_z: f32,
    world_scale: f32,
) -> Transform {
    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        rx.to_radians(),
        ry.to_radians(),
        rz.to_radians(),
    );
    Transform {
        translation: Vec3::new(
            tx * world_scale,
            ty * world_scale,
            tz * world_scale + order_z,
        ),
        rotation,
        scale: Vec3::new(sx, sy, sz),
    }
}

/// Write a single interpolated value to the appropriate view Bevy component.
fn write_view_value(
    prop: AnimatableProp,
    value: &AnimatableValue,
    node: &mut Node,
    bg: &mut BackgroundColor,
    border_color: &mut BorderColor,
    box_shadow: &mut BoxShadow,
    ui_transform: &mut UiTransform,
) {
    match prop {
        // Val fields → Node
        AnimatableProp::Width => {
            if let Some(v) = value.as_val() {
                node.width = v;
            }
        }
        AnimatableProp::Height => {
            if let Some(v) = value.as_val() {
                node.height = v;
            }
        }
        AnimatableProp::MinWidth => {
            if let Some(v) = value.as_val() {
                node.min_width = v;
            }
        }
        AnimatableProp::MaxWidth => {
            if let Some(v) = value.as_val() {
                node.max_width = v;
            }
        }
        AnimatableProp::MinHeight => {
            if let Some(v) = value.as_val() {
                node.min_height = v;
            }
        }
        AnimatableProp::MaxHeight => {
            if let Some(v) = value.as_val() {
                node.max_height = v;
            }
        }
        AnimatableProp::PaddingTop => {
            if let Some(v) = value.as_val() {
                node.padding.top = v;
            }
        }
        AnimatableProp::PaddingRight => {
            if let Some(v) = value.as_val() {
                node.padding.right = v;
            }
        }
        AnimatableProp::PaddingBottom => {
            if let Some(v) = value.as_val() {
                node.padding.bottom = v;
            }
        }
        AnimatableProp::PaddingLeft => {
            if let Some(v) = value.as_val() {
                node.padding.left = v;
            }
        }
        AnimatableProp::MarginTop => {
            if let Some(v) = value.as_val() {
                node.margin.top = v;
            }
        }
        AnimatableProp::MarginRight => {
            if let Some(v) = value.as_val() {
                node.margin.right = v;
            }
        }
        AnimatableProp::MarginBottom => {
            if let Some(v) = value.as_val() {
                node.margin.bottom = v;
            }
        }
        AnimatableProp::MarginLeft => {
            if let Some(v) = value.as_val() {
                node.margin.left = v;
            }
        }
        AnimatableProp::BorderWidthTop => {
            if let Some(v) = value.as_val() {
                node.border.top = v;
            }
        }
        AnimatableProp::BorderWidthRight => {
            if let Some(v) = value.as_val() {
                node.border.right = v;
            }
        }
        AnimatableProp::BorderWidthBottom => {
            if let Some(v) = value.as_val() {
                node.border.bottom = v;
            }
        }
        AnimatableProp::BorderWidthLeft => {
            if let Some(v) = value.as_val() {
                node.border.left = v;
            }
        }
        AnimatableProp::BorderRadiusTopLeft => {
            if let Some(v) = value.as_val() {
                node.border_radius.top_left = v;
            }
        }
        AnimatableProp::BorderRadiusTopRight => {
            if let Some(v) = value.as_val() {
                node.border_radius.top_right = v;
            }
        }
        AnimatableProp::BorderRadiusBottomRight => {
            if let Some(v) = value.as_val() {
                node.border_radius.bottom_right = v;
            }
        }
        AnimatableProp::BorderRadiusBottomLeft => {
            if let Some(v) = value.as_val() {
                node.border_radius.bottom_left = v;
            }
        }
        AnimatableProp::Left => {
            if let Some(v) = value.as_val() {
                node.left = v;
            }
        }
        AnimatableProp::Right => {
            if let Some(v) = value.as_val() {
                node.right = v;
            }
        }
        AnimatableProp::Top => {
            if let Some(v) = value.as_val() {
                node.top = v;
            }
        }
        AnimatableProp::Bottom => {
            if let Some(v) = value.as_val() {
                node.bottom = v;
            }
        }
        AnimatableProp::FlexBasis => {
            if let Some(v) = value.as_val() {
                node.flex_basis = v;
            }
        }
        AnimatableProp::ColumnGap | AnimatableProp::Gap => {
            if let Some(v) = value.as_val() {
                node.column_gap = v;
            }
        }
        AnimatableProp::RowGap => {
            if let Some(v) = value.as_val() {
                node.row_gap = v;
            }
        }

        // f32 fields → Node
        AnimatableProp::FlexGrow => {
            if let Some(v) = value.as_f32() {
                node.flex_grow = v;
            }
        }
        AnimatableProp::FlexShrink => {
            if let Some(v) = value.as_f32() {
                node.flex_shrink = v;
            }
        }

        // Color fields
        AnimatableProp::BackgroundColor => {
            if let Some(c) = value.as_color() {
                *bg = BackgroundColor(c);
            }
        }
        AnimatableProp::BorderColor => {
            if let Some(c) = value.as_color() {
                *border_color = BorderColor::all(c);
            }
        }

        // 2D transforms → UiTransform
        AnimatableProp::TranslateX => {
            if let Some(v) = value.as_f32() {
                ui_transform.translation.x = Val::Px(v);
            }
        }
        AnimatableProp::TranslateY => {
            if let Some(v) = value.as_f32() {
                ui_transform.translation.y = Val::Px(v);
            }
        }
        AnimatableProp::Rotate => {
            if let Some(v) = value.as_f32() {
                ui_transform.rotation = Rot2::radians(v.to_radians());
            }
        }
        AnimatableProp::ScaleX => {
            if let Some(v) = value.as_f32() {
                ui_transform.scale.x = v;
            }
        }
        AnimatableProp::ScaleY => {
            if let Some(v) = value.as_f32() {
                ui_transform.scale.y = v;
            }
        }

        // Box shadow
        AnimatableProp::BoxShadow => {
            if let Some(shadows) = value.as_shadow_list() {
                box_shadow.0 = shadows.iter().map(|s| s.to_shadow_style()).collect();
            }
        }

        // Properties not applicable to views
        _ => {}
    }
}

// ===========================================================================
// TTY transitions
// ===========================================================================

/// Combined extract + start for TTY transitions.
/// Reads Changed<TtyProps>, builds TransitionConfig, starts ActiveTransitions.
#[allow(clippy::type_complexity)]
pub fn handle_tty_transitions(
    mut query: Query<
        (
            Entity,
            &TtyProps,
            &BackgroundColor,
            &BorderColor,
            &BoxShadow,
            &mut TransitionConfig,
            &mut ActiveTransitions,
        ),
        Changed<TtyProps>,
    >,
    new_ttys: Query<Entity, Added<TtyProps>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let mut started_new = false;

    for (entity, props, bg, border_color, box_shadow, mut config, mut active) in &mut query {
        // Resolve TW classes into a temporary ViewProps for transition config + targets
        let mut resolved = ViewProps::default();
        if let Some(ref class) = props.class {
            tailwind::apply_classes(&mut resolved, class);
        }
        // Merge wire-level shadow/transition props
        if props.box_shadow.is_some() {
            resolved.box_shadow.clone_from(&props.box_shadow);
        }
        if props.transition.is_some() {
            resolved.transition.clone_from(&props.transition);
        }

        // Build transition config
        *config = build_transition_config(
            &resolved.transition,
            &resolved.tw_transition_property,
            resolved.tw_transition_duration,
            resolved.tw_transition_easing,
            resolved.tw_transition_delay,
        );

        // Skip first appearance
        if new_ttys.contains(entity) {
            continue;
        }

        if config.rules.is_empty() {
            active.transitions.clear();
            continue;
        }

        let mut transitions = std::mem::take(&mut active.transitions);
        let count_before = transitions.len();

        // --- Color fields ---
        check_color(
            &mut transitions,
            &config,
            AnimatableProp::BackgroundColor,
            bg.0,
            resolved.background_color.as_ref().map(|c| c.0),
        );
        check_color(
            &mut transitions,
            &config,
            AnimatableProp::BorderColor,
            border_color.top,
            resolved.border_color.as_ref().map(|c| c.0),
        );

        // --- BoxShadow ---
        check_shadow_list(
            &mut transitions,
            &config,
            &box_shadow.0,
            &style::resolve_box_shadow(&resolved),
        );

        if transitions.len() > count_before {
            started_new = true;
            for t in &transitions[count_before..] {
                match &t.state {
                    TransitionState::Eased {
                        from,
                        to,
                        duration_secs,
                        ..
                    } => {
                        debug!(
                            "tty transition started: {:?} from {:?} to {:?} over {:.3}s",
                            t.prop, from, to, duration_secs
                        );
                    }
                    TransitionState::PhysicsSpring {
                        current,
                        target,
                        stiffness,
                        damping,
                        ..
                    } => {
                        debug!(
                            "tty physics spring started: {:?} from {:.3} to {:.3} (k={}, d={})",
                            t.prop, current, target, stiffness, damping
                        );
                    }
                }
            }
        }

        active.transitions = transitions;
    }

    if started_new {
        redraw.write(RequestRedraw);
    }
}

/// Advance TTY transitions and write interpolated values.
#[allow(clippy::type_complexity)]
pub fn tick_tty_transitions(
    mut query: Query<
        (
            &mut ActiveTransitions,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut BoxShadow,
        ),
        With<ByoTty>,
    >,
    time: Res<Time>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let raw_delta = time.delta_secs();
    let delta = raw_delta.min(MAX_TICK_DELTA);
    let mut needs_redraw = false;

    for (mut active, mut bg, mut border_color, mut box_shadow) in &mut query {
        if active.transitions.is_empty() {
            continue;
        }

        if raw_delta > 0.1 {
            debug!("tick_tty: capped delta {:.3}s → {:.3}s", raw_delta, delta);
        }

        active.transitions.retain_mut(|t| {
            t.tick(delta);
            let value = t.current_value();

            write_tty_value(t.prop, &value, &mut bg, &mut border_color, &mut box_shadow);

            if t.is_complete() {
                let final_val = t.final_value();
                write_tty_value(
                    t.prop,
                    &final_val,
                    &mut bg,
                    &mut border_color,
                    &mut box_shadow,
                );
                debug!("tty transition complete: {:?}", t.prop);
                false
            } else {
                true
            }
        });

        if !active.transitions.is_empty() {
            needs_redraw = true;
        }
    }

    if needs_redraw {
        redraw.write(RequestRedraw);
    }
}

/// Write a single animated value onto TTY Bevy components.
fn write_tty_value(
    prop: AnimatableProp,
    value: &AnimatableValue,
    bg: &mut BackgroundColor,
    border_color: &mut BorderColor,
    box_shadow: &mut BoxShadow,
) {
    match prop {
        AnimatableProp::BackgroundColor => {
            if let Some(c) = value.as_color() {
                *bg = BackgroundColor(c);
            }
        }
        AnimatableProp::BorderColor => {
            if let Some(c) = value.as_color() {
                *border_color = BorderColor::all(c);
            }
        }
        AnimatableProp::BoxShadow => {
            if let Some(shadows) = value.as_shadow_list() {
                box_shadow.0 = shadows.iter().map(|s| s.to_shadow_style()).collect();
            }
        }
        _ => {}
    }
}
