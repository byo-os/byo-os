//! Runtime animation state for active transitions.

use bevy::prelude::*;

use crate::props::types::ByoShadow;

use super::config::{AnimatableProp, EaseFn, TRANSFORM_3D_PROPS};

/// A value that can be interpolated during a transition.
#[derive(Debug, Clone)]
pub enum AnimatableValue {
    Val(Val),
    F32(f32),
    Color(Color),
    ShadowList(Vec<ByoShadow>),
}

impl AnimatableValue {
    /// Interpolate between two values at parameter `t` in [0, 1].
    pub fn interpolate(&self, to: &AnimatableValue, t: f32) -> AnimatableValue {
        match (self, to) {
            (AnimatableValue::F32(a), AnimatableValue::F32(b)) => {
                AnimatableValue::F32(a + (b - a) * t)
            }
            (AnimatableValue::Color(a), AnimatableValue::Color(b)) => {
                AnimatableValue::Color(lerp_color(*a, *b, t))
            }
            (AnimatableValue::Val(a), AnimatableValue::Val(b)) => {
                AnimatableValue::Val(lerp_val(*a, *b, t))
            }
            (AnimatableValue::ShadowList(a), AnimatableValue::ShadowList(b)) => {
                AnimatableValue::ShadowList(interpolate_shadow_lists(a, b, t))
            }
            // Mismatched types: snap to target
            _ => to.clone(),
        }
    }

    /// Extract f32 value, returning None for other types.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            AnimatableValue::F32(v) => Some(*v),
            _ => None,
        }
    }

    /// Extract Color value, returning None for other types.
    pub fn as_color(&self) -> Option<Color> {
        match self {
            AnimatableValue::Color(c) => Some(*c),
            _ => None,
        }
    }

    /// Extract Val value, returning None for other types.
    pub fn as_val(&self) -> Option<Val> {
        match self {
            AnimatableValue::Val(v) => Some(*v),
            _ => None,
        }
    }

    /// Extract shadow list, returning None for other types.
    pub fn as_shadow_list(&self) -> Option<&Vec<ByoShadow>> {
        match self {
            AnimatableValue::ShadowList(v) => Some(v),
            _ => None,
        }
    }
}

/// The internal animation state of a transition.
#[derive(Debug, Clone)]
pub enum TransitionState {
    /// Standard timed transition: interpolates from→to over a fixed duration.
    Eased {
        from: AnimatableValue,
        to: AnimatableValue,
        duration_secs: f32,
        delay_secs: f32,
        easing: EaseFn,
        elapsed: f32,
    },
    /// Velocity-based damped spring (ODE integration per frame).
    /// Supports retargeting: when target changes mid-animation, velocity is preserved.
    PhysicsSpring {
        current: f32,
        velocity: f32,
        target: f32,
        stiffness: f32,
        damping: f32,
    },
}

/// A single active transition being animated.
#[derive(Debug, Clone)]
pub struct ActiveTransition {
    pub prop: AnimatableProp,
    pub state: TransitionState,
}

impl ActiveTransition {
    /// Advance the animation by `delta` seconds.
    pub fn tick(&mut self, delta: f32) {
        match &mut self.state {
            TransitionState::Eased { elapsed, .. } => {
                *elapsed += delta;
            }
            TransitionState::PhysicsSpring {
                current,
                velocity,
                target,
                stiffness,
                damping,
            } => {
                // Semi-implicit Euler (symplectic — stable and simple)
                let accel = -*stiffness * (*current - *target) - *damping * *velocity;
                *velocity += accel * delta;
                *current += *velocity * delta;
            }
        }
    }

    /// Compute the current interpolated value.
    pub fn current_value(&self) -> AnimatableValue {
        match &self.state {
            TransitionState::Eased {
                from,
                to,
                duration_secs,
                delay_secs,
                easing,
                elapsed,
            } => {
                let progress =
                    ((elapsed - delay_secs) / duration_secs.max(f32::EPSILON)).clamp(0.0, 1.0);
                if progress <= 0.0 {
                    return from.clone();
                }
                let eased = easing.sample(progress);
                from.interpolate(to, eased)
            }
            TransitionState::PhysicsSpring { current, .. } => AnimatableValue::F32(*current),
        }
    }

    /// The final/target value this transition is animating towards.
    pub fn final_value(&self) -> AnimatableValue {
        match &self.state {
            TransitionState::Eased { to, .. } => to.clone(),
            TransitionState::PhysicsSpring { target, .. } => AnimatableValue::F32(*target),
        }
    }

