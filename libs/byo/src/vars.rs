//! Variable interpolation utilities for `$name(args)` references in prop values.
//!
//! BYO prop values can contain variable references like `$img(42)` or
//! `$env(HOME)`. This module provides scanning and replacement utilities
//! that are protocol-level (no interpretation of specific variable types).
//!
//! # Syntax
//!
//! ```text
//! $name(arg)
//! $name(arg1,arg2)     // future: multiple args
//! ```
//!
//! Variable references can appear as complete prop values or embedded within
//! strings.

use std::borrow::Cow;
use std::ops::Range;

/// A parsed variable reference found within a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef<'a> {
    /// Variable name (e.g. `"img"`, `"env"`)
    pub name: &'a str,
    /// Arguments string (e.g. `"42"`, `"HOME"`)
    pub args: &'a str,
    /// Byte offset range of the entire `$name(args)` in the source string.
    pub span: Range<usize>,
}

/// Find all `$name(args)` references in a string.
///
/// Returns references in order of appearance. Nested parentheses are not
/// supported — the first `)` closes the reference.
///
/// # Examples
///
/// ```
/// use byo::vars::scan;
///
/// let refs = scan("$img(42)");
/// assert_eq!(refs.len(), 1);
/// assert_eq!(refs[0].name, "img");
/// assert_eq!(refs[0].args, "42");
///
/// let refs = scan("bg=$img(7) fg=$env(HOME)");
/// assert_eq!(refs.len(), 2);
/// assert_eq!(refs[0].name, "img");
/// assert_eq!(refs[1].name, "env");
/// ```
pub fn scan(value: &str) -> Vec<VarRef<'_>> {
    let mut refs = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && is_name_start(bytes[i + 1]) {
            let start = i;
            i += 1; // skip '$'

            // Parse name: [a-zA-Z][a-zA-Z0-9_]*
            let name_start = i;
            while i < bytes.len() && is_name_char(bytes[i]) {
                i += 1;
            }
            let name = &value[name_start..i];

            // Expect '('
            if i < bytes.len() && bytes[i] == b'(' {
                i += 1; // skip '('
                let args_start = i;

                // Scan for closing ')'
                while i < bytes.len() && bytes[i] != b')' {
                    i += 1;
                }

                if i < bytes.len() {
                    // Found ')'
                    let args = &value[args_start..i];
                    i += 1; // skip ')'
                    refs.push(VarRef {
                        name,
                        args,
                        span: start..i,
                    });
                }
                // If no ')' found, i is at end — not a valid ref, skip
            }
            // No '(' after name — not a valid ref, continue scanning
        } else {
            i += 1;
        }
    }

    refs
}

/// Replace `$name(args)` references using a callback.
///
/// For each variable reference found, the callback is called. If it returns
/// `Some(replacement)`, the reference is replaced. If `None`, the reference
/// is left unchanged.
///
/// Returns `Cow::Borrowed` if no replacements were made (zero allocation).
///
/// # Examples
///
/// ```
/// use byo::vars::replace;
///
/// let result = replace("$img(42)", |var| {
///     if var.name == "img" { Some(format!("texture_{}", var.args)) }
///     else { None }
/// });
/// assert_eq!(result, "texture_42");
///
/// // No match → borrowed
/// let result = replace("plain text", |_| None);
/// assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
/// ```
pub fn replace<'a>(value: &'a str, mut f: impl FnMut(&VarRef) -> Option<String>) -> Cow<'a, str> {
    let refs = scan(value);
    if refs.is_empty() {
        return Cow::Borrowed(value);
    }

    let mut result = String::with_capacity(value.len());
    let mut last_end = 0;

    let mut any_replaced = false;
    for var in &refs {
        if let Some(replacement) = f(var) {
            result.push_str(&value[last_end..var.span.start]);
            result.push_str(&replacement);
            last_end = var.span.end;
            any_replaced = true;
        }
    }

    if !any_replaced {
        return Cow::Borrowed(value);
    }

    // Append remaining text after last replacement
    result.push_str(&value[last_end..]);
    Cow::Owned(result)
}

/// A parsed multi-density image entry from `$img()` args.
///
/// Entries represent individual images in a multi-density set:
/// - `$img(1)` → one entry with id=1, scale=1.0, no modifiers
/// - `$img(1 @2x, 2 @1x)` → two entries with different scales
/// - `$img(1@2x dark, 2@1x light)` → entries with modifiers (future use)
#[derive(Debug, Clone, PartialEq)]
pub struct ImgEntry {
    pub id: u32,
    pub scale: f32,
    pub modifiers: Vec<String>,
}

