//! Transition configuration types and CSS transition shorthand parsing.

use bevy::prelude::*;

/// Identifies a specific animatable property on a BYO entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum AnimatableProp {
    // View layout (Val)
    Width,
    Height,
    MinWidth,
    MaxWidth,
    MinHeight,
    MaxHeight,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    BorderWidthTop,
    BorderWidthRight,
    BorderWidthBottom,
    BorderWidthLeft,
    BorderRadiusTopLeft,
    BorderRadiusTopRight,
    BorderRadiusBottomRight,
    BorderRadiusBottomLeft,
    Gap,
    ColumnGap,
    RowGap,
    Left,
    Right,
    Top,
    Bottom,
    FlexBasis,
    // View colors (Color)
    BackgroundColor,
    BorderColor,
    // View f32
    FlexGrow,
    FlexShrink,
    Opacity,
    // 2D transforms (f32)
    TranslateX,
    TranslateY,
    Rotate,
    Scale,
    ScaleX,
    ScaleY,
    // 3D transforms (f32)
    TranslateZ,
    RotateX,
    RotateY,
    RotateZ,
    ScaleZ,
    // Layer PBR colors
    BaseColor,
    EmissiveColor,
    AttenuationColor,
    // Layer PBR f32
    PerceptualRoughness,
    Metallic,
    Reflectance,
    EmissiveExposureWeight,
    DepthBias,
    DiffuseTransmission,
    SpecularTransmission,
    Ior,
    Thickness,
    AttenuationDistance,
    Clearcoat,
    ClearcoatPerceptualRoughness,
    AnisotropyStrength,
    AnisotropyRotation,
    // Text
    FontSize,
    TextColor,
    LineHeight,
}

/// The 3D transform AnimatableProp variants.
pub const TRANSFORM_3D_PROPS: &[AnimatableProp] = &[
    AnimatableProp::TranslateX,
    AnimatableProp::TranslateY,
    AnimatableProp::TranslateZ,
    AnimatableProp::Rotate,
    AnimatableProp::RotateX,
    AnimatableProp::RotateY,
    AnimatableProp::RotateZ,
    AnimatableProp::Scale,
    AnimatableProp::ScaleX,
    AnimatableProp::ScaleY,
    AnimatableProp::ScaleZ,
];

/// Specifies which properties a transition rule applies to.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionProperty {
    /// Applies to all animatable properties.
    All,
    /// Applies to specific properties.
    Properties(Vec<AnimatableProp>),
}

/// Easing function for transitions.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EaseFn {
    #[default]
    Linear,
    /// CSS "ease" approximation (smooth S-curve).
    SmoothStep,
    /// CSS "ease-in" (slow start).
    CubicIn,
    /// CSS "ease-out" (slow end).
    CubicOut,
    /// CSS "ease-in-out" (slow start and end).
    CubicInOut,
    /// Analytical damped spring timing curve.
    /// Duration is derived from the spring parameters (not user-specified).
    /// Can overshoot 1.0 for underdamped (bouncy) springs.
    Spring {
        omega0: f32,
        zeta: f32,
        settle_duration: f32,
    },
}

impl EaseFn {
    /// Create a spring easing from stiffness and damping (mass = 1).
    ///
    /// - `stiffness`: spring constant (higher = faster oscillation)
    /// - `damping`: damping coefficient (higher = less bounce)
    ///
    /// Damping ratio ζ = damping / (2√stiffness):
    /// - ζ < 1: underdamped (bouncy, overshoots target)
    /// - ζ = 1: critically damped (fastest settle, no bounce)
    /// - ζ > 1: overdamped (slow settle, no bounce)
    pub fn spring(stiffness: f32, damping: f32) -> Self {
        let stiffness = stiffness.max(0.1);
        let omega0 = stiffness.sqrt();
        let zeta = (damping / (2.0 * omega0)).max(0.01);
        let settle_duration = compute_settle_time(omega0, zeta);
        Self::Spring {
            omega0,
            zeta,
            settle_duration,
        }
    }

