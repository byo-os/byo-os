//! Recursive descent parser for the `byo!`/`byo_write!` macro DSL.
//!
//! Transforms a `proc_macro2::TokenStream` into an IR ([`IrCommand`] tree).
//! The DSL mirrors the BYO/OS wire protocol syntax but operates on Rust
//! tokens instead of raw bytes, adding expression interpolation (`{expr}`),
//! conditionals (`if`/`else`), and loops (`for`).
//!
//! # Span-based compound name gluing
//!
//! Rust's tokenizer splits names like `notes-app:save` into multiple tokens.
//! When `real_spans` is `true` (proc macro context), the parser uses span
//! adjacency to glue them back together, with position-specific punctuation
//! sets matching the grammar:
//!
//! | Position  | Allowed join chars | Grammar pattern                  |
//! |-----------|--------------------|----------------------------------|
//! | Type      | `.` `-`            | `[a-zA-Z][a-zA-Z0-9._-]*`       |
//! | ID        | `:` `-`            | `[a-zA-Z_][a-zA-Z0-9_:-]*`      |
//! | Prop name | `-`                | `[a-zA-Z][a-zA-Z0-9_-]*`        |
//! | Value     | `-` `:` `.` `/`    | unquoted bare values             |

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::ir::{IrCommand, IrProp, IrValue};

/// Check if two tokens are adjacent (no whitespace between them) by
/// comparing their span positions. Only meaningful when `real_spans` is true.
fn spans_adjacent(prev: &TokenTree, next: &TokenTree) -> bool {
    let end = prev.span().end();
    let start = next.span().start();
    end.line == start.line && end.column == start.column
}

/// Parser state wrapping a peekable iterator over `TokenTree`.
struct Parser {
    tokens: Vec<TokenTree>,
    pos: usize,
    /// Whether token spans carry real source locations (true in proc macro
    /// context, false in tests using `quote!` where all spans are dummy).
    real_spans: bool,
}

impl Parser {
    fn new(stream: TokenStream, real_spans: bool) -> Self {
        Self {
            tokens: stream.into_iter().collect(),
            pos: 0,
            real_spans,
        }
    }

    fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<TokenTree> {
        let tt = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        Some(tt)
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Check if the next token is a specific punctuation character.
    fn peek_punct(&self, ch: char) -> bool {
        matches!(self.peek(), Some(TokenTree::Punct(p)) if p.as_char() == ch)
    }

    /// Check if the next token is a specific keyword (ident).
    fn peek_keyword(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(TokenTree::Ident(id)) if id == kw)
    }

