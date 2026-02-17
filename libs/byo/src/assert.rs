//! Structural assertion helpers for BYO/OS protocol output.
//!
//! These functions parse both actual and expected BYO strings and compare
//! them structurally, providing clear diffs on mismatch. Use with
//! [`byo_str!`](crate::byo_str) or [`byo_assert_eq!`](crate::byo_assert_eq)
//! for compile-time-checked expected values.

use crate::parser::parse;
use crate::protocol::{Command, strip_apc};

/// Parse both strings and assert structural equality.
///
/// APC framing is automatically stripped from `actual` before parsing.
/// Commands are compared in order. Props within each command are also
/// compared in order (derived `PartialEq`). Panics with a diff on mismatch.
///
/// # Panics
///
/// Panics if either string fails to parse or if the parsed commands differ.
pub fn assert_eq(actual: &str, expected: &str) {
    let actual = strip_apc(actual);
    let actual_cmds = match parse(actual) {
        Ok(cmds) => cmds,
        Err(e) => panic!(
            "byo::assert::assert_eq: failed to parse actual output:\n  error: {e}\n  actual: {actual:?}"
        ),
    };
    let expected_cmds = match parse(expected) {
        Ok(cmds) => cmds,
        Err(e) => panic!(
            "byo::assert::assert_eq: failed to parse expected output:\n  error: {e}\n  expected: {expected:?}"
        ),
    };

    if actual_cmds != expected_cmds {
        let mut msg = String::from("byo::assert::assert_eq: structural mismatch\n");

        msg.push_str(&format!("  actual ({} commands):\n", actual_cmds.len()));
        for (i, cmd) in actual_cmds.iter().enumerate() {
            msg.push_str(&format!("    [{i}] {cmd:?}\n"));
        }

        msg.push_str(&format!("  expected ({} commands):\n", expected_cmds.len()));
        for (i, cmd) in expected_cmds.iter().enumerate() {
            msg.push_str(&format!("    [{i}] {cmd:?}\n"));
        }

        // Find the first differing command
        let min_len = actual_cmds.len().min(expected_cmds.len());
        for i in 0..min_len {
            if actual_cmds[i] != expected_cmds[i] {
                msg.push_str(&format!("  first diff at index {i}:\n"));
                msg.push_str(&format!("    actual:   {:?}\n", actual_cmds[i]));
                msg.push_str(&format!("    expected: {:?}\n", expected_cmds[i]));
                break;
            }
        }
        if actual_cmds.len() != expected_cmds.len()
            && min_len == actual_cmds.len().min(expected_cmds.len())
        {
            let longer = if actual_cmds.len() > expected_cmds.len() {
                "actual"
            } else {
                "expected"
            };
            msg.push_str(&format!(
                "  {longer} has {} extra command(s)\n",
                actual_cmds.len().abs_diff(expected_cmds.len())
            ));
        }

        msg.push_str(&format!("  raw actual:   {actual:?}\n"));
        msg.push_str(&format!("  raw expected: {expected:?}\n"));
        panic!("{msg}");
    }
}

/// Convenience: same as [`assert_eq`] but accepts `&[u8]` for actual.
///
/// # Panics
///
/// Panics if `actual` is not valid UTF-8, or if the structural comparison fails.
pub fn assert_eq_bytes(actual: &[u8], expected: &str) {
    let actual_str = std::str::from_utf8(actual)
        .unwrap_or_else(|e| panic!("byo::assert::assert_eq_bytes: actual is not valid UTF-8: {e}"));
    assert_eq(actual_str, expected);
}

/// Compare two command slices with prop-order-insensitive comparison.
///
/// Commands are compared in order, but props within each command are
/// compared as unordered sets (using a bitmask scan). Useful for the
/// rare case where prop ordering is non-deterministic.
pub fn commands_eq_unordered(a: &[Command<'_>], b: &[Command<'_>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(ca, cb)| cmd_eq_unordered(ca, cb))
}