    /// Whether this transition has completed.
    pub fn is_complete(&self) -> bool {
        match &self.state {
            TransitionState::Eased {
                duration_secs,
                delay_secs,
                elapsed,
                ..
            } => *elapsed >= *delay_secs + *duration_secs,
            TransitionState::PhysicsSpring {
                current,
                velocity,
                target,
                ..
            } => (*current - *target).abs() < 0.001 && velocity.abs() < 0.01,
        }
    }
}

/// Component tracking all active transitions on an entity.
#[derive(Component, Debug, Default)]
pub struct ActiveTransitions {
    pub transitions: Vec<ActiveTransition>,
}

impl ActiveTransitions {
    /// Check if a specific property has an active transition.
    pub fn has(&self, prop: AnimatableProp) -> bool {
        self.transitions.iter().any(|t| t.prop == prop)
    }

    /// Check if any 3D transform property has an active transition.
    pub fn has_any_3d_transform(&self) -> bool {
        self.transitions
            .iter()
            .any(|t| TRANSFORM_3D_PROPS.contains(&t.prop))
    }

    /// Get the current interpolated f32 value for a property, if transitioning.
    #[allow(dead_code)]
    pub fn get_f32(&self, prop: AnimatableProp) -> Option<f32> {
        self.transitions
            .iter()
            .find(|t| t.prop == prop)
            .and_then(|t| t.current_value().as_f32())
    }

    /// Get the current interpolated Color value for a property, if transitioning.
    #[allow(dead_code)]
    pub fn get_color(&self, prop: AnimatableProp) -> Option<Color> {
        self.transitions
            .iter()
            .find(|t| t.prop == prop)
            .and_then(|t| t.current_value().as_color())
    }

    /// Get the current interpolated Val for a property, if transitioning.
    #[allow(dead_code)]
    pub fn get_val(&self, prop: AnimatableProp) -> Option<Val> {
        self.transitions
            .iter()
            .find(|t| t.prop == prop)
            .and_then(|t| t.current_value().as_val())
    }
}

// ---------------------------------------------------------------------------
// Interpolation helpers
// ---------------------------------------------------------------------------

/// Linearly interpolate between two `Val` values.
/// Only same-variant Vals (Px↔Px, Percent↔Percent, etc.) interpolate smoothly.
/// Mismatched variants snap to `b` (matches CSS behavior).
fn lerp_val(a: Val, b: Val, t: f32) -> Val {
    match (a, b) {
        (Val::Px(a), Val::Px(b)) => Val::Px(a + (b - a) * t),
        (Val::Percent(a), Val::Percent(b)) => Val::Percent(a + (b - a) * t),
        (Val::Vw(a), Val::Vw(b)) => Val::Vw(a + (b - a) * t),
        (Val::Vh(a), Val::Vh(b)) => Val::Vh(a + (b - a) * t),
        (Val::VMin(a), Val::VMin(b)) => Val::VMin(a + (b - a) * t),
        (Val::VMax(a), Val::VMax(b)) => Val::VMax(a + (b - a) * t),
        // Mismatched variants: snap to target
        (_, b) => b,
    }
}

/// Linearly interpolate between two colors in sRGB space.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a = a.to_srgba();
    let b = b.to_srgba();
    Color::srgba(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    )
}

/// Interpolate two shadow lists per CSS spec: pad shorter list with transparent zeros,
/// then interpolate each pair 1:1.
fn interpolate_shadow_lists(a: &[ByoShadow], b: &[ByoShadow], t: f32) -> Vec<ByoShadow> {
    let max_len = a.len().max(b.len());
    (0..max_len)
        .map(|i| {
            let sa = a.get(i).unwrap_or(&ByoShadow::TRANSPARENT_ZERO);
            let sb = b.get(i).unwrap_or(&ByoShadow::TRANSPARENT_ZERO);
            sa.interpolate(sb, t)
        })
        .collect()
}

/// Approximate equality for `Val` values (avoids spurious transitions).
pub fn val_approx_eq(a: Val, b: Val) -> bool {
    match (a, b) {
        (Val::Auto, Val::Auto) => true,
        (Val::Px(a), Val::Px(b)) => (a - b).abs() < 0.01,
        (Val::Percent(a), Val::Percent(b)) => (a - b).abs() < 0.01,
        (Val::Vw(a), Val::Vw(b)) => (a - b).abs() < 0.01,
        (Val::Vh(a), Val::Vh(b)) => (a - b).abs() < 0.01,
        (Val::VMin(a), Val::VMin(b)) => (a - b).abs() < 0.01,
        (Val::VMax(a), Val::VMax(b)) => (a - b).abs() < 0.01,
        _ => false,
    }
}