    /// Returns the natural duration for easing functions that compute their own
    /// (e.g. spring). Returns `None` for standard easings that use the
    /// user-specified duration.
    pub fn natural_duration(&self) -> Option<f32> {
        match self {
            Self::Spring {
                settle_duration, ..
            } => Some(*settle_duration),
            _ => None,
        }
    }

    /// Sample the easing function at time `t` in [0, 1].
    /// For spring easings, the output may overshoot 1.0 (underdamped bounce).
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::SmoothStep => t * t * (3.0 - 2.0 * t),
            Self::CubicIn => t * t * t,
            Self::CubicOut => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Self::CubicInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - u * u * u / 2.0
                }
            }
            Self::Spring {
                omega0,
                zeta,
                settle_duration,
            } => spring_eval(omega0, zeta, t * settle_duration),
        }
    }
}

// ---------------------------------------------------------------------------
// Analytical spring math
// ---------------------------------------------------------------------------

/// Evaluate a damped spring (mass=1) at real time `t`.
/// Returns position starting from 0 and settling to 1.
///
/// Solution to: x'' + 2ζω₀x' + ω₀²x = ω₀² with x(0)=0, x'(0)=0.
fn spring_eval(omega0: f32, zeta: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }

    if zeta < 1.0 - 1e-5 {
        // Underdamped: oscillates while decaying
        let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega0 * t).exp();
        let cos_term = (omega_d * t).cos();
        let sin_term = (zeta / (1.0 - zeta * zeta).sqrt()) * (omega_d * t).sin();
        1.0 - decay * (cos_term + sin_term)
    } else if zeta > 1.0 + 1e-5 {
        // Overdamped: slow exponential approach, no oscillation
        let s = (zeta * zeta - 1.0).sqrt();
        let alpha = omega0 * s;
        let decay = (-zeta * omega0 * t).exp();
        1.0 - decay * ((alpha * t).cosh() + (zeta / s) * (alpha * t).sinh())
    } else {
        // Critically damped: fastest non-oscillating approach
        let decay = (-omega0 * t).exp();
        1.0 - decay * (1.0 + omega0 * t)
    }
}

/// Compute the time at which a spring has effectively settled (amplitude < 0.1%).
fn compute_settle_time(omega0: f32, zeta: f32) -> f32 {
    let threshold = 0.001;

    if zeta < 1.0 {
        // Underdamped: max envelope amplitude = 1/√(1-ζ²)
        // Settle when envelope × e^(-ζω₀t) < threshold
        let peak_amplitude = 1.0 / (1.0 - zeta * zeta).sqrt();
        (peak_amplitude / threshold).ln() / (zeta * omega0)
    } else if (zeta - 1.0).abs() < 0.01 {
        // Critically damped: (1+ω₀t)·e^(-ω₀t), conservative 1.5× estimate
        1.5 * (-threshold.ln()) / omega0
    } else {
        // Overdamped: slower eigenvalue ω₀(ζ-√(ζ²-1)) dominates
        let slower_rate = omega0 * (zeta - (zeta * zeta - 1.0).sqrt());
        1.5 * (-threshold.ln()) / slower_rate
    }
}

/// A single transition rule specifying property, duration, easing, and delay.
#[derive(Debug, Clone)]
pub struct TransitionRule {
    pub property: TransitionProperty,
    pub duration_secs: f32,
    pub easing: EaseFn,
    pub delay_secs: f32,
}

/// Configuration component storing all active transition rules for an entity.
#[derive(Component, Debug, Clone, Default)]
pub struct TransitionConfig {
    pub rules: Vec<TransitionRule>,
}

