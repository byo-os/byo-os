//! IR → canonical BYO string serializer.
//!
//! Takes `Vec<IrCommand>` (from the existing `parse::parse()`), produces
//! a canonical `String` representation. Only accepts literals — compile
//! error on interpolation, conditionals, or loops.

use crate::ir::{IrCommand, IrProp, IrValue};

/// Serialize a list of IR commands to a canonical BYO string.
///
/// Returns `Err` if any command contains interpolation, conditionals, or loops.
pub fn serialize_commands(cmds: &[IrCommand]) -> Result<String, String> {
    let mut out = String::new();
    serialize_commands_into(cmds, &mut out, "")?;
    Ok(out)
}

fn serialize_commands_into(
    cmds: &[IrCommand],
    out: &mut String,
    indent: &str,
) -> Result<(), String> {
    for cmd in cmds {
        serialize_command(cmd, out, indent)?;
    }
    Ok(())
}

fn serialize_command(cmd: &IrCommand, out: &mut String, indent: &str) -> Result<(), String> {
    match cmd {
        IrCommand::Upsert {
            kind,
            id,
            props,
            children,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('+');
            out.push_str(&expect_literal(kind, "type")?);
            out.push(' ');
            out.push_str(&expect_literal(id, "id")?);
            serialize_props(props, out)?;
            if let Some(children) = children {
                out.push_str(" {");
                let child_indent = format!("{indent}  ");
                serialize_commands_into(children, out, &child_indent)?;
                out.push('\n');
                out.push_str(indent);
                out.push('}');
            }
        }
        IrCommand::Destroy { kind, id } => {
            out.push('\n');
            out.push_str(indent);
            out.push('-');
            out.push_str(&expect_literal(kind, "type")?);
            out.push(' ');
            out.push_str(&expect_literal(id, "id")?);
        }
        IrCommand::Patch {
            kind,
            id,
            props,
            children,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('@');
            out.push_str(&expect_literal(kind, "type")?);
            out.push(' ');
            out.push_str(&expect_literal(id, "id")?);
            serialize_props(props, out)?;
            if let Some(children) = children {
                out.push_str(" {");
                let child_indent = format!("{indent}  ");
                serialize_commands_into(children, out, &child_indent)?;
                out.push('\n');
                out.push_str(indent);
                out.push('}');
            }
        }
        IrCommand::Event {
            kind,
            seq,
            id,
            props,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('!');
            out.push_str(&expect_literal(kind, "event kind")?);
            out.push(' ');
            out.push_str(&expect_literal(seq, "sequence number")?);
            out.push(' ');
            out.push_str(&expect_literal(id, "id")?);
            serialize_props(props, out)?;
        }
        IrCommand::Ack { kind, seq, props } => {
            out.push('\n');
            out.push_str(indent);
            out.push_str("!ack ");
            out.push_str(&expect_literal(kind, "event kind")?);
            out.push(' ');
            out.push_str(&expect_literal(seq, "sequence number")?);
            serialize_props(props, out)?;
        }
        IrCommand::Pragma { kind, targets } => {
            out.push('\n');
            out.push_str(indent);
            out.push('#');
            let kind_str = expect_literal(kind, "pragma kind")?;
            out.push_str(&kind_str);
            match kind_str.as_str() {
                "claim" | "unclaim" | "observe" | "unobserve" | "handle" | "unhandle" => {
                    out.push(' ');
                    for (i, t) in targets.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&expect_literal(t, "target")?);
                    }
                }
                "redirect" => {
                    if let Some(t) = targets.first() {
                        out.push(' ');
                        out.push_str(&expect_literal(t, "target")?);
                    }
                }
                "unredirect" => {
                    // no targets
                }
                _ => {
                    if !targets.is_empty() {
                        out.push(' ');
                        for (i, t) in targets.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            out.push_str(&expect_literal(t, "target")?);
                        }
                    }
                }
            }
        }
        IrCommand::Request {
            kind,
            seq,
            targets,
            props,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('?');
            let kind_str = expect_literal(kind, "request kind")?;
            out.push_str(&kind_str);
            out.push(' ');
            out.push_str(&expect_literal(seq, "sequence number")?);
            out.push(' ');
            if let Some(t) = targets.first() {
                out.push_str(&expect_literal(t, "target")?);
            }
            serialize_props(props, out)?;
        }
        IrCommand::Response {
            kind,
            seq,
            props,
            children,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('.');
            out.push_str(&expect_literal(kind, "response kind")?);
            out.push(' ');
            out.push_str(&expect_literal(seq, "sequence number")?);
            serialize_props(props, out)?;
            if let Some(children) = children {
                out.push_str(" {");
                let child_indent = format!("{indent}  ");
                serialize_commands_into(children, out, &child_indent)?;
                out.push('\n');
                out.push_str(indent);
                out.push('}');
            }
        }
        IrCommand::Message {
            kind,
            target,
            props,
            children,
        } => {
            out.push('\n');
            out.push_str(indent);
            out.push('.');
            out.push_str(&expect_literal(kind, "message kind")?);
            out.push(' ');
            out.push_str(&expect_literal(target, "message target")?);
            serialize_props(props, out)?;
            if let Some(children) = children {
                out.push_str(" {");
                let child_indent = format!("{indent}  ");
                serialize_commands_into(children, out, &child_indent)?;
                out.push('\n');
                out.push_str(indent);
                out.push('}');
            }
        }
        IrCommand::Conditional { .. } => {
            return Err("byo_str!/byo_assert_eq! does not support `if` conditionals".to_string());
        }
        IrCommand::ForLoop { .. } => {
            return Err("byo_str!/byo_assert_eq! does not support `for` loops".to_string());
        }
        IrCommand::Match { .. } => {
            return Err("byo_str!/byo_assert_eq! does not support `match` expressions".to_string());
        }
        IrCommand::SlotBlock { name, children } => {
            out.push('\n');
            out.push_str(indent);
            out.push('{');
            out.push_str(&expect_literal(name, "slot name")?);
            let child_indent = format!("{indent}  ");
            serialize_commands_into(children, out, &child_indent)?;
            out.push('\n');
            out.push_str(indent);
            out.push('}');
        }
    }
    Ok(())
}