/// Approximate equality for `Color` values.
pub fn color_approx_eq(a: Color, b: Color) -> bool {
    let a = a.to_srgba();
    let b = b.to_srgba();
    (a.red - b.red).abs() < 0.005
        && (a.green - b.green).abs() < 0.005
        && (a.blue - b.blue).abs() < 0.005
        && (a.alpha - b.alpha).abs() < 0.005
}

/// Insert or update a timed (eased) transition in the list (upsert by prop).
pub fn upsert_transition(
    transitions: &mut Vec<ActiveTransition>,
    prop: AnimatableProp,
    from: AnimatableValue,
    to: AnimatableValue,
    duration_secs: f32,
    delay_secs: f32,
    easing: EaseFn,
) {
    let t = ActiveTransition {
        prop,
        state: TransitionState::Eased {
            from,
            to,
            duration_secs,
            delay_secs,
            easing,
            elapsed: 0.0,
        },
    };
    if let Some(existing) = transitions.iter_mut().find(|e| e.prop == prop) {
        *existing = t;
    } else {
        transitions.push(t);
    }
}

/// Insert or retarget a physics spring transition (upsert by prop).
///
/// If an existing physics spring exists for this prop, only the target is updated
/// (velocity and current position are preserved — this is the key retargeting behavior).
/// If an eased transition exists, it is replaced with a new spring.
pub fn upsert_physics_spring(
    transitions: &mut Vec<ActiveTransition>,
    prop: AnimatableProp,
    current_bevy: f32,
    target: f32,
    stiffness: f32,
    damping: f32,
) {
    if let Some(existing) = transitions.iter_mut().find(|e| e.prop == prop) {
        match &mut existing.state {
            TransitionState::PhysicsSpring {
                target: t,
                stiffness: k,
                damping: d,
                ..
            } => {
                // Retarget: preserve velocity and current position
                *t = target;
                *k = stiffness;
                *d = damping;
            }
            _ => {
                // Replace eased transition with new spring
                existing.state = TransitionState::PhysicsSpring {
                    current: current_bevy,
                    velocity: 0.0,
                    target,
                    stiffness,
                    damping,
                };
            }
        }
    } else {
        transitions.push(ActiveTransition {
            prop,
            state: TransitionState::PhysicsSpring {
                current: current_bevy,
                velocity: 0.0,
                target,
                stiffness,
                damping,
            },
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_f32() {
        let a = AnimatableValue::F32(0.0);
        let b = AnimatableValue::F32(100.0);
        let mid = a.interpolate(&b, 0.5);
        assert_eq!(mid.as_f32(), Some(50.0));
    }

    #[test]
    fn interpolate_val_px() {
        let a = AnimatableValue::Val(Val::Px(0.0));
        let b = AnimatableValue::Val(Val::Px(200.0));
        let mid = a.interpolate(&b, 0.25);
        assert_eq!(mid.as_val(), Some(Val::Px(50.0)));
    }

    #[test]
    fn interpolate_val_percent() {
        let a = AnimatableValue::Val(Val::Percent(0.0));
        let b = AnimatableValue::Val(Val::Percent(100.0));
        let mid = a.interpolate(&b, 0.5);
        assert_eq!(mid.as_val(), Some(Val::Percent(50.0)));
    }

    #[test]
    fn interpolate_val_mismatch_snaps() {
        let a = AnimatableValue::Val(Val::Px(100.0));
        let b = AnimatableValue::Val(Val::Percent(50.0));
        let result = a.interpolate(&b, 0.5);
        assert_eq!(result.as_val(), Some(Val::Percent(50.0)));
    }

    #[test]
    fn interpolate_color() {
        let a = AnimatableValue::Color(Color::srgba(0.0, 0.0, 0.0, 1.0));
        let b = AnimatableValue::Color(Color::srgba(1.0, 1.0, 1.0, 1.0));
        let mid = a.interpolate(&b, 0.5);
        let c = mid.as_color().unwrap().to_srgba();
        assert!((c.red - 0.5).abs() < 0.01);
        assert!((c.green - 0.5).abs() < 0.01);
        assert!((c.blue - 0.5).abs() < 0.01);
    }

    #[test]
    fn val_approx_eq_same() {
        assert!(val_approx_eq(Val::Px(100.0), Val::Px(100.001)));
        assert!(!val_approx_eq(Val::Px(100.0), Val::Px(200.0)));
    }

    #[test]
    fn val_approx_eq_different_types() {
        assert!(!val_approx_eq(Val::Px(100.0), Val::Percent(100.0)));
    }

    #[test]
    fn active_transition_delay() {
        let t = ActiveTransition {
            prop: AnimatableProp::Width,
            state: TransitionState::Eased {
                from: AnimatableValue::F32(0.0),
                to: AnimatableValue::F32(100.0),
                duration_secs: 1.0,
                delay_secs: 0.5,
                easing: EaseFn::Linear,
                elapsed: 0.3, // still in delay
            },
        };
        assert_eq!(t.current_value().as_f32(), Some(0.0));
    }

    #[test]
    fn active_transition_midway() {
        let t = ActiveTransition {
            prop: AnimatableProp::Width,
            state: TransitionState::Eased {
                from: AnimatableValue::F32(0.0),
                to: AnimatableValue::F32(100.0),
                duration_secs: 1.0,
                delay_secs: 0.0,
                easing: EaseFn::Linear,
                elapsed: 0.5,
            },
        };
        let v = t.current_value().as_f32().unwrap();
        assert!((v - 50.0).abs() < 0.01);
    }

    #[test]
    fn upsert_replaces_existing() {
        let mut transitions = Vec::new();
        upsert_transition(
            &mut transitions,
            AnimatableProp::Width,
            AnimatableValue::F32(0.0),
            AnimatableValue::F32(100.0),
            1.0,
            0.0,
            EaseFn::Linear,
        );
        upsert_transition(
            &mut transitions,
            AnimatableProp::Width,
            AnimatableValue::F32(50.0),
            AnimatableValue::F32(200.0),
            0.5,
            0.0,
            EaseFn::CubicOut,
        );
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].final_value().as_f32(), Some(200.0));
    }

    // --- Physics spring tests ---

    #[test]
    fn physics_spring_current_value() {
        let t = ActiveTransition {
            prop: AnimatableProp::TranslateX,
            state: TransitionState::PhysicsSpring {
                current: 50.0,
                velocity: 10.0,
                target: 100.0,
                stiffness: 100.0,
                damping: 12.0,
            },
        };
        assert_eq!(t.current_value().as_f32(), Some(50.0));
        assert_eq!(t.final_value().as_f32(), Some(100.0));
        assert!(!t.is_complete());
    }

    #[test]
    fn physics_spring_settled() {
        let t = ActiveTransition {
            prop: AnimatableProp::TranslateX,
            state: TransitionState::PhysicsSpring {
                current: 100.0005,
                velocity: 0.005,
                target: 100.0,
                stiffness: 100.0,
                damping: 12.0,
            },
        };
        assert!(t.is_complete());
    }

    #[test]
    fn physics_spring_tick_moves_towards_target() {
        let mut t = ActiveTransition {
            prop: AnimatableProp::TranslateX,
            state: TransitionState::PhysicsSpring {
                current: 0.0,
                velocity: 0.0,
                target: 100.0,
                stiffness: 100.0,
                damping: 12.0,
            },
        };
        // Tick a few frames
        for _ in 0..100 {
            t.tick(1.0 / 60.0);
        }
        let v = t.current_value().as_f32().unwrap();
        // Should be close to target after ~1.67s of simulation
        assert!(
            (v - 100.0).abs() < 5.0,
            "spring should approach target, got {v}"
        );
    }

    #[test]
    fn physics_spring_retarget_preserves_velocity() {
        let mut transitions = Vec::new();
        upsert_physics_spring(
            &mut transitions,
            AnimatableProp::TranslateX,
            0.0,
            100.0,
            100.0,
            12.0,
        );
        assert_eq!(transitions.len(), 1);

        // Simulate a few ticks to build velocity
        for _ in 0..10 {
            transitions[0].tick(1.0 / 60.0);
        }
        let TransitionState::PhysicsSpring {
            velocity: vel_before,
            current: pos_before,
            ..
        } = transitions[0].state
        else {
            panic!()
        };
        assert!(
            vel_before.abs() > 0.1,
            "should have velocity, got {vel_before}"
        );

        // Retarget to a new position
        upsert_physics_spring(
            &mut transitions,
            AnimatableProp::TranslateX,
            pos_before,
            200.0,
            100.0,
            12.0,
        );
        assert_eq!(transitions.len(), 1);

        let TransitionState::PhysicsSpring {
            velocity: vel_after,
            current: pos_after,
            target,
            ..
        } = transitions[0].state
        else {
            panic!()
        };
        // Velocity and position should be preserved
        assert_eq!(vel_after, vel_before);
        assert_eq!(pos_after, pos_before);
        // Target should be updated
        assert_eq!(target, 200.0);
    }

    #[test]
    fn upsert_physics_spring_new() {
        let mut transitions = Vec::new();
        upsert_physics_spring(
            &mut transitions,
            AnimatableProp::TranslateX,
            0.0,
            100.0,
            100.0,
            12.0,
        );
        assert_eq!(transitions.len(), 1);
        let TransitionState::PhysicsSpring {
            current,
            velocity,
            target,
            ..
        } = transitions[0].state
        else {
            panic!()
        };
        assert_eq!(current, 0.0);
        assert_eq!(velocity, 0.0);
        assert_eq!(target, 100.0);
    }

    // --- Shadow list interpolation tests ---

    #[test]
    fn interpolate_shadow_list_equal_length() {
        let a = vec![ByoShadow {
            color: Color::srgba(0.0, 0.0, 0.0, 0.1),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(0.0),
            blur_radius: Val::Px(0.0),
            spread_radius: Val::Px(0.0),
            inset: false,
        }];
        let b = vec![ByoShadow {
            color: Color::srgba(0.0, 0.0, 0.0, 0.1),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(10.0),
            blur_radius: Val::Px(20.0),
            spread_radius: Val::Px(0.0),
            inset: false,
        }];
        let va = AnimatableValue::ShadowList(a);
        let vb = AnimatableValue::ShadowList(b);
        let mid = va.interpolate(&vb, 0.5);
        let list = mid.as_shadow_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].y_offset, Val::Px(5.0));
        assert_eq!(list[0].blur_radius, Val::Px(10.0));
    }

    #[test]
    fn interpolate_shadow_list_unequal_length() {
        let a = vec![ByoShadow {
            color: Color::srgba(0.0, 0.0, 0.0, 0.5),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(4.0),
            blur_radius: Val::Px(6.0),
            spread_radius: Val::Px(0.0),
            inset: false,
        }];
        let b = vec![
            ByoShadow {
                color: Color::srgba(0.0, 0.0, 0.0, 0.5),
                x_offset: Val::Px(0.0),
                y_offset: Val::Px(4.0),
                blur_radius: Val::Px(6.0),
                spread_radius: Val::Px(0.0),
                inset: false,
            },
            ByoShadow {
                color: Color::srgba(1.0, 0.0, 0.0, 1.0),
                x_offset: Val::Px(0.0),
                y_offset: Val::Px(10.0),
                blur_radius: Val::Px(20.0),
                spread_radius: Val::Px(0.0),
                inset: false,
            },
        ];
        let va = AnimatableValue::ShadowList(a);
        let vb = AnimatableValue::ShadowList(b);
        let mid = va.interpolate(&vb, 1.0);
        let list = mid.as_shadow_list().unwrap();
        assert_eq!(list.len(), 2);
        // Second shadow should be fully the target
        assert_eq!(list[1].y_offset, Val::Px(10.0));
    }

    #[test]
    fn interpolate_shadow_list_transparent_fadein() {
        // From empty list to a single shadow
        let a: Vec<ByoShadow> = vec![];
        let b = vec![ByoShadow {
            color: Color::srgba(0.0, 0.0, 0.0, 0.5),
            x_offset: Val::Px(0.0),
            y_offset: Val::Px(10.0),
            blur_radius: Val::Px(20.0),
            spread_radius: Val::Px(0.0),
            inset: false,
        }];
        let va = AnimatableValue::ShadowList(a);
        let vb = AnimatableValue::ShadowList(b);
        let mid = va.interpolate(&vb, 0.5);
        let list = mid.as_shadow_list().unwrap();
        assert_eq!(list.len(), 1);
        // y_offset should be halfway
        assert_eq!(list[0].y_offset, Val::Px(5.0));
        // alpha should be halfway from 0 to 0.5
        let c = list[0].color.to_srgba();
        assert!((c.alpha - 0.25).abs() < 0.02);
    }

    #[test]
    fn upsert_physics_spring_replaces_eased() {
        let mut transitions = Vec::new();
        upsert_transition(
            &mut transitions,
            AnimatableProp::TranslateX,
            AnimatableValue::F32(0.0),
            AnimatableValue::F32(50.0),
            0.3,
            0.0,
            EaseFn::Linear,
        );
        // Now replace with physics spring
        upsert_physics_spring(
            &mut transitions,
            AnimatableProp::TranslateX,
            25.0,
            100.0,
            100.0,
            12.0,
        );
        assert_eq!(transitions.len(), 1);
        assert!(matches!(
            transitions[0].state,
            TransitionState::PhysicsSpring { .. }
        ));
    }
}
