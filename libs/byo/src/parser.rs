//! Parser for BYO/OS protocol payloads.
//!
//! Parses a `&str` payload (the content between `ESC _ B` and `ESC \`)
//! into a sequence of [`Command`] values. Internally uses the
//! [lexer](crate::lexer) to tokenize first, then performs recursive
//! descent over the token stream.
//!
//! # Example
//!
//! ```
//! use byo::parser::parse;
//! use byo::protocol::{Command, Prop};
//!
//! let cmds = parse("+view sidebar class=\"w-64\"").unwrap();
//! assert!(matches!(&cmds[0], Command::Upsert { kind: "view", id: "sidebar", .. }));
//! ```

use std::borrow::Cow;

use crate::lexer::{ParseError, ParseErrorKind, Span, Spanned, Token, tokenize};
use crate::protocol::{Command, EventKind, Prop};

/// Parses a BYO/OS protocol payload string into a sequence of commands.
///
/// The input should be the payload content between `ESC _ B` and `ESC \`
/// (not including the protocol identifier byte).
///
/// Children blocks (`{ ... }`) are flattened into [`Command::Push`] and
/// [`Command::Pop`] in the output stream:
///
/// ```
/// use byo::parser::parse;
/// use byo::protocol::Command;
///
/// let cmds = parse("+view root { +text child }").unwrap();
/// assert!(matches!(&cmds[0], Command::Upsert { kind: "view", id: "root", .. }));
/// assert!(matches!(&cmds[1], Command::Push));
/// assert!(matches!(&cmds[2], Command::Upsert { kind: "text", id: "child", .. }));
/// assert!(matches!(&cmds[3], Command::Pop));
/// ```
///
/// Hyphenated names stay intact — `baz-boop` is one ID, not a destroy
/// command followed by a word:
///
/// ```
/// use byo::parser::parse;
/// use byo::protocol::{Command, Prop};
///
/// let cmds = parse("+view root baz-boop").unwrap();
/// assert_eq!(cmds.len(), 1);
/// match &cmds[0] {
///     Command::Upsert { id, props, .. } => {
///         assert_eq!(*id, "root");
///         assert_eq!(props[0], Prop::flag("baz-boop"));
///     }
///     _ => panic!("expected Upsert"),
/// }
/// ```
///
/// # Errors
///
/// Returns a [`ParseError`] on syntax errors, unexpected tokens, or
/// invalid values (e.g. non-numeric sequence numbers).
///
/// ```
/// use byo::parser::parse;
/// use byo::lexer::ParseErrorKind;
///
/// let err = parse("+view root {").unwrap_err();
/// assert_eq!(err.kind, ParseErrorKind::UnclosedBrace);
/// ```
pub fn parse(input: &str) -> Result<Vec<Command<'_>>, ParseError> {
    let tokens = tokenize(input)?;
    let mut p = Parser::new(input, &tokens);
    p.parse_batch()
}

// -- Parser internals ---------------------------------------------------------

struct Parser<'a, 'tok> {
    input: &'a str,
    tokens: &'tok [Spanned<'a>],
    pos: usize,
}

