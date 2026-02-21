//! Font store — TOML-based font registry with CSS Fonts Level 4 matching.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Serde types for TOML deserialization
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FontRegistry {
    defaults: FontDefaults,
    presets: HashMap<String, String>,
    tailwind: HashMap<String, String>,
    family: Vec<FontFamilyDef>,
}

#[derive(Deserialize)]
struct FontDefaults {
    text: String,
    tty: String,
}

#[derive(Deserialize)]
struct FontFamilyDef {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    variant: Vec<FontVariantDef>,
}

#[derive(Deserialize)]
struct FontVariantDef {
    file: String,
    #[serde(default = "default_weight")]
    weight: u16,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    slant: f32,
    #[serde(default = "default_stretch")]
    stretch: f32,
}

fn default_weight() -> u16 {
    400
}
fn default_stretch() -> f32 {
    100.0
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A requested font style (from CSS font-style property).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FontStyleRequest {
    #[default]
    Normal,
    Italic,
    Oblique(f32),
}

/// Axes for a single font variant.
#[derive(Debug, Clone)]
struct FontAxes {
    weight: u16,
    italic: bool,
    slant: f32,
    stretch: f32,
    file: String,
}

/// A font family with its variants.
#[derive(Debug, Clone)]
struct FontFamily {
    variants: Vec<FontAxes>,
}

/// Cache key for loaded fonts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey(String); // asset path

// ---------------------------------------------------------------------------
// FontStore resource
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct FontStore {
    families: HashMap<String, FontFamily>,
    aliases: HashMap<String, String>,
    tailwind_map: HashMap<String, String>,
    pub default_text_family: String,
    pub default_tty_family: String,
    loaded: HashMap<FontKey, Handle<Font>>,
}

impl FontStore {
    /// Parse the TOML registry and build the store.
    pub fn from_toml(s: &str) -> Self {
        let registry: FontRegistry = toml::from_str(s).expect("failed to parse fonts.toml");

        let mut families = HashMap::new();
        let mut aliases = HashMap::new();

        // Build families
        for fam in &registry.family {
            let canonical = fam.name.to_lowercase();
            let variants: Vec<FontAxes> = fam
                .variant
                .iter()
                .map(|v| FontAxes {
                    weight: v.weight,
                    italic: v.italic,
                    slant: v.slant,
                    stretch: v.stretch,
                    file: v.file.clone(),
                })
                .collect();
            families.insert(canonical.clone(), FontFamily { variants });

            // Register aliases
            for alias in &fam.aliases {
                aliases.insert(alias.to_lowercase(), canonical.clone());
            }
        }

        // Register presets as aliases
        for (preset_name, target) in &registry.presets {
            aliases.insert(preset_name.to_lowercase(), target.to_lowercase());
        }

        // Tailwind map: class name -> preset/family name
        let mut tailwind_map = HashMap::new();
        for (tw_class, preset) in &registry.tailwind {
            tailwind_map.insert(tw_class.clone(), preset.to_lowercase());
        }

        // Resolve defaults
        let default_text_family =
            Self::resolve_chain(&registry.defaults.text.to_lowercase(), &families, &aliases)
                .unwrap_or_default();

        let default_tty_family =
            Self::resolve_chain(&registry.defaults.tty.to_lowercase(), &families, &aliases)
                .unwrap_or_default();

        Self {
            families,
            aliases,
            tailwind_map,
            default_text_family,
            default_tty_family,
            loaded: HashMap::new(),
        }
    }

    /// Resolve a specifier to a canonical family name (case-insensitive).
    /// Follows alias chains (preset -> family name -> canonical).
    pub fn resolve_family(&self, specifier: &str) -> Option<String> {
        Self::resolve_chain(&specifier.to_lowercase(), &self.families, &self.aliases)
    }

    /// Internal chain resolution.
    fn resolve_chain(
        key: &str,
        families: &HashMap<String, FontFamily>,
        aliases: &HashMap<String, String>,
    ) -> Option<String> {
        // Direct family match
        if families.contains_key(key) {
            return Some(key.to_string());
        }
        // Alias chain (max 5 hops to prevent cycles)
        let mut current = key.to_string();
        for _ in 0..5 {
            if let Some(target) = aliases.get(&current) {
                let target_lower = target.to_lowercase();
                if families.contains_key(&target_lower) {
                    return Some(target_lower);
                }
                current = target_lower;
            } else {
                return None;
            }
        }
        None
    }

    /// Resolve a tailwind class name (e.g. "font-mono") to a canonical family name.
    pub fn resolve_tailwind_class(&self, class: &str) -> Option<String> {
        let preset = self.tailwind_map.get(class)?;
        Self::resolve_chain(preset, &self.families, &self.aliases)
    }

    /// Parse a CSS font-family list (e.g. `"Fira Code", monospace, sans-serif`)
    /// and return the first resolved canonical family name.
    pub fn resolve_font_family_list(&self, css: &str) -> Option<String> {
        for part in css.split(',') {
            let trimmed = part.trim();
            // Strip quotes
            let name = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            {
                &trimmed[1..trimmed.len() - 1]
            } else {
                trimmed
            };
            if !name.is_empty()
                && let Some(canonical) = self.resolve_family(name)
            {
                return Some(canonical);
            }
        }
        None
    }

    /// Find the best matching variant using CSS Fonts Level 4 algorithm.
    fn best_match(
        &self,
        family: &str,
        weight: u16,
        style: FontStyleRequest,
        stretch: f32,
    ) -> Option<&FontAxes> {
        let fam = self.families.get(family)?;
        if fam.variants.is_empty() {
            return None;
        }

        let candidates = &fam.variants;

        // Step 1: Filter by closest stretch
        let min_stretch_dist = candidates
            .iter()
            .map(|v| (v.stretch - stretch).abs())
            .fold(f32::INFINITY, f32::min);
        let stretch_filtered: Vec<&FontAxes> = candidates
            .iter()
            .filter(|v| (v.stretch - stretch).abs() - min_stretch_dist < 0.01)
            .collect();

        // Step 2: Filter by style
        let style_filtered = match style {
            FontStyleRequest::Italic => {
                // Prefer italic=true, then highest |slant|, then normal
                let italics: Vec<&&FontAxes> =
                    stretch_filtered.iter().filter(|v| v.italic).collect();
                if !italics.is_empty() {
                    italics.into_iter().copied().collect::<Vec<_>>()
                } else {
                    let max_slant = stretch_filtered
                        .iter()
                        .map(|v| v.slant.abs())
                        .fold(0.0f32, f32::max);
                    if max_slant > 0.0 {
                        stretch_filtered
                            .iter()
                            .filter(|v| (v.slant.abs() - max_slant).abs() < 0.01)
                            .copied()
                            .collect()
                    } else {
                        stretch_filtered
                    }
                }
            }
            FontStyleRequest::Oblique(angle) => {
                // Prefer closest slant to angle, then italic=true, then normal
                let obliques: Vec<&&FontAxes> =
                    stretch_filtered.iter().filter(|v| v.slant != 0.0).collect();
                if !obliques.is_empty() {
                    let min_dist = obliques
                        .iter()
                        .map(|v| (v.slant - angle).abs())
                        .fold(f32::INFINITY, f32::min);
                    obliques
                        .into_iter()
                        .filter(|v| (v.slant - angle).abs() - min_dist < 0.01)
                        .copied()
                        .collect()
                } else {
                    let italics: Vec<&&FontAxes> =
                        stretch_filtered.iter().filter(|v| v.italic).collect();
                    if !italics.is_empty() {
                        italics.into_iter().copied().collect()
                    } else {
                        stretch_filtered
                    }
                }
            }
            FontStyleRequest::Normal => {
                // Prefer !italic && slant==0, then lowest |slant|, then italic
                let normals: Vec<&&FontAxes> = stretch_filtered
                    .iter()
                    .filter(|v| !v.italic && v.slant == 0.0)
                    .collect();
                if !normals.is_empty() {
                    normals.into_iter().copied().collect()
                } else {
                    let min_slant = stretch_filtered
                        .iter()
                        .filter(|v| !v.italic)
                        .map(|v| v.slant.abs())
                        .fold(f32::INFINITY, f32::min);
                    if min_slant < f32::INFINITY {
                        stretch_filtered
                            .iter()
                            .filter(|v| !v.italic && (v.slant.abs() - min_slant).abs() < 0.01)
                            .copied()
                            .collect()
                    } else {
                        stretch_filtered
                    }
                }
            }
        };

        // Step 3: Weight matching
        // Exact match
        if let Some(v) = style_filtered.iter().find(|v| v.weight == weight) {
            return Some(v);
        }

        if weight <= 400 {
            // Search down then up
            let mut best_below = None;
            let mut best_above = None;
            for v in &style_filtered {
                if v.weight <= weight {
                    if best_below.is_none_or(|b: &FontAxes| v.weight > b.weight) {
                        best_below = Some(*v);
                    }
                } else if best_above.is_none_or(|b: &FontAxes| v.weight < b.weight) {
                    best_above = Some(*v);
                }
            }
            best_below.or(best_above)
        } else {
            // Search up then down
            let mut best_above = None;
            let mut best_below = None;
            for v in &style_filtered {
                if v.weight >= weight {
                    if best_above.is_none_or(|b: &FontAxes| v.weight < b.weight) {
                        best_above = Some(*v);
                    }
                } else if best_below.is_none_or(|b: &FontAxes| v.weight > b.weight) {
                    best_below = Some(*v);
                }
            }
            best_above.or(best_below)
        }
    }

    /// Get or load a font handle by asset path.
    fn get_or_load(&mut self, path: &str, asset_server: &AssetServer) -> Handle<Font> {
        let key = FontKey(path.to_string());
        if let Some(handle) = self.loaded.get(&key) {
            return handle.clone();
        }
        let handle = asset_server.load::<Font>(path.to_string());
        self.loaded.insert(key, handle.clone());
        handle
    }

    /// Full resolution pipeline: specifier -> family -> best match -> Handle<Font>.
    /// Returns `None` only if the default family itself has no variants.
    pub fn resolve_font(
        &mut self,
        family_spec: Option<&str>,
        weight: u16,
        style: FontStyleRequest,
        stretch: f32,
        default_family: &str,
        asset_server: &AssetServer,
    ) -> Option<Handle<Font>> {
        // Try the specifier first (CSS font-family list or single name)
        let canonical = family_spec
            .and_then(|s| self.resolve_font_family_list(s))
            .or_else(|| Some(default_family.to_string()));

        let canonical = canonical?;

        if let Some(axes) = self.best_match(&canonical, weight, style, stretch) {
            let path = axes.file.clone();
            return Some(self.get_or_load(&path, asset_server));
        }

        // Fallback: try default family
        if canonical != default_family
            && let Some(axes) = self.best_match(default_family, weight, style, stretch)
        {
            let path = axes.file.clone();
            return Some(self.get_or_load(&path, asset_server));
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers (public for use in props/types.rs)
// ---------------------------------------------------------------------------

/// Parse a CSS font-weight keyword or numeric value.
pub fn parse_font_weight(s: &str) -> Option<u16> {
    match s.to_lowercase().as_str() {
        "thin" | "hairline" => Some(100),
        "extralight" | "ultralight" => Some(200),
        "light" => Some(300),
        "normal" | "regular" => Some(400),
        "medium" => Some(500),
        "semibold" | "demibold" => Some(600),
        "bold" => Some(700),
        "extrabold" | "ultrabold" => Some(800),
        "black" | "heavy" => Some(900),
        "extrablack" | "ultrablack" => Some(950),
        _ => {
            let n = s.parse::<u16>().ok()?;
            (1..=1000).contains(&n).then_some(n)
        }
    }
}

/// Parse a CSS font-stretch keyword or numeric percentage.
pub fn parse_font_stretch(s: &str) -> Option<f32> {
    match s.to_lowercase().as_str() {
        "ultra-condensed" => Some(50.0),
        "extra-condensed" => Some(62.5),
        "condensed" => Some(75.0),
        "semi-condensed" => Some(87.5),
        "normal" => Some(100.0),
        "semi-expanded" => Some(112.5),
        "expanded" => Some(125.0),
        "extra-expanded" => Some(150.0),
        "ultra-expanded" => Some(200.0),
        _ => {
            let n = s.strip_suffix('%').unwrap_or(s).parse::<f32>().ok()?;
            (n > 0.0).then_some(n)
        }
    }
}

/// Parse a CSS font-style value.
pub fn parse_font_style(s: &str) -> Option<FontStyleRequest> {
    let lower = s.to_lowercase();
    match lower.as_str() {
        "normal" => Some(FontStyleRequest::Normal),
        "italic" => Some(FontStyleRequest::Italic),
        "oblique" => Some(FontStyleRequest::Oblique(14.0)),
        _ => {
            if let Some(rest) = lower.strip_prefix("oblique") {
                let rest = rest.trim();
                let deg_str = rest.strip_suffix("deg").unwrap_or(rest);
                let angle = deg_str.trim().parse::<f32>().ok()?;
                Some(FontStyleRequest::Oblique(angle))
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Startup system
// ---------------------------------------------------------------------------

/// Read and parse `assets/fonts/fonts.toml`, insert `FontStore` resource.
pub fn setup_font_store(mut commands: Commands) {
    let toml_bytes = include_str!("../../../assets/fonts/fonts.toml");
    let store = FontStore::from_toml(toml_bytes);
    info!(
        "FontStore loaded: {} families, text default={}, tty default={}",
        store.families.len(),
        store.default_text_family,
        store.default_tty_family,
    );
    commands.insert_resource(store);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> FontStore {
        let toml = include_str!("../../../assets/fonts/fonts.toml");
        FontStore::from_toml(toml)
    }

    #[test]
    fn defaults_resolved() {
        let store = test_store();
        assert_eq!(store.default_text_family, "vend sans");
        assert_eq!(store.default_tty_family, "fira mono");
    }

    #[test]
    fn resolve_direct_family() {
        let store = test_store();
        assert_eq!(store.resolve_family("Vend Sans"), Some("vend sans".into()));
        assert_eq!(store.resolve_family("vend sans"), Some("vend sans".into()));
        assert_eq!(store.resolve_family("FIRA CODE"), Some("fira code".into()));
    }

    #[test]
    fn resolve_alias() {
        let store = test_store();
        assert_eq!(store.resolve_family("VendSans"), Some("vend sans".into()));
        assert_eq!(store.resolve_family("FiraCode"), Some("fira code".into()));
        assert_eq!(
            store.resolve_family("DMSerifText"),
            Some("dm serif text".into())
        );
    }

    #[test]
    fn resolve_preset() {
        let store = test_store();
        assert_eq!(store.resolve_family("system-ui"), Some("vend sans".into()));
        assert_eq!(
            store.resolve_family("ui-monospace"),
            Some("fira mono".into())
        );
        assert_eq!(
            store.resolve_family("serif"),
            Some("liberation serif".into())
        );
        assert_eq!(
            store.resolve_family("sans-serif"),
            Some("liberation sans".into())
        );
        assert_eq!(
            store.resolve_family("monospace"),
            Some("liberation mono".into())
        );
        assert_eq!(store.resolve_family("cursive"), Some("felipa".into()));
        assert_eq!(store.resolve_family("fantasy"), Some("tangerine".into()));
        assert_eq!(store.resolve_family("code"), Some("fira code".into()));
    }

    #[test]
    fn resolve_tailwind_class() {
        let store = test_store();
        assert_eq!(
            store.resolve_tailwind_class("font-mono"),
            Some("liberation mono".into())
        );
        assert_eq!(
            store.resolve_tailwind_class("font-sans"),
            Some("liberation sans".into())
        );
        assert_eq!(
            store.resolve_tailwind_class("font-code"),
            Some("fira code".into())
        );
        assert_eq!(
            store.resolve_tailwind_class("font-system"),
            Some("vend sans".into())
        );
    }

    #[test]
    fn resolve_font_family_list() {
        let store = test_store();
        assert_eq!(
            store.resolve_font_family_list("\"Fira Code\", monospace, sans-serif"),
            Some("fira code".into())
        );
        assert_eq!(
            store.resolve_font_family_list("'Unknown Font', serif"),
            Some("liberation serif".into())
        );
        assert_eq!(
            store.resolve_font_family_list("NonExistent, AnotherFake"),
            None
        );
    }

    #[test]
    fn resolve_unknown_family() {
        let store = test_store();
        assert_eq!(store.resolve_family("Comic Sans MS"), None);
    }

    // ── Weight matching ─────────────────────────────────────────────

    #[test]
    fn best_match_exact_weight() {
        let store = test_store();
        let v = store
            .best_match("vend sans", 400, FontStyleRequest::Normal, 100.0)
            .unwrap();
        assert_eq!(v.weight, 400);
        assert!(!v.italic);
    }

    #[test]
    fn best_match_weight_down_then_up() {
        let store = test_store();
        // Request 350 (<=400) → should go down to 300
        let v = store
            .best_match("vend sans", 350, FontStyleRequest::Normal, 100.0)
            .unwrap();
        assert_eq!(v.weight, 300);
    }

    #[test]
    fn best_match_weight_up_then_down() {
        let store = test_store();
        // Request 550 (>=500) → should go up to 600
        let v = store
            .best_match("vend sans", 550, FontStyleRequest::Normal, 100.0)
            .unwrap();
        assert_eq!(v.weight, 600);
    }

    #[test]
    fn best_match_italic() {
        let store = test_store();
        let v = store
            .best_match("vend sans", 400, FontStyleRequest::Italic, 100.0)
            .unwrap();
        assert!(v.italic);
        assert_eq!(v.weight, 400);
    }

    #[test]
    fn best_match_italic_bold() {
        let store = test_store();
        let v = store
            .best_match("vend sans", 700, FontStyleRequest::Italic, 100.0)
            .unwrap();
        assert!(v.italic);
        assert_eq!(v.weight, 700);
    }

    #[test]
    fn best_match_italic_fallback_to_normal() {
        let store = test_store();
        // Comfortaa has no italic variants
        let v = store
            .best_match("comfortaa", 400, FontStyleRequest::Italic, 100.0)
            .unwrap();
        assert_eq!(v.weight, 400);
    }

    #[test]
    fn best_match_no_such_family() {
        let store = test_store();
        assert!(
            store
                .best_match("nonexistent", 400, FontStyleRequest::Normal, 100.0)
                .is_none()
        );
    }

    // ── Parsing helpers ─────────────────────────────────────────────

    #[test]
    fn parse_font_weight_keywords() {
        assert_eq!(parse_font_weight("thin"), Some(100));
        assert_eq!(parse_font_weight("normal"), Some(400));
        assert_eq!(parse_font_weight("bold"), Some(700));
        assert_eq!(parse_font_weight("black"), Some(900));
        assert_eq!(parse_font_weight("extrablack"), Some(950));
    }

    #[test]
    fn parse_font_weight_numeric() {
        assert_eq!(parse_font_weight("400"), Some(400));
        assert_eq!(parse_font_weight("1"), Some(1));
        assert_eq!(parse_font_weight("1000"), Some(1000));
        assert_eq!(parse_font_weight("0"), None);
        assert_eq!(parse_font_weight("1001"), None);
    }

    #[test]
    fn parse_font_stretch_keywords() {
        assert_eq!(parse_font_stretch("ultra-condensed"), Some(50.0));
        assert_eq!(parse_font_stretch("normal"), Some(100.0));
        assert_eq!(parse_font_stretch("ultra-expanded"), Some(200.0));
    }

    #[test]
    fn parse_font_stretch_numeric() {
        assert_eq!(parse_font_stretch("75"), Some(75.0));
        assert_eq!(parse_font_stretch("125%"), Some(125.0));
    }

    #[test]
    fn parse_font_style_values() {
        assert_eq!(parse_font_style("normal"), Some(FontStyleRequest::Normal));
        assert_eq!(parse_font_style("italic"), Some(FontStyleRequest::Italic));
        assert_eq!(
            parse_font_style("oblique"),
            Some(FontStyleRequest::Oblique(14.0))
        );
        assert_eq!(
            parse_font_style("oblique 20deg"),
            Some(FontStyleRequest::Oblique(20.0))
        );
        assert_eq!(
            parse_font_style("oblique 12"),
            Some(FontStyleRequest::Oblique(12.0))
        );
    }
}