    /// Consume a punctuation character, returning true if it matched.
    fn eat_punct(&mut self, ch: char) -> bool {
        if self.peek_punct(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consume a keyword, returning true if it matched.
    fn eat_keyword(&mut self, kw: &str) -> bool {
        if self.peek_keyword(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Extend a name by consuming adjacent punctuation + ident/number tokens,
    /// but only punctuation characters listed in `allowed_punct`.
    ///
    /// Each grammar position has its own set of allowed join characters:
    /// - **Type**: `.` and `-` (grammar: `[a-zA-Z][a-zA-Z0-9._-]*`)
    /// - **ID**: `:` and `-` (grammar: `[a-zA-Z_][a-zA-Z0-9_:-]*`)
    /// - **Prop name**: `-` only (grammar: `[a-zA-Z][a-zA-Z0-9_-]*`)
    /// - **Value**: `-`, `:`, `.`, `/` (unquoted values are permissive)
    fn extend_compound(&mut self, mut name: String, allowed_punct: &[char]) -> String {
        loop {
            if self.pos >= self.tokens.len() {
                break;
            }
            let is_allowed_punct = matches!(
                &self.tokens[self.pos],
                TokenTree::Punct(p) if allowed_punct.contains(&p.as_char())
            );
            if !is_allowed_punct
                || !spans_adjacent(&self.tokens[self.pos - 1], &self.tokens[self.pos])
            {
                break;
            }
            let ch = match &self.tokens[self.pos] {
                TokenTree::Punct(p) => p.as_char(),
                _ => unreachable!(),
            };
            // The punct must be followed by an adjacent ident or integer literal
            if self.pos + 1 >= self.tokens.len()
                || !spans_adjacent(&self.tokens[self.pos], &self.tokens[self.pos + 1])
            {
                break;
            }
            match &self.tokens[self.pos + 1] {
                TokenTree::Ident(id) => {
                    name.push(ch);
                    name.push_str(&id.to_string());
                    self.pos += 2;
                }
                TokenTree::Literal(lit) => {
                    let s = lit.to_string();
                    // Only glue numeric literals (not strings)
                    if !s.starts_with('"') && !s.starts_with('\'') {
                        name.push(ch);
                        name.push_str(&s);
                        self.pos += 2;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        name
    }

    /// Peek ahead from an `if` keyword to find the brace body and check whether
    /// it contains commands (vs props). Used to disambiguate conditional props
    /// from conditional commands in prop-parsing context.
    fn if_body_is_commands(&self) -> bool {
        let mut i = self.pos + 1; // skip past `if`
        while i < self.tokens.len() {
            if let TokenTree::Group(g) = &self.tokens[i]
                && g.delimiter() == Delimiter::Brace
            {
                let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                return match inner.first() {
                    Some(TokenTree::Punct(p)) => {
                        matches!(p.as_char(), '+' | '-' | '@' | '!' | '?' | '.')
                    }
                    Some(TokenTree::Ident(id)) => id == "if" || id == "for",
                    _ => false,
                };
            }
            i += 1;
        }
        false
    }

    /// Parse a name in a specific grammar position. `context` is used for
    /// error messages, `glue` lists the punctuation characters that should
    /// be joined via span-adjacency gluing.
    fn parse_name_in(&mut self, context: &str, glue: &[char]) -> Result<IrValue, String> {
        match self.peek() {
            Some(TokenTree::Ident(id)) => {
                let s = id.to_string();
                self.pos += 1;
                if self.real_spans {
                    Ok(IrValue::Literal(self.extend_compound(s, glue)))
                } else {
                    Ok(IrValue::Literal(s))
                }
            }
            Some(TokenTree::Literal(lit)) => {
                let s = lit.to_string();
                self.pos += 1;
                // String literal: strip quotes
                if (s.starts_with('"') && s.ends_with('"'))
                    || (s.starts_with('\'') && s.ends_with('\''))
                {
                    Ok(IrValue::Literal(s[1..s.len() - 1].to_string()))
                } else {
                    // Numeric literal or other — use as-is
                    Ok(IrValue::Literal(s))
                }
            }
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                let stream = g.stream();
                self.pos += 1;
                Ok(IrValue::Interpolation(stream))
            }
            other => Err(format!(
                "expected {context} (ident, string literal, or {{expr}}), found {:?}",
                other
            )),
        }
    }

    /// Parse a type name. Glues `.` and `-` (grammar: `[a-zA-Z][a-zA-Z0-9._-]*`).
    fn parse_type(&mut self) -> Result<IrValue, String> {
        self.parse_name_in("type name", &['.', '-'])
    }

    /// Parse an object ID. Glues `:` and `-` (grammar: `[a-zA-Z_][a-zA-Z0-9_:-]*`).
    /// Accepts `_` for anonymous objects.
    fn parse_id(&mut self) -> Result<IrValue, String> {
        self.parse_name_in("id", &[':', '-'])
    }

    /// Parse an object ID, rejecting `_` (anonymous). Used by destroy and patch.
    fn parse_named_id(&mut self) -> Result<IrValue, String> {
        let id = self.parse_id()?;
        if matches!(&id, IrValue::Literal(s) if s == "_") {
            return Err(
                "anonymous id '_' is not allowed in this position (only '+' supports '_')"
                    .to_string(),
            );
        }
        Ok(id)
    }

    /// Parse a prop value (after `=`).
    /// Can be a bare ident, string literal, numeric literal, or `{expr}`.
    /// Glues `-`, `:`, `.`, `/` for compound values (e.g. `bg-zinc-700/50`).
    fn parse_value(&mut self) -> Result<IrValue, String> {
        match self.peek() {
            Some(TokenTree::Ident(id)) => {
                let s = id.to_string();
                self.pos += 1;
                if self.real_spans {
                    Ok(IrValue::Literal(
                        self.extend_compound(s, &['-', ':', '.', '/']),
                    ))
                } else {
                    Ok(IrValue::Literal(s))
                }
            }
            Some(TokenTree::Literal(lit)) => {
                let s = lit.to_string();
                self.pos += 1;
                if (s.starts_with('"') && s.ends_with('"'))
                    || (s.starts_with('\'') && s.ends_with('\''))
                {
                    Ok(IrValue::Literal(s[1..s.len() - 1].to_string()))
                } else {
                    // Numeric literal — use the raw string representation
                    Ok(IrValue::Literal(s))
                }
            }
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                let stream = g.stream();
                self.pos += 1;
                Ok(IrValue::Interpolation(stream))
            }
            other => Err(format!(
                "expected value (ident, literal, or {{expr}}), found {:?}",
                other
            )),
        }
    }

    /// Parse props until we hit a `{` children block, another command operator,
    /// `if`/`for` keyword at command level, or end of input.
    /// Also handles `if` in prop position for conditional props.
    fn parse_props(&mut self) -> Result<(Vec<IrProp>, Option<Vec<IrCommand>>), String> {
        let mut props = Vec::new();
        let mut children = None;

        loop {
            // Check for end conditions
            if self.is_empty() {
                break;
            }

            // `{` as children block — only when NOT preceded by `=`
            if let Some(TokenTree::Group(g)) = self.peek()
                && g.delimiter() == Delimiter::Brace
            {
                let stream = g.stream();
                self.pos += 1;
                let mut child_parser = Parser::new(stream, self.real_spans);
                children = Some(child_parser.parse_commands()?);
                break;
            }

            // Command operators signal end of this command's props
            if self.peek_punct('+')
                || self.peek_punct('-')
                || self.peek_punct('@')
                || self.peek_punct('!')
                || self.peek_punct('?')
                || self.peek_punct('.')
            {
                break;
            }

            // `for` in command position — not a prop
            if self.peek_keyword("for") {
                break;
            }

            // `if` — conditional props or conditional command?
            // Peek inside the brace body: if it starts with a command operator
            // or control flow keyword, it's a conditional command (break out).
            if self.peek_keyword("if") && self.if_body_is_commands() {
                break;
            }

            if self.peek_keyword("if") {
                self.pos += 1;
                let condition = self.collect_until_brace()?;
                let then_props = self.parse_prop_block()?;
                let else_props = if self.eat_keyword("else") {
                    Some(self.parse_prop_block()?)
                } else {
                    None
                };
                props.push(IrProp::Conditional {
                    condition,
                    then_props,
                    else_props,
                });
                continue;
            }

            // `~key` — remove prop (glue `-` for hyphenated names)
            if self.peek_punct('~') {
                self.pos += 1;
                let mut key = self.expect_ident("expected prop name after ~")?;
                if self.real_spans {
                    key = self.extend_compound(key, &['-']);
                }
                props.push(IrProp::Remove { key });
                continue;
            }

            // Ident — could be `key=value` or bare boolean flag
            // Prop names glue `-` only (grammar: `[a-zA-Z][a-zA-Z0-9_-]*`)
            if let Some(TokenTree::Ident(_)) = self.peek() {
                let mut key = self.expect_ident("expected prop name")?;
                if self.real_spans {
                    key = self.extend_compound(key, &['-']);
                }
                if self.eat_punct('=') {
                    let value = self.parse_value()?;
                    props.push(IrProp::Value { key, value });
                } else {
                    props.push(IrProp::Boolean { key });
                }
                continue;
            }

            // Nothing we recognize in prop position — stop
            break;
        }

        Ok((props, children))
    }

    /// Parse a brace-delimited block of props (for conditional prop blocks).
    fn parse_prop_block(&mut self) -> Result<Vec<IrProp>, String> {
        match self.next() {
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                let mut inner = Parser::new(g.stream(), self.real_spans);
                let mut props = Vec::new();
                while !inner.is_empty() {
                    // `~key`
                    if inner.peek_punct('~') {
                        inner.pos += 1;
                        let mut key = inner.expect_ident("expected prop name after ~")?;
                        if inner.real_spans {
                            key = inner.extend_compound(key, &['-']);
                        }
                        props.push(IrProp::Remove { key });
                        continue;
                    }
                    // `if` nested conditional
                    if inner.peek_keyword("if") {
                        inner.pos += 1;
                        let condition = inner.collect_until_brace()?;
                        let then_props = inner.parse_prop_block()?;
                        let else_props = if inner.eat_keyword("else") {
                            Some(inner.parse_prop_block()?)
                        } else {
                            None
                        };
                        props.push(IrProp::Conditional {
                            condition,
                            then_props,
                            else_props,
                        });
                        continue;
                    }
                    // ident — key=value or boolean
                    let mut key = inner.expect_ident("expected prop name in conditional block")?;
                    if inner.real_spans {
                        key = inner.extend_compound(key, &['-']);
                    }
                    if inner.eat_punct('=') {
                        let value = inner.parse_value()?;
                        props.push(IrProp::Value { key, value });
                    } else {
                        props.push(IrProp::Boolean { key });
                    }
                }
                Ok(props)
            }
            other => Err(format!("expected {{ props }}, found {:?}", other)),
        }
    }

    /// Collect all tokens until the next `Group(Brace)`, producing a TokenStream
    /// (used for `if` conditions and `for` iterators).
    fn collect_until_brace(&mut self) -> Result<TokenStream, String> {
        let mut collected = Vec::new();
        loop {
            match self.peek() {
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                    break;
                }
                Some(_) => {
                    collected.push(self.next().unwrap());
                }
                None => return Err("unexpected end of input, expected {".to_string()),
            }
        }
        if collected.is_empty() {
            return Err("expected condition tokens before {".to_string());
        }
        Ok(collected.into_iter().collect())
    }

    /// Collect tokens for a `for` pattern: everything until the `in` keyword.
    fn collect_until_in(&mut self) -> Result<TokenStream, String> {
        let mut collected = Vec::new();
        loop {
            if self.peek_keyword("in") {
                self.pos += 1; // consume `in`
                break;
            }
            match self.next() {
                Some(tt) => collected.push(tt),
                None => return Err("unexpected end of input, expected `in`".to_string()),
            }
        }
        if collected.is_empty() {
            return Err("expected pattern before `in`".to_string());
        }
        Ok(collected.into_iter().collect())
    }

    /// Expect and consume an ident, returning its string value.
    fn expect_ident(&mut self, msg: &str) -> Result<String, String> {
        match self.next() {
            Some(TokenTree::Ident(id)) => {
                let mut name = id.to_string();
                // Strip r# prefix (raw identifier) for prop names
                if let Some(stripped) = name.strip_prefix("r#") {
                    name = stripped.to_string();
                }
                Ok(name)
            }
            other => Err(format!("{msg}, found {:?}", other)),
        }
    }

    /// Parse a single command.
    fn parse_command(&mut self) -> Result<IrCommand, String> {
        // `if` — conditional command
        if self.eat_keyword("if") {
            let condition = self.collect_until_brace()?;
            let then_body = self.parse_command_block()?;
            let else_body = if self.eat_keyword("else") {
                Some(self.parse_command_block()?)
            } else {
                None
            };
            return Ok(IrCommand::Conditional {
                condition,
                then_cmds: then_body,
                else_cmds: else_body,
            });
        }

        // `for` — loop
        if self.eat_keyword("for") {
            let pat = self.collect_until_in()?;
            let iter = self.collect_until_brace()?;
            let body = self.parse_command_block()?;
            return Ok(IrCommand::ForLoop { pat, iter, body });
        }

        // `+` — upsert
        if self.eat_punct('+') {
            let kind = self.parse_type()?;
            let id = self.parse_id()?;
            let (props, children) = self.parse_props()?;
            return Ok(IrCommand::Upsert {
                kind,
                id,
                props,
                children,
            });
        }

        // `-` — destroy (anonymous `_` not allowed)
        if self.eat_punct('-') {
            let kind = self.parse_type()?;
            let id = self.parse_named_id()?;
            return Ok(IrCommand::Destroy { kind, id });
        }

        // `@` — patch (anonymous `_` not allowed)
        if self.eat_punct('@') {
            let kind = self.parse_type()?;
            let id = self.parse_named_id()?;
            let (props, children) = self.parse_props()?;
            return Ok(IrCommand::Patch {
                kind,
                id,
                props,
                children,
            });
        }

        // `!` — event/ack
        if self.eat_punct('!') {
            return self.parse_event_command();
        }

        // `?` — request (sub, unsub, expand, custom)
        if self.eat_punct('?') {
            return self.parse_request_command();
        }

        // `.` — response (expand, custom)
        if self.eat_punct('.') {
            return self.parse_response_command();
        }

        Err(format!(
            "expected command (+, -, @, !, ?, ., if, for), found {:?}",
            self.peek()
        ))
    }

    /// Parse an event command after the `!` has been consumed.
    fn parse_event_command(&mut self) -> Result<IrCommand, String> {
        // Check for ack keyword
        if self.peek_keyword("ack") {
            self.pos += 1;
            let kind = self.parse_type()?;
            let seq = self.parse_seq()?;
            let (props, children) = self.parse_props()?;
            if children.is_some() {
                return Err("'!ack' cannot have a children block".to_string());
            }
            return Ok(IrCommand::Ack { kind, seq, props });
        }

        // Generic event: !kind seq id props...
        let kind = self.parse_type()?;
        let seq = self.parse_seq()?;
        let id = self.parse_id()?;
        let (props, children) = self.parse_props()?;
        if children.is_some() {
            return Err("events cannot have a children block".to_string());
        }
        Ok(IrCommand::Event {
            kind,
            seq,
            id,
            props,
        })
    }

    /// Parse a request command after the `?` has been consumed.
    fn parse_request_command(&mut self) -> Result<IrCommand, String> {
        // claim/unclaim/observe/unobserve: ?claim seq type, etc.
        if self.peek_keyword("claim")
            || self.peek_keyword("unclaim")
            || self.peek_keyword("observe")
            || self.peek_keyword("unobserve")
        {
            let kind_str = self.expect_ident("expected request kind")?;
            let kind = IrValue::Literal(kind_str);
            let seq = self.parse_seq()?;
            let target = self.parse_type()?;
            return Ok(IrCommand::Request {
                kind,
                seq,
                target,
                props: Vec::new(),
            });
        }

        // Generic request: ?kind seq target props...
        let kind = self.parse_type()?;
        let seq = self.parse_seq()?;
        let target = self.parse_id()?;
        let (props, children) = self.parse_props()?;
        if children.is_some() {
            return Err("requests cannot have a children block".to_string());
        }
        Ok(IrCommand::Request {
            kind,
            seq,
            target,
            props,
        })
    }

    /// Parse a response command after the `.` has been consumed.
    fn parse_response_command(&mut self) -> Result<IrCommand, String> {
        let kind = self.parse_type()?;
        let seq = self.parse_seq()?;
        let (props, children) = self.parse_props()?;
        Ok(IrCommand::Response {
            kind,
            seq,
            props,
            children,
        })
    }

    /// Parse a sequence number (integer literal or `{expr}`).
    fn parse_seq(&mut self) -> Result<IrValue, String> {
        match self.peek() {
            Some(TokenTree::Literal(lit)) => {
                let s = lit.to_string();
                self.pos += 1;
                Ok(IrValue::Literal(s))
            }
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                let stream = g.stream();
                self.pos += 1;
                Ok(IrValue::Interpolation(stream))
            }
            other => Err(format!(
                "expected sequence number (integer or {{expr}}), found {:?}",
                other
            )),
        }
    }

    /// Parse a brace-delimited block of commands.
    fn parse_command_block(&mut self) -> Result<Vec<IrCommand>, String> {
        match self.next() {
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                let mut inner = Parser::new(g.stream(), self.real_spans);
                inner.parse_commands()
            }
            other => Err(format!("expected {{ commands }}, found {:?}", other)),
        }
    }

    /// Parse all commands until end of stream.
    fn parse_commands(&mut self) -> Result<Vec<IrCommand>, String> {
        let mut cmds = Vec::new();
        while !self.is_empty() {
            cmds.push(self.parse_command()?);
        }
        Ok(cmds)
    }
}

/// Parse a `TokenStream` into a list of IR commands.
///
/// When `real_spans` is `true` (proc macro context), the parser uses span
/// positions to glue adjacent tokens into compound names like `notes-app:save`.
/// When `false` (tests using `quote!`), span-based gluing is disabled.
pub fn parse(input: TokenStream, real_spans: bool) -> Result<Vec<IrCommand>, String> {
    let mut parser = Parser::new(input, real_spans);
    parser.parse_commands()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parse_simple_upsert() {
        let input = quote! { +view sidebar };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(
            matches!(&cmds[0], IrCommand::Upsert { kind: IrValue::Literal(k), id: IrValue::Literal(i), props, children } if k == "view" && i == "sidebar" && props.is_empty() && children.is_none())
        );
    }

    #[test]
    fn parse_upsert_with_props() {
        let input = quote! { +view sidebar class="w-64" hidden };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Upsert { props, .. } => {
                assert_eq!(props.len(), 2);
                assert!(
                    matches!(&props[0], IrProp::Value { key, value: IrValue::Literal(v) } if key == "class" && v == "w-64")
                );
                assert!(matches!(&props[1], IrProp::Boolean { key } if key == "hidden"));
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn parse_upsert_with_children() {
        let input = quote! {
            +view root {
                +text label content="Hello"
            }
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Upsert {
                children: Some(ch), ..
            } => {
                assert_eq!(ch.len(), 1);
            }
            _ => panic!("expected Upsert with children"),
        }
    }

    #[test]
    fn parse_destroy() {
        let input = quote! { -view sidebar };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(
            matches!(&cmds[0], IrCommand::Destroy { kind: IrValue::Literal(k), id: IrValue::Literal(i) } if k == "view" && i == "sidebar")
        );
    }

    #[test]
    fn parse_patch() {
        let input = quote! { @view sidebar hidden ~tooltip };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Patch { props, .. } => {
                assert_eq!(props.len(), 2);
                assert!(matches!(&props[0], IrProp::Boolean { key } if key == "hidden"));
                assert!(matches!(&props[1], IrProp::Remove { key } if key == "tooltip"));
            }
            _ => panic!("expected Patch"),
        }
    }

    #[test]
    fn parse_event() {
        let input = quote! { !click 0 save };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(
            matches!(&cmds[0], IrCommand::Event { kind: IrValue::Literal(k), seq: IrValue::Literal(s), id: IrValue::Literal(i), .. } if k == "click" && s == "0" && i == "save")
        );
    }

    #[test]
    fn parse_ack() {
        let input = quote! { !ack click 0 handled=true };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Ack {
                kind: IrValue::Literal(k),
                seq: IrValue::Literal(s),
                props,
            } => {
                assert_eq!(k, "click");
                assert_eq!(s, "0");
                assert_eq!(props.len(), 1);
            }
            _ => panic!("expected Ack"),
        }
    }

    #[test]
    fn parse_claim_unclaim() {
        let input = quote! { ?claim 0 button ?unclaim 1 slider };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(
            matches!(&cmds[0], IrCommand::Request { kind: IrValue::Literal(k), seq: IrValue::Literal(s), target: IrValue::Literal(t), .. } if k == "claim" && s == "0" && t == "button")
        );
        assert!(
            matches!(&cmds[1], IrCommand::Request { kind: IrValue::Literal(k), seq: IrValue::Literal(s), target: IrValue::Literal(t), .. } if k == "unclaim" && s == "1" && t == "slider")
        );
    }

    #[test]
    fn parse_observe_unobserve() {
        let input = quote! { ?observe 0 view ?unobserve 1 text };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(
            matches!(&cmds[0], IrCommand::Request { kind: IrValue::Literal(k), seq: IrValue::Literal(s), target: IrValue::Literal(t), .. } if k == "observe" && s == "0" && t == "view")
        );
        assert!(
            matches!(&cmds[1], IrCommand::Request { kind: IrValue::Literal(k), seq: IrValue::Literal(s), target: IrValue::Literal(t), .. } if k == "unobserve" && s == "1" && t == "text")
        );
    }

    #[test]
    fn parse_request_expand() {
        let input = quote! { ?expand 0 save kind=button };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Request {
                kind: IrValue::Literal(k),
                seq: IrValue::Literal(s),
                target: IrValue::Literal(t),
                props,
            } => {
                assert_eq!(k, "expand");
                assert_eq!(s, "0");
                assert_eq!(t, "save");
                assert_eq!(props.len(), 1);
            }
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn parse_response_expand() {
        let input = quote! {
            .expand 0 {
                +view root
            }
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Response {
                kind: IrValue::Literal(k),
                seq: IrValue::Literal(s),
                children: Some(ch),
                ..
            } => {
                assert_eq!(k, "expand");
                assert_eq!(s, "0");
                assert_eq!(ch.len(), 1);
            }
            _ => panic!("expected Response with children"),
        }
    }

    #[test]
    fn parse_response_no_body() {
        let input = quote! { .status 0 ok=true };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Response {
                kind: IrValue::Literal(k),
                seq: IrValue::Literal(s),
                props,
                children: None,
            } => {
                assert_eq!(k, "status");
                assert_eq!(s, "0");
                assert_eq!(props.len(), 1);
            }
            _ => panic!("expected Response without children"),
        }
    }

    #[test]
    fn parse_if_command() {
        let input = quote! {
            if show_save {
                +button save label="Save"
            }
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Conditional {
                then_cmds,
                else_cmds,
                ..
            } => {
                assert_eq!(then_cmds.len(), 1);
                assert!(else_cmds.is_none());
            }
            _ => panic!("expected Conditional"),
        }
    }

    #[test]
    fn parse_if_else_command() {
        let input = quote! {
            if loading {
                +view spinner
            } else {
                +view content
            }
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::Conditional {
                then_cmds,
                else_cmds: Some(ec),
                ..
            } => {
                assert_eq!(then_cmds.len(), 1);
                assert_eq!(ec.len(), 1);
            }
            _ => panic!("expected Conditional with else"),
        }
    }

    #[test]
    fn parse_for_loop() {
        let input = quote! {
            for item in &items {
                +view item
            }
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            IrCommand::ForLoop { body, .. } => {
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected ForLoop"),
        }
    }

    #[test]
    fn parse_multiple_commands() {
        let input = quote! {
            +view a
            +view b
            -view c
        };
        let cmds = parse(input, false).unwrap();
        assert_eq!(cmds.len(), 3);
    }

    #[test]
    fn parse_interpolated_value() {
        let input = quote! { +view sidebar class={cls} };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert { props, .. } => {
                assert!(
                    matches!(&props[0], IrProp::Value { key, value: IrValue::Interpolation(_) } if key == "class")
                );
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn parse_interpolated_id() {
        let input = quote! { +view {my_id} class="foo" };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert {
                id: IrValue::Interpolation(_),
                ..
            } => {}
            _ => panic!("expected interpolated id"),
        }
    }

    #[test]
    fn parse_conditional_props() {
        let input =
            quote! { +button save label="Save" if disabled { disabled class="opacity-50" } };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert { props, .. } => {
                assert_eq!(props.len(), 2); // label, Conditional
                assert!(
                    matches!(&props[1], IrProp::Conditional { then_props, else_props: None, .. } if then_props.len() == 2)
                );
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn parse_anonymous_id() {
        let input = quote! { +view _ };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert {
                id: IrValue::Literal(i),
                ..
            } => assert_eq!(i, "_"),
            _ => panic!("expected Upsert with _ id"),
        }
    }

    #[test]
    fn parse_string_literal_type() {
        let input = quote! { +"org.example.Widget" myid };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert {
                kind: IrValue::Literal(k),
                ..
            } => assert_eq!(k, "org.example.Widget"),
            _ => panic!("expected Upsert with string type"),
        }
    }

    /// Helper: assert that parsing fails with an error containing `needle`.
    fn assert_parse_err(input: TokenStream, needle: &str) {
        match parse(input, false) {
            Ok(_) => panic!("expected parse error containing {needle:?}"),
            Err(e) => assert!(e.contains(needle), "error {e:?} did not contain {needle:?}"),
        }
    }

    #[test]
    fn anon_id_rejected_in_destroy() {
        assert_parse_err(quote! { -view _ }, "anonymous id '_'");
    }

    #[test]
    fn anon_id_rejected_in_patch() {
        assert_parse_err(quote! { @view _ hidden }, "anonymous id '_'");
    }

    #[test]
    fn anon_id_allowed_in_upsert() {
        let input = quote! { +view _ };
        let cmds = parse(input, false).unwrap();
        match &cmds[0] {
            IrCommand::Upsert {
                id: IrValue::Literal(i),
                ..
            } => assert_eq!(i, "_"),
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn event_children_rejected() {
        assert_parse_err(quote! { !click 0 save { +view child } }, "children block");
    }

    #[test]
    fn ack_children_rejected() {
        assert_parse_err(quote! { !ack click 0 { +view child } }, "children block");
    }
}
