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
}

impl EaseFn {
    /// Sample the easing function at time `t` in [0, 1].
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
        }
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
pub fn parse_transition_prop(value: &str) -> Vec<TransitionRule> {
    value
        .split(',')
        .filter_map(|part| {
            let tokens: Vec<&str> = part.split_whitespace().collect();
            if tokens.is_empty() {
                return None;
            }

            let property = parse_property_name(tokens[0])?;
            let duration_secs = tokens.get(1).and_then(|s| parse_duration(s)).unwrap_or(0.0);
            let easing = tokens
                .get(2)
                .and_then(|s| parse_easing(s))
                .unwrap_or(EaseFn::SmoothStep);
            let delay_secs = tokens.get(3).and_then(|s| parse_duration(s)).unwrap_or(0.0);

            Some(TransitionRule {
                property,
                duration_secs,
                easing,
                delay_secs,
            })
        })
        .collect()
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

/// Parse a CSS easing function name.
pub fn parse_easing(s: &str) -> Option<EaseFn> {
    Some(match s {
        "linear" => EaseFn::Linear,
        "ease" => EaseFn::SmoothStep,
        "ease-in" => EaseFn::CubicIn,
        "ease-out" => EaseFn::CubicOut,
        "ease-in-out" => EaseFn::CubicInOut,
        _ => return None,
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
    let duration_secs = tw_duration.unwrap_or(0.15);
    let easing = tw_easing.unwrap_or(EaseFn::SmoothStep);
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
}