impl TransitionConfig {
    /// Find the first matching rule for a given property.
    pub fn find_rule(&self, prop: AnimatableProp) -> Option<&TransitionRule> {
        self.rules.iter().find(|r| match &r.property {
            TransitionProperty::All => true,
            TransitionProperty::Properties(props) => {
                props.contains(&prop)
                    || match prop {
                        // scale-x/y/z inherit from scale
                        AnimatableProp::ScaleX
                        | AnimatableProp::ScaleY
                        | AnimatableProp::ScaleZ => props.contains(&AnimatableProp::Scale),
                        _ => false,
                    }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// CSS transition shorthand parsing
// ---------------------------------------------------------------------------

/// Parse the CSS `transition` shorthand value.
/// Format: `"property duration [easing] [delay], ..."`
///
/// Also supports `"property spring(k,d) [delay]"` where the spring
/// determines its own duration from the parameters.
pub fn parse_transition_prop(value: &str) -> Vec<TransitionRule> {
    split_respecting_parens(value, ',')
        .iter()
        .filter_map(|part| {
            let tokens: Vec<&str> = part.split_whitespace().collect();
            if tokens.is_empty() {
                return None;
            }

            let property = parse_property_name(tokens[0])?;

            // Classify remaining tokens: easing vs time values.
            // This handles both `"all 300ms ease-in-out 100ms"` (standard)
            // and `"all spring(100,10) 100ms"` (spring replaces duration).
            let mut easing = None;
            let mut times = Vec::new();
            for token in &tokens[1..] {
                if easing.is_none() && parse_easing(token).is_some() {
                    easing = parse_easing(token);
                } else if let Some(d) = parse_duration(token) {
                    times.push(d);
                }
            }

            let easing = easing.unwrap_or(EaseFn::SmoothStep);
            // When the easing provides its own duration (e.g. spring), time values
            // shift: any specified time is delay, not duration.
            let (duration_secs, delay_secs) = if let Some(natural) = easing.natural_duration() {
                (natural, times.first().copied().unwrap_or(0.0))
            } else {
                (
                    times.first().copied().unwrap_or(0.0),
                    times.get(1).copied().unwrap_or(0.0),
                )
            };

            Some(TransitionRule {
                property,
                duration_secs,
                easing,
                delay_secs,
            })
        })
        .collect()
}

/// Split a string by `sep`, but not inside parentheses.
fn split_respecting_parens(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Parse a CSS duration string: `"300ms"` -> 0.3, `"0.3s"` -> 0.3.
pub fn parse_duration(s: &str) -> Option<f32> {
    if let Some(rest) = s.strip_suffix("ms") {
        rest.parse::<f32>().ok().map(|v| v / 1000.0)
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.parse::<f32>().ok()
    } else {
        // Bare number treated as ms
        s.parse::<f32>().ok().map(|v| v / 1000.0)
    }
}

/// Parse a CSS easing function name or `spring(stiffness,damping)`.
pub fn parse_easing(s: &str) -> Option<EaseFn> {
    Some(match s {
        "linear" => EaseFn::Linear,
        "ease" => EaseFn::SmoothStep,
        "ease-in" => EaseFn::CubicIn,
        "ease-out" => EaseFn::CubicOut,
        "ease-in-out" => EaseFn::CubicInOut,
        _ => {
            // spring(stiffness,damping)
            if let Some(inner) = s.strip_prefix("spring(").and_then(|r| r.strip_suffix(')')) {
                let (a, b) = inner.split_once(',')?;
                let stiffness = a.trim().parse::<f32>().ok()?;
                let damping = b.trim().parse::<f32>().ok()?;
                return Some(EaseFn::spring(stiffness, damping));
            }
            return None;
        }
    })
}

/// Parse a CSS transition property name into a [`TransitionProperty`].
pub fn parse_property_name(s: &str) -> Option<TransitionProperty> {
    if s == "all" {
        return Some(TransitionProperty::All);
    }
    if s == "transform" {
        return Some(TransitionProperty::Properties(TRANSFORM_3D_PROPS.to_vec()));
    }
    if s == "color" || s == "colors" {
        return Some(TransitionProperty::Properties(vec![
            AnimatableProp::BackgroundColor,
            AnimatableProp::BorderColor,
            AnimatableProp::TextColor,
        ]));
    }

    let prop = match s {
        "width" => AnimatableProp::Width,
        "height" => AnimatableProp::Height,
        "min-width" => AnimatableProp::MinWidth,
        "max-width" => AnimatableProp::MaxWidth,
        "min-height" => AnimatableProp::MinHeight,
        "max-height" => AnimatableProp::MaxHeight,
        "flex-basis" => AnimatableProp::FlexBasis,
        "flex-grow" => AnimatableProp::FlexGrow,
        "flex-shrink" => AnimatableProp::FlexShrink,
        "gap" => AnimatableProp::Gap,
        "column-gap" => AnimatableProp::ColumnGap,
        "row-gap" => AnimatableProp::RowGap,
        "left" => AnimatableProp::Left,
        "right" => AnimatableProp::Right,
        "top" => AnimatableProp::Top,
        "bottom" => AnimatableProp::Bottom,
        "opacity" => AnimatableProp::Opacity,
        "background-color" => AnimatableProp::BackgroundColor,
        "border-color" => AnimatableProp::BorderColor,
        "translate-x" => AnimatableProp::TranslateX,
        "translate-y" => AnimatableProp::TranslateY,
        "translate-z" => AnimatableProp::TranslateZ,
        "rotate" => AnimatableProp::Rotate,
        "rotate-x" => AnimatableProp::RotateX,
        "rotate-y" => AnimatableProp::RotateY,
        "rotate-z" => AnimatableProp::RotateZ,
        "scale" => AnimatableProp::Scale,
        "scale-x" => AnimatableProp::ScaleX,
        "scale-y" => AnimatableProp::ScaleY,
        "scale-z" => AnimatableProp::ScaleZ,
        "base-color" => AnimatableProp::BaseColor,
        "emissive-color" => AnimatableProp::EmissiveColor,
        "roughness" | "perceptual-roughness" => AnimatableProp::PerceptualRoughness,
        "metallic" => AnimatableProp::Metallic,
        "reflectance" => AnimatableProp::Reflectance,
        "font-size" => AnimatableProp::FontSize,
        "text-color" => AnimatableProp::TextColor,
        "line-height" => AnimatableProp::LineHeight,
        _ => return None,
    };

    Some(TransitionProperty::Properties(vec![prop]))
}

/// Default property list for Tailwind `transition` class.
pub fn tw_transition_default() -> TransitionProperty {
    TransitionProperty::Properties(vec![
        AnimatableProp::BackgroundColor,
        AnimatableProp::BorderColor,
        AnimatableProp::Opacity,
    ])
}

/// Property list for Tailwind `transition-colors` class.
pub fn tw_transition_colors() -> TransitionProperty {
    TransitionProperty::Properties(vec![
        AnimatableProp::BackgroundColor,
        AnimatableProp::BorderColor,
        AnimatableProp::TextColor,
    ])
}

/// Property list for Tailwind `transition-opacity` class.
pub fn tw_transition_opacity() -> TransitionProperty {
    TransitionProperty::Properties(vec![AnimatableProp::Opacity])
}

// ---------------------------------------------------------------------------
// Build TransitionConfig from resolved props
// ---------------------------------------------------------------------------

/// Build a TransitionConfig from resolved view/window/layer props.
/// The `transition` wire prop takes priority over Tailwind-derived values.
pub fn build_transition_config(
    transition_prop: &Option<String>,
    tw_property: &Option<TransitionProperty>,
    tw_duration: Option<f32>,
    tw_easing: Option<EaseFn>,
    tw_delay: Option<f32>,
) -> TransitionConfig {
    // Wire prop takes priority
    if let Some(transition_str) = transition_prop {
        let rules = parse_transition_prop(transition_str);
        if !rules.is_empty() {
            return TransitionConfig { rules };
        }
    }
    // Fall back to Tailwind-derived config
    let property = match tw_property {
        Some(p) => p.clone(),
        None => return TransitionConfig::default(),
    };
    let easing = tw_easing.unwrap_or(EaseFn::SmoothStep);
    // Spring easing overrides explicit duration with its natural settle time.
    let duration_secs = easing
        .natural_duration()
        .unwrap_or(tw_duration.unwrap_or(0.15));
    let delay_secs = tw_delay.unwrap_or(0.0);
    TransitionConfig {
        rules: vec![TransitionRule {
            property,
            duration_secs,
            easing,
            delay_secs,
        }],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_fn_linear() {
        assert_eq!(EaseFn::Linear.sample(0.0), 0.0);
        assert_eq!(EaseFn::Linear.sample(0.5), 0.5);
        assert_eq!(EaseFn::Linear.sample(1.0), 1.0);
    }

    #[test]
    fn ease_fn_smooth_step_endpoints() {
        assert!(EaseFn::SmoothStep.sample(0.0).abs() < 0.001);
        assert!((EaseFn::SmoothStep.sample(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn ease_fn_smooth_step_midpoint() {
        assert!((EaseFn::SmoothStep.sample(0.5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn ease_fn_cubic_in() {
        assert_eq!(EaseFn::CubicIn.sample(0.0), 0.0);
        assert!((EaseFn::CubicIn.sample(0.5) - 0.125).abs() < 0.001);
        assert_eq!(EaseFn::CubicIn.sample(1.0), 1.0);
    }

    #[test]
    fn ease_fn_cubic_out() {
        assert_eq!(EaseFn::CubicOut.sample(0.0), 0.0);
        assert!((EaseFn::CubicOut.sample(0.5) - 0.875).abs() < 0.001);
        assert_eq!(EaseFn::CubicOut.sample(1.0), 1.0);
    }

    #[test]
    fn ease_fn_cubic_in_out() {
        assert_eq!(EaseFn::CubicInOut.sample(0.0), 0.0);
        assert!((EaseFn::CubicInOut.sample(0.5) - 0.5).abs() < 0.001);
        assert_eq!(EaseFn::CubicInOut.sample(1.0), 1.0);
    }

    #[test]
    fn parse_duration_ms() {
        assert_eq!(parse_duration("300ms"), Some(0.3));
        assert_eq!(parse_duration("150ms"), Some(0.15));
    }

    #[test]
    fn parse_duration_s() {
        assert_eq!(parse_duration("0.3s"), Some(0.3));
        assert_eq!(parse_duration("1s"), Some(1.0));
    }

    #[test]
    fn parse_duration_bare() {
        assert_eq!(parse_duration("200"), Some(0.2));
    }

    #[test]
    fn parse_easing_names() {
        assert_eq!(parse_easing("linear"), Some(EaseFn::Linear));
        assert_eq!(parse_easing("ease"), Some(EaseFn::SmoothStep));
        assert_eq!(parse_easing("ease-in"), Some(EaseFn::CubicIn));
        assert_eq!(parse_easing("ease-out"), Some(EaseFn::CubicOut));
        assert_eq!(parse_easing("ease-in-out"), Some(EaseFn::CubicInOut));
        assert_eq!(parse_easing("unknown"), None);
    }

    #[test]
    fn parse_transition_single() {
        let rules = parse_transition_prop("background-color 300ms ease-in-out");
        assert_eq!(rules.len(), 1);
        assert!((rules[0].duration_secs - 0.3).abs() < 0.001);
        assert_eq!(rules[0].easing, EaseFn::CubicInOut);
        assert_eq!(rules[0].delay_secs, 0.0);
    }

    #[test]
    fn parse_transition_multi() {
        let rules =
            parse_transition_prop("background-color 300ms ease-in-out 150ms, opacity 150ms linear");
        assert_eq!(rules.len(), 2);
        assert!((rules[0].duration_secs - 0.3).abs() < 0.001);
        assert!((rules[0].delay_secs - 0.15).abs() < 0.001);
        assert!((rules[1].duration_secs - 0.15).abs() < 0.001);
        assert_eq!(rules[1].easing, EaseFn::Linear);
    }

    #[test]
    fn parse_transition_all() {
        let rules = parse_transition_prop("all 500ms ease");
        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].property, TransitionProperty::All));
    }

    #[test]
    fn find_rule_all() {
        let config = TransitionConfig {
            rules: vec![TransitionRule {
                property: TransitionProperty::All,
                duration_secs: 0.3,
                easing: EaseFn::Linear,
                delay_secs: 0.0,
            }],
        };
        assert!(config.find_rule(AnimatableProp::Width).is_some());
        assert!(config.find_rule(AnimatableProp::BackgroundColor).is_some());
    }

    #[test]
    fn find_rule_specific() {
        let config = TransitionConfig {
            rules: vec![TransitionRule {
                property: TransitionProperty::Properties(vec![AnimatableProp::BackgroundColor]),
                duration_secs: 0.3,
                easing: EaseFn::Linear,
                delay_secs: 0.0,
            }],
        };
        assert!(config.find_rule(AnimatableProp::BackgroundColor).is_some());
        assert!(config.find_rule(AnimatableProp::Width).is_none());
    }

    #[test]
    fn find_rule_scale_inheritance() {
        let config = TransitionConfig {
            rules: vec![TransitionRule {
                property: TransitionProperty::Properties(vec![AnimatableProp::Scale]),
                duration_secs: 0.3,
                easing: EaseFn::Linear,
                delay_secs: 0.0,
            }],
        };
        assert!(config.find_rule(AnimatableProp::ScaleX).is_some());
        assert!(config.find_rule(AnimatableProp::ScaleY).is_some());
        assert!(config.find_rule(AnimatableProp::ScaleZ).is_some());
        assert!(config.find_rule(AnimatableProp::TranslateX).is_none());
    }

    #[test]
    fn build_config_wire_priority() {
        let config = build_transition_config(
            &Some("opacity 200ms linear".to_string()),
            &Some(tw_transition_colors()),
            Some(0.3),
            Some(EaseFn::SmoothStep),
            None,
        );
        // Wire prop should win
        assert_eq!(config.rules.len(), 1);
        assert!(config.find_rule(AnimatableProp::Opacity).is_some());
        assert!(config.find_rule(AnimatableProp::BackgroundColor).is_none());
    }

    #[test]
    fn build_config_tw_fallback() {
        let config = build_transition_config(
            &None,
            &Some(tw_transition_colors()),
            Some(0.3),
            Some(EaseFn::CubicOut),
            None,
        );
        assert_eq!(config.rules.len(), 1);
        assert!(config.find_rule(AnimatableProp::BackgroundColor).is_some());
        assert_eq!(config.rules[0].easing, EaseFn::CubicOut);
    }

    // --- Spring tests ---

    #[test]
    fn spring_underdamped_endpoints() {
        let s = EaseFn::spring(100.0, 10.0); // ζ = 0.5, bouncy
        assert!(s.sample(0.0).abs() < 0.001);
        assert!((s.sample(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn spring_underdamped_overshoots() {
        let s = EaseFn::spring(100.0, 10.0); // ζ = 0.5
        // Should overshoot 1.0 somewhere in the middle
        let max = (0..100)
            .map(|i| s.sample(i as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(max > 1.05, "underdamped spring should overshoot, max={max}");
    }

    #[test]
    fn spring_critically_damped_no_overshoot() {
        let s = EaseFn::spring(100.0, 20.0); // ζ ≈ 1.0
        for i in 0..=100 {
            let v = s.sample(i as f32 / 100.0);
            assert!(
                v <= 1.01,
                "critically damped should not overshoot, v={v} at t={}",
                i as f32 / 100.0
            );
        }
        assert!((s.sample(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn spring_overdamped_no_overshoot() {
        let s = EaseFn::spring(100.0, 40.0); // ζ = 2.0
        for i in 0..=100 {
            let v = s.sample(i as f32 / 100.0);
            assert!(v <= 1.01, "overdamped should not overshoot, v={v}");
        }
        assert!((s.sample(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn spring_natural_duration() {
        let s = EaseFn::spring(100.0, 10.0);
        assert!(s.natural_duration().is_some());
        let d = s.natural_duration().unwrap();
        assert!(
            d > 0.5 && d < 5.0,
            "settle duration should be reasonable, got {d}"
        );
    }

    #[test]
    fn spring_no_duration_for_standard_easings() {
        assert!(EaseFn::Linear.natural_duration().is_none());
        assert!(EaseFn::CubicOut.natural_duration().is_none());
    }

    #[test]
    fn parse_easing_spring() {
        let s = parse_easing("spring(100,10)").unwrap();
        assert!(matches!(s, EaseFn::Spring { .. }));
        assert!(s.natural_duration().is_some());
    }

    #[test]
    fn parse_easing_spring_with_spaces() {
        let s = parse_easing("spring(100, 10)").unwrap();
        assert!(matches!(s, EaseFn::Spring { .. }));
    }

    #[test]
    fn parse_transition_spring_only() {
        let rules = parse_transition_prop("all spring(100,10)");
        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].easing, EaseFn::Spring { .. }));
        // Duration should come from the spring, not default
        assert!(rules[0].duration_secs > 0.5);
    }

    #[test]
    fn parse_transition_spring_with_delay() {
        let rules = parse_transition_prop("all spring(100,10) 200ms");
        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].easing, EaseFn::Spring { .. }));
        assert!((rules[0].delay_secs - 0.2).abs() < 0.001);
        // Duration from spring, not 200ms
        assert!(rules[0].duration_secs > 0.5);
    }

    #[test]
    fn parse_transition_spring_overrides_duration() {
        let rules = parse_transition_prop("all 300ms spring(100,10)");
        assert_eq!(rules.len(), 1);
        // Spring duration overrides the 300ms
        assert!(rules[0].duration_secs > 0.5);
    }

    #[test]
    fn parse_transition_multi_with_spring() {
        // Comma inside spring() must not split the rules
        let rules =
            parse_transition_prop("background-color 300ms ease-in-out, opacity spring(100,10)");
        assert_eq!(rules.len(), 2);
        assert!((rules[0].duration_secs - 0.3).abs() < 0.001);
        assert_eq!(rules[0].easing, EaseFn::CubicInOut);
        assert!(matches!(rules[1].easing, EaseFn::Spring { .. }));
    }

    #[test]
    fn split_respecting_parens_basic() {
        let parts = split_respecting_parens("a, b, c", ',');
        assert_eq!(parts, vec!["a", " b", " c"]);
    }

    #[test]
    fn split_respecting_parens_nested() {
        let parts = split_respecting_parens("f(1,2), g(3,4)", ',');
        assert_eq!(parts, vec!["f(1,2)", " g(3,4)"]);
    }

    #[test]
    fn settle_time_reasonable() {
        // Bouncy: ζ=0.3 → longer settle
        let t1 = compute_settle_time(10.0, 0.3);
        // Stiff: ζ=0.8 → shorter settle
        let t2 = compute_settle_time(10.0, 0.8);
        assert!(t1 > t2, "bouncier spring should take longer to settle");
        assert!(t1 > 0.5 && t1 < 10.0);
        assert!(t2 > 0.2 && t2 < 5.0);
    }
}