fn cmd_eq_unordered<'a>(a: &Command<'a>, b: &Command<'a>) -> bool {
    match (a, b) {
        (
            Command::Upsert {
                kind: ka,
                id: ia,
                props: pa,
            },
            Command::Upsert {
                kind: kb,
                id: ib,
                props: pb,
            },
        ) => ka == kb && ia == ib && props_eq_unordered(pa, pb),

        (Command::Destroy { kind: ka, id: ia }, Command::Destroy { kind: kb, id: ib }) => {
            ka == kb && ia == ib
        }

        (Command::Push, Command::Push) | (Command::Pop, Command::Pop) => true,

        (
            Command::Patch {
                kind: ka,
                id: ia,
                props: pa,
            },
            Command::Patch {
                kind: kb,
                id: ib,
                props: pb,
            },
        ) => ka == kb && ia == ib && props_eq_unordered(pa, pb),

        (
            Command::Event {
                kind: ka,
                seq: sa,
                id: ia,
                props: pa,
            },
            Command::Event {
                kind: kb,
                seq: sb,
                id: ib,
                props: pb,
            },
        ) => ka == kb && sa == sb && ia == ib && props_eq_unordered(pa, pb),

        (
            Command::Ack {
                kind: ka,
                seq: sa,
                props: pa,
            },
            Command::Ack {
                kind: kb,
                seq: sb,
                props: pb,
            },
        ) => ka == kb && sa == sb && props_eq_unordered(pa, pb),

        (
            Command::Request {
                kind: ka,
                seq: sa,
                targets: ta,
                props: pa,
            },
            Command::Request {
                kind: kb,
                seq: sb,
                targets: tb,
                props: pb,
            },
        ) => ka == kb && sa == sb && ta == tb && props_eq_unordered(pa, pb),

        (
            Command::Response {
                kind: ka,
                seq: sa,
                props: pa,
                body: ba,
            },
            Command::Response {
                kind: kb,
                seq: sb,
                props: pb,
                body: bb,
            },
        ) => {
            ka == kb
                && sa == sb
                && props_eq_unordered(pa, pb)
                && match (ba, bb) {
                    (None, None) => true,
                    (Some(a), Some(b)) => commands_eq_unordered(a, b),
                    _ => false,
                }
        }

        _ => false,
    }
}

/// Bitmask-based unordered prop comparison.
fn props_eq_unordered(a: &[crate::protocol::Prop<'_>], b: &[crate::protocol::Prop<'_>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.len() > 64 {
        // Fallback for very large prop lists — just sort copies
        let mut a_sorted: Vec<_> = a.iter().map(format_prop).collect();
        let mut b_sorted: Vec<_> = b.iter().map(format_prop).collect();
        a_sorted.sort();
        b_sorted.sort();
        return a_sorted == b_sorted;
    }
    let mut used: u64 = 0;
    for prop_a in a {
        let mut found = false;
        for (j, prop_b) in b.iter().enumerate() {
            if used & (1 << j) == 0 && prop_a == prop_b {
                used |= 1 << j;
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

fn format_prop(p: &crate::protocol::Prop<'_>) -> String {
    format!("{p:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Prop;

    #[test]
    fn equal_commands() {
        assert_eq(
            "+view sidebar class=\"w-64\"",
            "+view sidebar class=\"w-64\"",
        );
    }

    #[test]
    #[should_panic(expected = "structural mismatch")]
    fn different_props() {
        assert_eq(
            "+view sidebar class=\"w-64\"",
            "+view sidebar class=\"w-32\"",
        );
    }

    #[test]
    #[should_panic(expected = "structural mismatch")]
    fn different_command_count() {
        assert_eq("+view a +view b", "+view a");
    }

    #[test]
    fn bytes_convenience() {
        let bytes = b"+view sidebar";
        assert_eq_bytes(bytes, "+view sidebar");
    }

    #[test]
    fn unordered_props_match() {
        let a = [Prop::val("class", "w-64"), Prop::flag("hidden")];
        let b = [Prop::flag("hidden"), Prop::val("class", "w-64")];
        assert!(props_eq_unordered(&a, &b));
    }

    #[test]
    fn auto_strips_apc_framing() {
        assert_eq(
            "\x1b_B\n+view sidebar class=\"w-64\"\n\x1b\\",
            "+view sidebar class=\"w-64\"",
        );
    }

    #[test]
    fn strip_apc_helper() {
        use crate::protocol::strip_apc;
        assert_eq!(
            strip_apc("\x1b_B\n+view sidebar\n\x1b\\"),
            "\n+view sidebar"
        );
        // No framing — returned unchanged
        assert_eq!(strip_apc("+view sidebar"), "+view sidebar");
    }

    #[test]
    fn unordered_props_mismatch() {
        let a = [Prop::val("class", "w-64"), Prop::flag("hidden")];
        let b = [Prop::val("class", "w-32"), Prop::flag("hidden")];
        assert!(!props_eq_unordered(&a, &b));
    }
}
