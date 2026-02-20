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
}