fn serialize_props(props: &[IrProp], out: &mut String) -> Result<(), String> {
    for prop in props {
        match prop {
            IrProp::Value { key, value } => {
                let v = expect_literal(value, "prop value")?;
                out.push(' ');
                out.push_str(key);
                out.push('=');
                write_quoted_value(&v, out);
            }
            IrProp::Boolean { key } => {
                out.push(' ');
                out.push_str(key);
            }
            IrProp::Remove { key } => {
                out.push_str(" ~");
                out.push_str(key);
            }
            IrProp::Spread { .. } => {
                return Err(
                    "spread props ({..expr}) are not supported in byo_str!/byo_assert_eq! \
                     (compile-time only); use byo!() or byo_write!() instead"
                        .to_string(),
                );
            }
            IrProp::Conditional { .. } => {
                return Err(
                    "byo_str!/byo_assert_eq! does not support conditional props".to_string()
                );
            }
        }
    }
    Ok(())
}

/// Write a value, quoting it in a way the parser can round-trip.
fn write_quoted_value(value: &str, out: &mut String) {
    // Use the same quoting strategy as the emitter:
    // - Bare if safe
    // - Double-quote if no double quotes
    // - Single-quote if no single quotes
    // - Double-quote with escaping otherwise
    if is_bare(value) {
        out.push_str(value);
    } else if !value.contains('"') {
        out.push('"');
        out.push_str(value);
        out.push('"');
    } else if !value.contains('\'') {
        out.push('\'');
        out.push_str(value);
        out.push('\'');
    } else {
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\0' => out.push_str("\\0"),
                _ => out.push(ch),
            }
        }
        out.push('"');
    }
}

/// Same logic as the emitter's `is_bare`.
fn is_bare(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| {
            !b.is_ascii_whitespace()
                && b != b'{'
                && b != b'}'
                && b != b'='
                && b != b'"'
                && b != b'\''
                && b != b'~'
                && b != b'\\'
                && b != b','
        })
}

fn expect_literal(val: &IrValue, context: &str) -> Result<String, String> {
    match val {
        IrValue::Literal(s) => Ok(s.clone()),
        IrValue::Interpolation(_) => Err(format!(
            "byo_str!/byo_assert_eq! does not support interpolation in {context}"
        )),
    }
}