impl<'a, 'tok> Parser<'a, 'tok> {
    fn new(input: &'a str, tokens: &'tok [Spanned<'a>]) -> Self {
        Self {
            input,
            tokens,
            pos: 0,
        }
    }

    // -- Navigation -----------------------------------------------------------

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Spanned<'a>> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> &Spanned<'a> {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    /// Span for the current position (or EOF).
    fn here_span(&self) -> Span {
        if let Some(tok) = self.peek() {
            tok.span
        } else {
            Span::at(self.input.len())
        }
    }

    fn error(&self, kind: ParseErrorKind) -> ParseError {
        ParseError {
            kind,
            span: self.here_span(),
        }
    }

    // -- Batch ----------------------------------------------------------------

    fn parse_batch(&mut self) -> Result<Vec<Command<'a>>, ParseError> {
        let mut cmds = Vec::new();
        while !self.at_end() {
            self.parse_command(&mut cmds)?;
        }
        Ok(cmds)
    }

    // -- Command dispatch -----------------------------------------------------

    fn parse_command(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        let tok = self.peek().ok_or_else(|| {
            self.error(ParseErrorKind::Expected {
                expected: "command",
                found: "end of input".into(),
            })
        })?;

        match &tok.token {
            Token::Plus => self.parse_upsert(cmds),
            Token::Minus => self.parse_destroy(cmds),
            Token::At => self.parse_patch(cmds),
            Token::Bang => self.parse_event(cmds),
            _ => Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "command operator (+, -, @, !)",
                    found: self.describe_token(tok),
                },
                span: tok.span,
            }),
        }
    }

    // -- Upsert (+) -----------------------------------------------------------

    fn parse_upsert(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        self.advance(); // consume +
        let kind = self.expect_type()?;
        let id = self.expect_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Upsert { kind, id, props });
        self.parse_children(cmds)?;
        Ok(())
    }

    // -- Destroy (-) ----------------------------------------------------------

    fn parse_destroy(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        self.advance(); // consume -
        let kind = self.expect_type()?;
        let id = self.expect_named_id()?;
        cmds.push(Command::Destroy { kind, id });
        Ok(())
    }

    // -- Patch (@) ------------------------------------------------------------

    fn parse_patch(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        self.advance(); // consume @
        let kind = self.expect_type()?;
        let id = self.expect_named_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Patch { kind, id, props });
        self.parse_children(cmds)?;
        Ok(())
    }

    // -- Event (!) ------------------------------------------------------------

    fn parse_event(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        self.advance(); // consume !

        let word = self.expect_type()?;

        match word {
            "ack" => self.parse_ack(cmds),
            "sub" => self.parse_sub(cmds),
            "unsub" => self.parse_unsub(cmds),
            _ => self.parse_other_event(word, cmds),
        }
    }

    fn parse_ack(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        let kind_str = self.expect_type()?;
        let kind = EventKind::from_wire(kind_str);
        let seq = self.expect_seqnum()?;
        let props = self.parse_props()?;
        cmds.push(Command::Ack { kind, seq, props });
        Ok(())
    }

    fn parse_sub(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        let seq = self.expect_seqnum()?;
        let target_type = self.expect_type()?;
        cmds.push(Command::Sub { seq, target_type });
        Ok(())
    }

    fn parse_unsub(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        let seq = self.expect_seqnum()?;
        let target_type = self.expect_type()?;
        cmds.push(Command::Unsub { seq, target_type });
        Ok(())
    }

    fn parse_other_event(
        &mut self,
        kind_str: &'a str,
        cmds: &mut Vec<Command<'a>>,
    ) -> Result<(), ParseError> {
        let kind = EventKind::from_wire(kind_str);
        let seq = self.expect_seqnum()?;
        let id = self.expect_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Event {
            kind,
            seq,
            id,
            props,
        });
        Ok(())
    }

    // -- Children -------------------------------------------------------------

    fn parse_children(&mut self, cmds: &mut Vec<Command<'a>>) -> Result<(), ParseError> {
        if matches!(self.peek(), Some(Spanned { token: Token::LBrace, .. })) {
            let open_span = self.advance().span; // consume {
            cmds.push(Command::Push);

            while !self.at_end() {
                if matches!(self.peek(), Some(Spanned { token: Token::RBrace, .. })) {
                    break;
                }
                self.parse_command(cmds)?;
            }

            if self.at_end() {
                return Err(ParseError {
                    kind: ParseErrorKind::UnclosedBrace,
                    span: open_span,
                });
            }

            self.advance(); // consume }
            cmds.push(Command::Pop);
        }
        Ok(())
    }

    // -- Props ----------------------------------------------------------------

    fn parse_props(&mut self) -> Result<Vec<Prop<'a>>, ParseError> {
        let mut props = Vec::new();

        loop {
            match self.peek() {
                // Stop at operators, braces, or end
                None => break,
                Some(Spanned { token: Token::Plus, .. })
                | Some(Spanned { token: Token::Minus, .. })
                | Some(Spanned { token: Token::At, .. })
                | Some(Spanned { token: Token::Bang, .. })
                | Some(Spanned { token: Token::LBrace, .. })
                | Some(Spanned { token: Token::RBrace, .. }) => break,

                // ~name — remove prop
                Some(Spanned { token: Token::Tilde, .. }) => {
                    self.advance(); // consume ~
                    let name = self.expect_name()?;
                    props.push(Prop::remove(name));
                }

                // name=value or bare name (boolean flag)
                Some(Spanned { token: Token::Word(_), .. }) => {
                    let name = self.expect_name()?;

                    if matches!(self.peek(), Some(Spanned { token: Token::Eq, .. })) {
                        self.advance(); // consume =
                        let value = self.expect_value()?;
                        props.push(Prop::val(name, value));
                    } else {
                        props.push(Prop::flag(name));
                    }
                }

                Some(tok) => {
                    return Err(ParseError {
                        kind: ParseErrorKind::Expected {
                            expected: "property or command",
                            found: self.describe_token(tok),
                        },
                        span: tok.span,
                    });
                }
            }
        }

        Ok(props)
    }

    // -- Expect helpers -------------------------------------------------------

    /// Expect a Word token matching the type pattern: `[a-zA-Z][a-zA-Z0-9._-]*`
    fn expect_type(&mut self) -> Result<&'a str, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("type name")?;
        if is_valid_type(word) {
            Ok(word)
        } else {
            Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "type name",
                    found: format!("{word:?}"),
                },
                span,
            })
        }
    }

    /// Expect a Word token matching the id pattern: `[a-zA-Z_][a-zA-Z0-9_:-]*`
    ///
    /// Accepts `_` (anonymous). Use [`expect_named_id`](Self::expect_named_id)
    /// when anonymous IDs are not allowed (destroy, patch).
    fn expect_id(&mut self) -> Result<&'a str, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("id")?;
        if is_valid_id(word) {
            Ok(word)
        } else {
            Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "id",
                    found: format!("{word:?}"),
                },
                span,
            })
        }
    }

    /// Like [`expect_id`](Self::expect_id) but rejects `_` (anonymous).
    ///
    /// Only `+` (upsert) supports anonymous objects. `-` and `@` require
    /// a named ID.
    fn expect_named_id(&mut self) -> Result<&'a str, ParseError> {
        let span = self.here_span();
        let id = self.expect_id()?;
        if id == "_" {
            Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "named id (not '_')",
                    found: "\"_\"".into(),
                },
                span,
            })
        } else {
            Ok(id)
        }
    }

    /// Expect a Word token matching the name pattern: `[a-zA-Z][a-zA-Z0-9_-]*`
    fn expect_name(&mut self) -> Result<&'a str, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("prop name")?;
        if is_valid_name(word) {
            Ok(word)
        } else {
            Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "prop name",
                    found: format!("{word:?}"),
                },
                span,
            })
        }
    }

    /// Expect a Word that parses as u64.
    fn expect_seqnum(&mut self) -> Result<u64, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("sequence number")?;
        word.parse::<u64>().map_err(|_| ParseError {
            kind: ParseErrorKind::InvalidSeqnum,
            span,
        })
    }

    /// Expect a value: Word or Str token.
    fn expect_value(&mut self) -> Result<Cow<'a, str>, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            self.error(ParseErrorKind::Expected {
                expected: "value",
                found: "end of input".into(),
            })
        })?;

        match &tok.token {
            Token::Word(w) => {
                let w = *w;
                self.advance();
                Ok(Cow::Borrowed(w))
            }
            Token::Str(cow) => {
                let cow = cow.clone();
                self.advance();
                Ok(cow)
            }
            _ => Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "value",
                    found: self.describe_token(tok),
                },
                span: tok.span,
            }),
        }
    }

    /// Expect a bare Word token.
    fn expect_word(&mut self, context: &'static str) -> Result<&'a str, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            self.error(ParseErrorKind::Expected {
                expected: context,
                found: "end of input".into(),
            })
        })?;

        match &tok.token {
            Token::Word(w) => {
                let w = *w;
                self.advance();
                Ok(w)
            }
            _ => Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: context,
                    found: self.describe_token(tok),
                },
                span: tok.span,
            }),
        }
    }

    fn describe_token(&self, tok: &Spanned<'_>) -> String {
        match &tok.token {
            Token::Plus => "'+'".into(),
            Token::Minus => "'-'".into(),
            Token::At => "'@'".into(),
            Token::Bang => "'!'".into(),
            Token::LBrace => "'{'".into(),
            Token::RBrace => "'}'".into(),
            Token::Tilde => "'~'".into(),
            Token::Eq => "'='".into(),
            Token::Word(w) => format!("word {w:?}"),
            Token::Str(s) => format!("string {s:?}"),
        }
    }
}