/// Parse `$img()` args into multi-density entries.
///
/// Syntax: comma-separated entries, each with a leading integer ID,
/// optional `@<float>x` scale, and optional word modifiers.
///
/// # Examples
///
/// ```
/// use byo::vars::parse_img_args;
///
/// let entries = parse_img_args("42").unwrap();
/// assert_eq!(entries.len(), 1);
/// assert_eq!(entries[0].id, 42);
/// assert_eq!(entries[0].scale, 1.0);
///
/// let entries = parse_img_args("1 @2x, 2 @1x").unwrap();
/// assert_eq!(entries.len(), 2);
/// assert_eq!(entries[0].scale, 2.0);
/// assert_eq!(entries[1].scale, 1.0);
/// ```
pub fn parse_img_args(args: &str) -> Option<Vec<ImgEntry>> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }

    let mut entries = Vec::new();

    for part in args.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Tokenize: split on whitespace, but also split leading digits from `@`
        // e.g. "1@2x" → ["1", "@2x"] and "1 @2x dark" → ["1", "@2x", "dark"]
        let mut tokens = Vec::new();
        for token in part.split_whitespace() {
            // Check if token has digits followed by @: "1@2x" → "1" + "@2x"
            if let Some(at_pos) = token.find('@') {
                if at_pos > 0 {
                    tokens.push(&token[..at_pos]);
                    tokens.push(&token[at_pos..]);
                } else {
                    tokens.push(token);
                }
            } else {
                tokens.push(token);
            }
        }

        if tokens.is_empty() {
            continue;
        }

        // First token must be a number (the image ID)
        let id: u32 = tokens[0].parse().ok()?;
        let mut scale = 1.0_f32;
        let mut modifiers = Vec::new();

        for &tok in &tokens[1..] {
            if tok.starts_with('@') && tok.ends_with('x') && tok.len() > 2 {
                // Scale descriptor: @2x, @1.5x, etc.
                let num_str = &tok[1..tok.len() - 1];
                if let Ok(s) = num_str.parse::<f32>() {
                    scale = s;
                    continue;
                }
            }
            // Everything else is a modifier
            modifiers.push(tok.to_string());
        }

        entries.push(ImgEntry {
            id,
            scale,
            modifiers,
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Pick the best image entry for a given display scale factor.
///
/// Algorithm: smallest scale >= display_scale, else largest available.
///
/// # Examples
///
/// ```
/// use byo::vars::{ImgEntry, best_img_match};
///
/// let entries = vec![
///     ImgEntry { id: 1, scale: 1.0, modifiers: vec![] },
///     ImgEntry { id: 2, scale: 2.0, modifiers: vec![] },
/// ];
/// // On a 2x display, pick the 2x image
/// assert_eq!(best_img_match(&entries, 2.0).unwrap().id, 2);
/// // On a 1x display, pick the 1x image
/// assert_eq!(best_img_match(&entries, 1.0).unwrap().id, 1);
/// ```
pub fn best_img_match(entries: &[ImgEntry], display_scale: f32) -> Option<&ImgEntry> {
    if entries.is_empty() {
        return None;
    }

    // Find smallest scale >= display_scale
    let mut best_ge: Option<&ImgEntry> = None;
    let mut best_largest: Option<&ImgEntry> = None;

    for entry in entries {
        if entry.scale >= display_scale && best_ge.is_none_or(|b| entry.scale < b.scale) {
            best_ge = Some(entry);
        }
        if best_largest.is_none_or(|b| entry.scale > b.scale) {
            best_largest = Some(entry);
        }
    }

    best_ge.or(best_largest)
}

/// Serialize image entries back to `$img()` args string.
///
/// Single entry with scale=1.0 and no modifiers produces just the ID
/// for backward compatibility: `"42"` instead of `"42 @1x"`.
///
/// # Examples
///
/// ```
/// use byo::vars::{ImgEntry, format_img_args};
///
/// // Simple backward-compatible case
/// let entries = vec![ImgEntry { id: 42, scale: 1.0, modifiers: vec![] }];
/// assert_eq!(format_img_args(&entries), "42");
///
/// // Multi-entry always includes scale
/// let entries = vec![
///     ImgEntry { id: 1, scale: 2.0, modifiers: vec![] },
///     ImgEntry { id: 2, scale: 1.0, modifiers: vec![] },
/// ];
/// assert_eq!(format_img_args(&entries), "1 @2x, 2 @1x");
/// ```
pub fn format_img_args(entries: &[ImgEntry]) -> String {
    // Single entry with scale=1.0 and no modifiers → just the ID
    if entries.len() == 1 && entries[0].scale == 1.0 && entries[0].modifiers.is_empty() {
        return entries[0].id.to_string();
    }

    let mut parts = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut s = entry.id.to_string();
        // Format scale: omit trailing .0 for integer scales
        if entry.scale == entry.scale.floor() {
            s.push_str(&format!(" @{}x", entry.scale as u32));
        } else {
            s.push_str(&format!(" @{}x", entry.scale));
        }
        for m in &entry.modifiers {
            s.push(' ');
            s.push_str(m);
        }
        parts.push(s);
    }

    parts.join(", ")
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── scan() tests ────────────────────────────────────────────

    #[test]
    fn scan_single_ref() {
        let refs = scan("$img(42)");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "img");
        assert_eq!(refs[0].args, "42");
        assert_eq!(refs[0].span, 0..8);
    }

    #[test]
    fn scan_multiple_refs() {
        let refs = scan("bg=$img(7) fg=$env(HOME)");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "img");
        assert_eq!(refs[0].args, "7");
        assert_eq!(refs[1].name, "env");
        assert_eq!(refs[1].args, "HOME");
    }

    #[test]
    fn scan_no_refs() {
        let refs = scan("plain text with no variables");
        assert!(refs.is_empty());
    }

    #[test]
    fn scan_dollar_without_parens() {
        // $foo without (args) is not a valid ref
        let refs = scan("$foo bar");
        assert!(refs.is_empty());
    }

    #[test]
    fn scan_dollar_number() {
        // $1 is not valid (name must start with alpha)
        let refs = scan("$1(x)");
        assert!(refs.is_empty());
    }

    #[test]
    fn scan_empty_args() {
        let refs = scan("$img()");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "img");
        assert_eq!(refs[0].args, "");
    }

    #[test]
    fn scan_comma_separated_args() {
        let refs = scan("$ctx(key,fallback)");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "ctx");
        assert_eq!(refs[0].args, "key,fallback");
    }

    #[test]
    fn scan_unclosed_paren() {
        // Unclosed paren — not a valid ref
        let refs = scan("$img(42");
        assert!(refs.is_empty());
    }

    #[test]
    fn scan_adjacent_refs() {
        let refs = scan("$a(1)$b(2)");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "a");
        assert_eq!(refs[0].args, "1");
        assert_eq!(refs[1].name, "b");
        assert_eq!(refs[1].args, "2");
    }

    #[test]
    fn scan_ref_with_underscores() {
        let refs = scan("$my_var(test_arg)");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "my_var");
        assert_eq!(refs[0].args, "test_arg");
    }

    #[test]
    fn scan_embedded_in_text() {
        let refs = scan("url(http://example.com/$img(3)/path)");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "img");
        assert_eq!(refs[0].args, "3");
    }

    // ── replace() tests ─────────────────────────────────────────

    #[test]
    fn replace_single() {
        let result = replace("$img(42)", |var| {
            if var.name == "img" {
                Some(format!("texture_{}", var.args))
            } else {
                None
            }
        });
        assert_eq!(result, "texture_42");
    }

    #[test]
    fn replace_no_match_borrows() {
        let result = replace("plain text", |_| None);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "plain text");
    }

    #[test]
    fn replace_no_vars_borrows() {
        let result = replace("no variables here", |_| Some("x".to_string()));
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn replace_multiple() {
        let result = replace("$a(1) and $b(2)", |var| {
            Some(format!("[{}={}]", var.name, var.args))
        });
        assert_eq!(result, "[a=1] and [b=2]");
    }

    #[test]
    fn replace_selective() {
        // Only replace $img, leave $env as-is
        let result = replace("$img(5) $env(HOME)", |var| {
            if var.name == "img" {
                Some("replaced".to_string())
            } else {
                None
            }
        });
        assert_eq!(result, "replaced $env(HOME)");
    }

    #[test]
    fn replace_preserves_surrounding_text() {
        let result = replace("before $img(1) after", |_| Some("X".to_string()));
        assert_eq!(result, "before X after");
    }

    #[test]
    fn replace_empty_string() {
        let result = replace("", |_| Some("X".to_string()));
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "");
    }

    // ── parse_img_args() tests ───────────────────────────────────

    #[test]
    fn parse_single_id() {
        let entries = parse_img_args("42").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 42);
        assert_eq!(entries[0].scale, 1.0);
        assert!(entries[0].modifiers.is_empty());
    }

    #[test]
    fn parse_multi_entry() {
        let entries = parse_img_args("1 @2x, 2 @1x").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[0].scale, 2.0);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[1].scale, 1.0);
    }

    #[test]
    fn parse_glued_at() {
        let entries = parse_img_args("1@2x, 2@1x").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[0].scale, 2.0);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[1].scale, 1.0);
    }

    #[test]
    fn parse_fractional_scale() {
        let entries = parse_img_args("5 @1.5x").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 5);
        assert_eq!(entries[0].scale, 1.5);
    }

    #[test]
    fn parse_with_modifiers() {
        let entries = parse_img_args("1@2x dark, 2@1x light").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].modifiers, vec!["dark"]);
        assert_eq!(entries[1].modifiers, vec!["light"]);
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_img_args("").is_none());
        assert!(parse_img_args("  ").is_none());
    }

    #[test]
    fn parse_invalid_first_token_returns_none() {
        assert!(parse_img_args("abc").is_none());
        assert!(parse_img_args("@2x").is_none());
    }

    #[test]
    fn parse_single_with_scale() {
        let entries = parse_img_args("3 @3x").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 3);
        assert_eq!(entries[0].scale, 3.0);
    }

    // ── best_img_match() tests ───────────────────────────────────

    #[test]
    fn match_exact_scale() {
        let entries = vec![
            ImgEntry {
                id: 1,
                scale: 1.0,
                modifiers: vec![],
            },
            ImgEntry {
                id: 2,
                scale: 2.0,
                modifiers: vec![],
            },
        ];
        assert_eq!(best_img_match(&entries, 2.0).unwrap().id, 2);
        assert_eq!(best_img_match(&entries, 1.0).unwrap().id, 1);
    }

    #[test]
    fn match_smallest_ge() {
        let entries = vec![
            ImgEntry {
                id: 1,
                scale: 1.0,
                modifiers: vec![],
            },
            ImgEntry {
                id: 2,
                scale: 2.0,
                modifiers: vec![],
            },
            ImgEntry {
                id: 3,
                scale: 3.0,
                modifiers: vec![],
            },
        ];
        // 1.5x display → pick 2x (smallest >= 1.5)
        assert_eq!(best_img_match(&entries, 1.5).unwrap().id, 2);
    }

    #[test]
    fn match_fallback_largest() {
        let entries = vec![
            ImgEntry {
                id: 1,
                scale: 1.0,
                modifiers: vec![],
            },
            ImgEntry {
                id: 2,
                scale: 2.0,
                modifiers: vec![],
            },
        ];
        // 3x display → no scale >= 3, fallback to largest (2x)
        assert_eq!(best_img_match(&entries, 3.0).unwrap().id, 2);
    }

    #[test]
    fn match_single_entry() {
        let entries = vec![ImgEntry {
            id: 42,
            scale: 1.0,
            modifiers: vec![],
        }];
        assert_eq!(best_img_match(&entries, 2.0).unwrap().id, 42);
        assert_eq!(best_img_match(&entries, 1.0).unwrap().id, 42);
    }

    #[test]
    fn match_empty_returns_none() {
        assert!(best_img_match(&[], 2.0).is_none());
    }

    // ── format_img_args() tests ──────────────────────────────────

    #[test]
    fn format_single_default_scale() {
        let entries = vec![ImgEntry {
            id: 42,
            scale: 1.0,
            modifiers: vec![],
        }];
        assert_eq!(format_img_args(&entries), "42");
    }

    #[test]
    fn format_single_non_default_scale() {
        let entries = vec![ImgEntry {
            id: 5,
            scale: 2.0,
            modifiers: vec![],
        }];
        assert_eq!(format_img_args(&entries), "5 @2x");
    }

    #[test]
    fn format_multi_entry() {
        let entries = vec![
            ImgEntry {
                id: 1,
                scale: 2.0,
                modifiers: vec![],
            },
            ImgEntry {
                id: 2,
                scale: 1.0,
                modifiers: vec![],
            },
        ];
        assert_eq!(format_img_args(&entries), "1 @2x, 2 @1x");
    }

    #[test]
    fn format_fractional_scale() {
        let entries = vec![ImgEntry {
            id: 1,
            scale: 1.5,
            modifiers: vec![],
        }];
        assert_eq!(format_img_args(&entries), "1 @1.5x");
    }

    #[test]
    fn format_with_modifiers() {
        let entries = vec![
            ImgEntry {
                id: 1,
                scale: 2.0,
                modifiers: vec!["dark".into()],
            },
            ImgEntry {
                id: 2,
                scale: 1.0,
                modifiers: vec!["light".into()],
            },
        ];
        assert_eq!(format_img_args(&entries), "1 @2x dark, 2 @1x light");
    }

    #[test]
    fn format_parse_roundtrip() {
        let original = "1 @2x, 2 @1x";
        let entries = parse_img_args(original).unwrap();
        assert_eq!(format_img_args(&entries), original);
    }

    #[test]
    fn format_parse_roundtrip_single() {
        let original = "42";
        let entries = parse_img_args(original).unwrap();
        assert_eq!(format_img_args(&entries), original);
    }

    #[test]
    fn format_parse_roundtrip_modifiers() {
        let original = "1 @2x dark, 2 @1x light";
        let entries = parse_img_args(original).unwrap();
        assert_eq!(format_img_args(&entries), original);
    }
}