// -- Validation helpers -------------------------------------------------------

/// `[a-zA-Z][a-zA-Z0-9._-]*`
fn is_valid_type(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// `[a-zA-Z_][a-zA-Z0-9_:-]*`
fn is_valid_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-')
}

/// `[a-zA-Z][a-zA-Z0-9_-]*`
fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Upsert ---------------------------------------------------------------

    #[test]
    fn upsert_no_props() {
        let cmds = parse("+view sidebar").unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(
            &cmds[0],
            Command::Upsert { kind: "view", id: "sidebar", props } if props.is_empty()
        ));
    }

    #[test]
    fn upsert_with_props() {
        let cmds = parse("+view sidebar class=\"w-64\" order=0 hidden").unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Upsert { kind, id, props } => {
                assert_eq!(*kind, "view");
                assert_eq!(*id, "sidebar");
                assert_eq!(props.len(), 3);
                assert_eq!(props[0], Prop::val("class", "w-64"));
                assert_eq!(props[1], Prop::val("order", "0"));
                assert_eq!(props[2], Prop::flag("hidden"));
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn upsert_anonymous() {
        let cmds = parse("+view _").unwrap();
        match &cmds[0] {
            Command::Upsert { id, .. } => assert_eq!(*id, "_"),
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn upsert_with_children() {
        let cmds = parse("+view root { +text child }").unwrap();
        assert_eq!(cmds.len(), 4); // Upsert, Push, Upsert, Pop
        assert!(matches!(&cmds[0], Command::Upsert { kind: "view", id: "root", .. }));
        assert!(matches!(&cmds[1], Command::Push));
        assert!(matches!(&cmds[2], Command::Upsert { kind: "text", id: "child", .. }));
        assert!(matches!(&cmds[3], Command::Pop));
    }

    #[test]
    fn nested_children() {
        let cmds = parse("+view a { +view b { +text c } }").unwrap();
        // +view a → Upsert, { → Push, +view b → Upsert, { → Push,
        // +text c → Upsert, } → Pop, } → Pop = 7
        assert_eq!(cmds.len(), 7);
        assert!(matches!(&cmds[0], Command::Upsert { id: "a", .. }));
        assert!(matches!(&cmds[1], Command::Push));
        assert!(matches!(&cmds[2], Command::Upsert { id: "b", .. }));
        assert!(matches!(&cmds[3], Command::Push));
        assert!(matches!(&cmds[4], Command::Upsert { id: "c", .. }));
        assert!(matches!(&cmds[5], Command::Pop));
        assert!(matches!(&cmds[6], Command::Pop));
    }

    #[test]
    fn upsert_props_and_children() {
        let cmds = parse("+view sidebar class=\"w-64\" { +text label content=\"Hi\" }").unwrap();
        assert_eq!(cmds.len(), 4);
        match &cmds[0] {
            Command::Upsert { props, .. } => {
                assert_eq!(props.len(), 1);
                assert_eq!(props[0], Prop::val("class", "w-64"));
            }
            _ => panic!("expected Upsert"),
        }
        match &cmds[2] {
            Command::Upsert { props, .. } => {
                assert_eq!(props[0], Prop::val("content", "Hi"));
            }
            _ => panic!("expected Upsert"),
        }
    }

    // -- Destroy --------------------------------------------------------------

    #[test]
    fn destroy() {
        let cmds = parse("-view item3").unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(matches!(&cmds[0], Command::Destroy { kind: "view", id: "item3" }));
    }

    // -- Patch ----------------------------------------------------------------

    #[test]
    fn patch_with_remove() {
        let cmds = parse("@view sidebar hidden ~tooltip").unwrap();
        match &cmds[0] {
            Command::Patch { kind, id, props } => {
                assert_eq!(*kind, "view");
                assert_eq!(*id, "sidebar");
                assert_eq!(props[0], Prop::flag("hidden"));
                assert_eq!(props[1], Prop::remove("tooltip"));
            }
            _ => panic!("expected Patch"),
        }
    }

    #[test]
    fn patch_with_children() {
        let cmds = parse("@view sidebar { +view child }").unwrap();
        assert_eq!(cmds.len(), 4);
        assert!(matches!(&cmds[0], Command::Patch { .. }));
        assert!(matches!(&cmds[1], Command::Push));
        assert!(matches!(&cmds[2], Command::Upsert { .. }));
        assert!(matches!(&cmds[3], Command::Pop));
    }

    // -- Events ---------------------------------------------------------------

    #[test]
    fn event_click() {
        let cmds = parse("!click 0 save").unwrap();
        match &cmds[0] {
            Command::Event { kind, seq, id, props } => {
                assert_eq!(*kind, EventKind::Click);
                assert_eq!(*seq, 0);
                assert_eq!(*id, "save");
                assert!(props.is_empty());
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_props() {
        let cmds = parse("!keydown 0 editor key=a mod=ctrl").unwrap();
        match &cmds[0] {
            Command::Event { kind, seq, id, props } => {
                assert_eq!(*kind, EventKind::KeyDown);
                assert_eq!(*seq, 0);
                assert_eq!(*id, "editor");
                assert_eq!(props[0], Prop::val("key", "a"));
                assert_eq!(props[1], Prop::val("mod", "ctrl"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_pointer() {
        let cmds = parse("!pointer 5 content x=120 y=45 type=move").unwrap();
        match &cmds[0] {
            Command::Event { kind, seq, .. } => {
                assert_eq!(*kind, EventKind::Pointer);
                assert_eq!(*seq, 5);
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_third_party() {
        let cmds = parse("!com.example.spell-check 0 editor").unwrap();
        match &cmds[0] {
            Command::Event { kind, .. } => {
                assert_eq!(*kind, EventKind::Other("com.example.spell-check"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn ack() {
        let cmds = parse("!ack click 0 handled=true").unwrap();
        match &cmds[0] {
            Command::Ack { kind, seq, props } => {
                assert_eq!(*kind, EventKind::Click);
                assert_eq!(*seq, 0);
                assert_eq!(props[0], Prop::val("handled", "true"));
            }
            _ => panic!("expected Ack"),
        }
    }

    #[test]
    fn ack_no_props() {
        let cmds = parse("!ack keydown 7").unwrap();
        match &cmds[0] {
            Command::Ack { kind, seq, props } => {
                assert_eq!(*kind, EventKind::KeyDown);
                assert_eq!(*seq, 7);
                assert!(props.is_empty());
            }
            _ => panic!("expected Ack"),
        }
    }

    #[test]
    fn sub() {
        let cmds = parse("!sub 0 button").unwrap();
        assert!(matches!(&cmds[0], Command::Sub { seq: 0, target_type: "button" }));
    }

    #[test]
    fn unsub() {
        let cmds = parse("!unsub 2 checkbox").unwrap();
        assert!(matches!(&cmds[0], Command::Unsub { seq: 2, target_type: "checkbox" }));
    }

    // -- Multi-command batches ------------------------------------------------

    #[test]
    fn multiple_commands() {
        let cmds = parse("+view a +view b -view c").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(&cmds[0], Command::Upsert { id: "a", .. }));
        assert!(matches!(&cmds[1], Command::Upsert { id: "b", .. }));
        assert!(matches!(&cmds[2], Command::Destroy { id: "c", .. }));
    }

    #[test]
    fn mixed_batch() {
        let cmds = parse("!sub 0 button !sub 1 slider !sub 2 checkbox").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(&cmds[0], Command::Sub { seq: 0, target_type: "button" }));
        assert!(matches!(&cmds[1], Command::Sub { seq: 1, target_type: "slider" }));
        assert!(matches!(&cmds[2], Command::Sub { seq: 2, target_type: "checkbox" }));
    }

    #[test]
    fn full_batch_from_spec() {
        let input = concat!(
            "+layer content order=0\n",
            "+view greeting class=\"p-4\" {\n",
            "  +text label content=\"Hello, world\" class=\"text-2xl text-white\"\n",
            "}\n",
        );
        let cmds = parse(input).unwrap();
        // layer, view, push, text, pop
        assert_eq!(cmds.len(), 5);
        assert!(matches!(&cmds[0], Command::Upsert { kind: "layer", id: "content", .. }));
        assert!(matches!(&cmds[1], Command::Upsert { kind: "view", id: "greeting", .. }));
        assert!(matches!(&cmds[2], Command::Push));
        assert!(matches!(&cmds[3], Command::Upsert { kind: "text", id: "label", .. }));
        assert!(matches!(&cmds[4], Command::Pop));
    }

    #[test]
    fn sidebar_example() {
        let input = concat!(
            "+view sidebar class=\"w-64 h-full bg-zinc-800\" {\n",
            "  +view item1 class=\"px-4 py-2\" {\n",
            "    +text item1-label content=\"Network\"\n",
            "  }\n",
            "  +view item2 class=\"px-4 py-2\" {\n",
            "    +text item2-label content=\"Display\"\n",
            "  }\n",
            "}\n",
        );
        let cmds = parse(input).unwrap();
        // sidebar, push, item1, push, item1-label, pop, item2, push, item2-label, pop, pop
        assert_eq!(cmds.len(), 11);
    }

    // -- Error cases ----------------------------------------------------------

    #[test]
    fn unclosed_brace() {
        let err = parse("+view root {").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnclosedBrace);
    }

    #[test]
    fn unexpected_rbrace() {
        let err = parse("}").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::Expected { .. }));
    }

    #[test]
    fn missing_id() {
        let err = parse("+view").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::Expected { expected: "id", .. }));
    }

    #[test]
    fn invalid_seqnum() {
        let err = parse("!click abc save").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::InvalidSeqnum);
    }

    #[test]
    fn invalid_event_type() {
        // Event type must match [a-zA-Z][a-zA-Z0-9._-]*, not a number
        let err = parse("!123 0 target").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::Expected { expected: "type name", .. }));
    }

    #[test]
    fn invalid_ack_event_type() {
        let err = parse("!ack 123 0").unwrap_err();
        assert!(matches!(err.kind, ParseErrorKind::Expected { expected: "type name", .. }));
    }

    #[test]
    fn anon_id_rejected_in_destroy() {
        let err = parse("-view _").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected { expected: "named id (not '_')", .. }
        ));
    }

    #[test]
    fn anon_id_rejected_in_patch() {
        let err = parse("@view _").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected { expected: "named id (not '_')", .. }
        ));
    }

    #[test]
    fn anon_id_allowed_in_upsert() {
        let cmds = parse("+view _").unwrap();
        match &cmds[0] {
            Command::Upsert { id, .. } => assert_eq!(*id, "_"),
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn empty_input() {
        let cmds = parse("").unwrap();
        assert!(cmds.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let cmds = parse("   \n\t  ").unwrap();
        assert!(cmds.is_empty());
    }

    // -- Escaped string values ------------------------------------------------

    #[test]
    fn prop_value_with_escapes() {
        let cmds = parse(r#"+text msg content="say \"hello\"""#).unwrap();
        match &cmds[0] {
            Command::Upsert { props, .. } => {
                assert_eq!(props[0], Prop::val("content", "say \"hello\""));
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn single_quoted_value() {
        let cmds = parse("+text msg content='hello world'").unwrap();
        match &cmds[0] {
            Command::Upsert { props, .. } => {
                assert_eq!(props[0], Prop::val("content", "hello world"));
            }
            _ => panic!("expected Upsert"),
        }
    }

    // -- Qualified IDs --------------------------------------------------------

    #[test]
    fn qualified_id() {
        let cmds = parse("+view notes-app:sidebar").unwrap();
        match &cmds[0] {
            Command::Upsert { id, .. } => assert_eq!(*id, "notes-app:sidebar"),
            _ => panic!("expected Upsert"),
        }
    }

    // -- Dot-qualified types --------------------------------------------------

    #[test]
    fn dot_qualified_type() {
        let cmds = parse("+org.mybrowser.WebView wv1").unwrap();
        match &cmds[0] {
            Command::Upsert { kind, .. } => assert_eq!(*kind, "org.mybrowser.WebView"),
            _ => panic!("expected Upsert"),
        }
    }

    // -- Expand event ---------------------------------------------------------

    #[test]
    fn expand_event() {
        let cmds =
            parse("!expand 0 notes-app:save kind=button label=\"Save\"").unwrap();
        match &cmds[0] {
            Command::Event { kind, seq, id, props } => {
                assert_eq!(*kind, EventKind::Expand);
                assert_eq!(*seq, 0);
                assert_eq!(*id, "notes-app:save");
                assert_eq!(props[0], Prop::val("kind", "button"));
                assert_eq!(props[1], Prop::val("label", "Save"));
            }
            _ => panic!("expected Event"),
        }
    }

    // -- Round-trip test (emitter → parser) -----------------------------------

    #[test]
    fn round_trip_upsert() {
        use crate::emitter::Emitter;

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| {
            em.upsert_with("view", "sidebar", &[Prop::val("class", "w-64")], |em| {
                em.upsert("text", "label", &[Prop::val("content", "Hello")])
            })
        })
        .unwrap();

        let wire = String::from_utf8(buf).unwrap();
        // Strip APC framing: ESC _ B ... \n ESC \
        let payload = wire
            .strip_prefix("\x1b_B")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();

        let cmds = parse(payload).unwrap();
        assert_eq!(cmds.len(), 4); // upsert, push, upsert, pop

        match &cmds[0] {
            Command::Upsert { kind, id, props } => {
                assert_eq!(*kind, "view");
                assert_eq!(*id, "sidebar");
                assert_eq!(props[0], Prop::val("class", "w-64"));
            }
            _ => panic!("expected Upsert"),
        }
        assert!(matches!(&cmds[1], Command::Push));
        match &cmds[2] {
            Command::Upsert { kind, id, props } => {
                assert_eq!(*kind, "text");
                assert_eq!(*id, "label");
                assert_eq!(props[0], Prop::val("content", "Hello"));
            }
            _ => panic!("expected Upsert"),
        }
        assert!(matches!(&cmds[3], Command::Pop));
    }

    #[test]
    fn round_trip_events() {
        use crate::emitter::Emitter;

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| {
            em.event("click", 0, "save", &[])?;
            em.ack("click", 0, &[Prop::val("handled", "true")])?;
            em.sub(0, "button")?;
            em.unsub(1, "slider")
        })
        .unwrap();

        let wire = String::from_utf8(buf).unwrap();
        let payload = wire
            .strip_prefix("\x1b_B")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();

        let cmds = parse(payload).unwrap();
        assert_eq!(cmds.len(), 4);
        assert!(matches!(&cmds[0], Command::Event { kind: EventKind::Click, seq: 0, id: "save", .. }));
        assert!(matches!(&cmds[1], Command::Ack { kind: EventKind::Click, seq: 0, .. }));
        assert!(matches!(&cmds[2], Command::Sub { seq: 0, target_type: "button" }));
        assert!(matches!(&cmds[3], Command::Unsub { seq: 1, target_type: "slider" }));
    }

    #[test]
    fn round_trip_escape_encoding() {
        use crate::emitter::Emitter;

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| {
            em.upsert("text", "msg", &[Prop::val("content", "it's a \"test\"")])
        })
        .unwrap();

        let wire = String::from_utf8(buf).unwrap();
        let payload = wire
            .strip_prefix("\x1b_B")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();

        let cmds = parse(payload).unwrap();
        match &cmds[0] {
            Command::Upsert { props, .. } => {
                assert_eq!(props[0], Prop::val("content", "it's a \"test\""));
            }
            _ => panic!("expected Upsert"),
        }
    }
}
