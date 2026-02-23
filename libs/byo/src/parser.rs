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
//! assert!(matches!(&cmds[0], Command::Upsert { kind, id, .. } if kind == "view" && id == "sidebar"));
//! ```

use std::borrow::Cow;

use bytes::Bytes;

use crate::byte_str::ByteStr;
use crate::lexer::{ParseError, ParseErrorKind, Span, Spanned, Token, tokenize};
use crate::protocol::{Command, EventKind, PragmaKind, Prop, RequestKind, ResponseKind};

/// Zero-copy parse from a `Bytes` source.
///
/// The `Bytes` buffer must contain valid UTF-8. The parser sub-slices
/// it to produce `ByteStr` fields in the returned commands — zero-copy
/// for types, IDs, keys, and unescaped values.
pub fn parse_bytes(input: Bytes) -> Result<Vec<Command>, ParseError> {
    // Validate UTF-8 upfront — all subsequent sub-slicing is safe.
    let source = input.clone(); // cheap: atomic increment
    let input_str = std::str::from_utf8(&input).map_err(|e| ParseError {
        kind: ParseErrorKind::Expected {
            expected: "valid UTF-8",
            found: format!("invalid UTF-8 at byte {}", e.valid_up_to()),
        },
        span: Span::at(e.valid_up_to()),
    })?;
    let tokens = tokenize(input_str)?;
    let mut p = Parser::new(input_str, &tokens, source);
    p.parse_batch()
}

/// Convenience wrapper — copies input into `Bytes` first.
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
/// assert!(matches!(&cmds[0], Command::Upsert { kind, id, .. } if kind == "view" && id == "root"));
/// assert!(matches!(&cmds[1], Command::Push { .. }));
/// assert!(matches!(&cmds[2], Command::Upsert { kind, id, .. } if kind == "text" && id == "child"));
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
///         assert_eq!(id, "root");
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
pub fn parse(input: &str) -> Result<Vec<Command>, ParseError> {
    parse_bytes(Bytes::copy_from_slice(input.as_bytes()))
}

// -- Parser internals ---------------------------------------------------------

struct Parser<'a, 'tok> {
    input: &'a str,
    source: Bytes,
    tokens: &'tok [Spanned<'a>],
    pos: usize,
}

impl<'a, 'tok> Parser<'a, 'tok> {
    fn new(input: &'a str, tokens: &'tok [Spanned<'a>], source: Bytes) -> Self {
        Self {
            input,
            source,
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

    /// Create a ByteStr by sub-slicing the source Bytes at the given span.
    fn byte_str_at(&self, span: Span) -> ByteStr {
        // SAFETY: source is validated UTF-8 and span comes from tokenizer.
        ByteStr::from_utf8(self.source.slice(span.start..span.end)).unwrap()
    }

    // -- Batch ----------------------------------------------------------------

    fn parse_batch(&mut self) -> Result<Vec<Command>, ParseError> {
        let mut cmds = Vec::new();
        while !self.at_end() {
            self.parse_command(&mut cmds)?;
        }
        Ok(cmds)
    }

    // -- Command dispatch -----------------------------------------------------

    fn parse_command(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
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
            Token::Hash => self.parse_pragma(cmds),
            Token::Question => self.parse_request(cmds),
            Token::Dot => self.parse_response(cmds),
            _ => Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "command operator (+, -, @, !, #, ?, .)",
                    found: self.describe_token(tok),
                },
                span: tok.span,
            }),
        }
    }

    // -- Upsert (+) -----------------------------------------------------------

    fn parse_upsert(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume +
        let kind = self.expect_type()?;
        let id = self.expect_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Upsert { kind, id, props });
        self.parse_children(cmds)?;
        Ok(())
    }

    // -- Destroy (-) ----------------------------------------------------------

    fn parse_destroy(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume -
        let kind = self.expect_type()?;
        let id = self.expect_named_id()?;
        cmds.push(Command::Destroy { kind, id });
        Ok(())
    }

    // -- Patch (@) ------------------------------------------------------------

    fn parse_patch(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume @
        let kind = self.expect_type()?;
        let id = self.expect_named_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Patch { kind, id, props });
        self.parse_children(cmds)?;
        Ok(())
    }

    // -- Event (!) ------------------------------------------------------------

    fn parse_event(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume !

        let word = self.expect_word("type name")?;
        let is_ack = word == "ack";

        if is_ack {
            self.parse_ack(cmds)
        } else {
            let span = self.tokens[self.pos - 1].span;
            if !is_valid_type(word) {
                return Err(ParseError {
                    kind: ParseErrorKind::Expected {
                        expected: "type name",
                        found: format!("{word:?}"),
                    },
                    span,
                });
            }
            let kind_str = self.byte_str_at(span);
            self.parse_other_event(kind_str, cmds)
        }
    }

    fn parse_ack(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        let kind_str = self.expect_type()?;
        let kind = EventKind::from_wire(kind_str);
        let seq = self.expect_seqnum()?;
        let props = self.parse_props()?;
        cmds.push(Command::Ack { kind, seq, props });
        Ok(())
    }

    fn parse_other_event(
        &mut self,
        kind_str: impl Into<ByteStr>,
        cmds: &mut Vec<Command>,
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

    // -- Pragma (#) -----------------------------------------------------------

    fn parse_pragma(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume #

        let word = self.expect_word("pragma kind")?;
        let span = self.tokens[self.pos - 1].span;

        if !is_valid_type(word) {
            return Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "pragma kind",
                    found: format!("{word:?}"),
                },
                span,
            });
        }

        let kind = PragmaKind::from_wire(self.byte_str_at(span));

        match kind {
            PragmaKind::Claim
            | PragmaKind::Unclaim
            | PragmaKind::Observe
            | PragmaKind::Unobserve => {
                let mut targets = vec![self.expect_type()?];
                while matches!(
                    self.peek(),
                    Some(Spanned {
                        token: Token::Comma,
                        ..
                    })
                ) {
                    self.advance(); // consume comma
                    targets.push(self.expect_type()?);
                }
                cmds.push(Command::Pragma { kind, targets });
                Ok(())
            }
            PragmaKind::Redirect => {
                let target = self.expect_id()?;
                cmds.push(Command::Pragma {
                    kind,
                    targets: vec![target],
                });
                Ok(())
            }
            PragmaKind::Unredirect => {
                cmds.push(Command::Pragma {
                    kind,
                    targets: Vec::new(),
                });
                Ok(())
            }
            _ => {
                // Generic pragma — consume optional targets
                let mut targets = Vec::new();
                while matches!(
                    self.peek(),
                    Some(Spanned {
                        token: Token::Word(_),
                        ..
                    })
                ) {
                    targets.push(self.expect_type()?);
                    if !matches!(
                        self.peek(),
                        Some(Spanned {
                            token: Token::Comma,
                            ..
                        })
                    ) {
                        break;
                    }
                    self.advance(); // consume comma
                }
                cmds.push(Command::Pragma { kind, targets });
                Ok(())
            }
        }
    }

    // -- Request (?) ----------------------------------------------------------

    fn parse_request(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume ?

        let word = self.expect_word("type name")?;
        let span = self.tokens[self.pos - 1].span;

        if !is_valid_type(word) {
            return Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "type name",
                    found: format!("{word:?}"),
                },
                span,
            });
        }

        let kind = RequestKind::from_wire(self.byte_str_at(span));
        let seq = self.expect_seqnum()?;
        let id = self.expect_id()?;
        let props = self.parse_props()?;
        cmds.push(Command::Request {
            kind,
            seq,
            targets: vec![id],
            props,
        });
        Ok(())
    }

    // -- Response (.) ---------------------------------------------------------

    fn parse_response(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        self.advance(); // consume .

        let word = self.expect_word("type name")?;
        let span = self.tokens[self.pos - 1].span;

        if !is_valid_type(word) {
            return Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "type name",
                    found: format!("{word:?}"),
                },
                span,
            });
        }

        let kind = ResponseKind::from_wire(self.byte_str_at(span));
        let seq = self.expect_seqnum()?;

        match kind {
            ResponseKind::Expand => {
                // .expand seq { body } — body is mandatory
                let mut body = Vec::new();
                self.parse_mandatory_children(&mut body)?;
                cmds.push(Command::Response {
                    kind,
                    seq,
                    props: Vec::new(),
                    body: Some(body),
                });
                Ok(())
            }
            _ => {
                // .kind seq props... [{ body }]
                let props = self.parse_props()?;
                let body = if matches!(
                    self.peek(),
                    Some(Spanned {
                        token: Token::LBrace,
                        ..
                    })
                ) {
                    // Use parse_mandatory_children (no Push/Pop) — same as
                    // .expand. Braces are grammar syntax, not tree structure.
                    let mut body = Vec::new();
                    self.parse_mandatory_children(&mut body)?;
                    if body.is_empty() { None } else { Some(body) }
                } else {
                    None
                };
                cmds.push(Command::Response {
                    kind,
                    seq,
                    props,
                    body,
                });
                Ok(())
            }
        }
    }

    /// Parse a mandatory body block (used by `.expand`).
    ///
    /// Unlike `parse_children`, this does **not** add `Push`/`Pop` to `cmds`.
    /// The braces here are grammar syntax delimiting the response body, not
    /// tree-structure commands. The emitter re-wraps with `{ ... }` on output.
    fn parse_mandatory_children(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        match self.peek() {
            Some(Spanned {
                token: Token::LBrace,
                ..
            }) => {
                let open_span = self.advance().span; // consume {

                while !self.at_end() {
                    if matches!(
                        self.peek(),
                        Some(Spanned {
                            token: Token::RBrace,
                            ..
                        })
                    ) {
                        break;
                    }
                    // `::name { ... }` — slot declaration inside a response body
                    if let Some(Spanned {
                        token: Token::Word(w),
                        ..
                    }) = self.peek()
                        && w.starts_with("::")
                    {
                        self.parse_slot_block(cmds)?;
                        continue;
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
                Ok(())
            }
            _ => Err(self.error(ParseErrorKind::Expected {
                expected: "'{' (response body)",
                found: match self.peek() {
                    Some(tok) => self.describe_token(tok),
                    None => "end of input".into(),
                },
            })),
        }
    }

    // -- Children -------------------------------------------------------------

    fn parse_children(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        if matches!(
            self.peek(),
            Some(Spanned {
                token: Token::LBrace,
                ..
            })
        ) {
            let open_span = self.advance().span; // consume {

            cmds.push(Command::Push { slot: None });

            while !self.at_end() {
                if matches!(
                    self.peek(),
                    Some(Spanned {
                        token: Token::RBrace,
                        ..
                    })
                ) {
                    break;
                }
                // `::name { ... }` — slot block inside a children block
                if let Some(Spanned {
                    token: Token::Word(w),
                    ..
                }) = self.peek()
                    && w.starts_with("::")
                {
                    self.parse_slot_block(cmds)?;
                    continue;
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

    /// Parse a `::name { ... }` slot block.
    ///
    /// The slot name is extracted from a `::name` Word token. The `{` `}`
    /// delimiters wrap the slot's children (fallback content on the expansion
    /// side, projected content on the app side).
    fn parse_slot_block(&mut self, cmds: &mut Vec<Command>) -> Result<(), ParseError> {
        let tok = self.advance(); // consume ::name
        let span = tok.span;
        let w = match &tok.token {
            Token::Word(w) => *w,
            _ => unreachable!("caller checked for Word"),
        };
        let name = &w[2..]; // strip `::`
        if name.is_empty() || !is_valid_slot_name(name) {
            return Err(ParseError {
                kind: ParseErrorKind::Expected {
                    expected: "slot name after '::'",
                    found: format!("{w:?}"),
                },
                span,
            });
        }
        let slot_name = self.byte_str_at(Span {
            start: span.start + 2,
            end: span.end,
        });
        cmds.push(Command::Push {
            slot: Some(slot_name),
        });

        // Expect `{`
        match self.peek() {
            Some(Spanned {
                token: Token::LBrace,
                ..
            }) => {
                let open_span = self.advance().span; // consume {

                while !self.at_end() {
                    if matches!(
                        self.peek(),
                        Some(Spanned {
                            token: Token::RBrace,
                            ..
                        })
                    ) {
                        break;
                    }
                    // Nested `::name { ... }` inside a slot block
                    if let Some(Spanned {
                        token: Token::Word(w),
                        ..
                    }) = self.peek()
                        && w.starts_with("::")
                    {
                        self.parse_slot_block(cmds)?;
                        continue;
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
                Ok(())
            }
            _ => Err(self.error(ParseErrorKind::Expected {
                expected: "'{' after slot name",
                found: match self.peek() {
                    Some(tok) => self.describe_token(tok),
                    None => "end of input".into(),
                },
            })),
        }
    }

    // -- Props ----------------------------------------------------------------

    fn parse_props(&mut self) -> Result<Vec<Prop>, ParseError> {
        let mut props = Vec::new();

        loop {
            match self.peek() {
                // Stop at operators, braces, or end
                None => break,
                Some(Spanned {
                    token: Token::Plus, ..
                })
                | Some(Spanned {
                    token: Token::Minus,
                    ..
                })
                | Some(Spanned {
                    token: Token::At, ..
                })
                | Some(Spanned {
                    token: Token::Bang, ..
                })
                | Some(Spanned {
                    token: Token::Question,
                    ..
                })
                | Some(Spanned {
                    token: Token::Hash, ..
                })
                | Some(Spanned {
                    token: Token::Dot, ..
                })
                | Some(Spanned {
                    token: Token::LBrace,
                    ..
                })
                | Some(Spanned {
                    token: Token::RBrace,
                    ..
                }) => break,

                // ~name — remove prop
                Some(Spanned {
                    token: Token::Tilde,
                    ..
                }) => {
                    self.advance(); // consume ~
                    let name = self.expect_name()?;
                    props.push(Prop::remove(name));
                }

                // name=value or bare name (boolean flag)
                Some(Spanned {
                    token: Token::Word(_),
                    ..
                }) => {
                    let name = self.expect_name()?;

                    if matches!(
                        self.peek(),
                        Some(Spanned {
                            token: Token::Eq,
                            ..
                        })
                    ) {
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
    /// Returns a ByteStr sub-sliced from the source.
    fn expect_type(&mut self) -> Result<ByteStr, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("type name")?;
        if is_valid_type(word) {
            Ok(self.byte_str_at(self.tokens[self.pos - 1].span))
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
    fn expect_id(&mut self) -> Result<ByteStr, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("id")?;
        if is_valid_id(word) {
            Ok(self.byte_str_at(self.tokens[self.pos - 1].span))
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
    fn expect_named_id(&mut self) -> Result<ByteStr, ParseError> {
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
    fn expect_name(&mut self) -> Result<ByteStr, ParseError> {
        let span = self.here_span();
        let word = self.expect_word("prop name")?;
        if is_valid_name(word) {
            Ok(self.byte_str_at(self.tokens[self.pos - 1].span))
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
            kind: ParseErrorKind::InvalidSeqnum(word.to_string()),
            span,
        })
    }

    /// Expect a value: Word, Str, or `-Word` (negative number). Returns a ByteStr.
    fn expect_value(&mut self) -> Result<ByteStr, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            self.error(ParseErrorKind::Expected {
                expected: "value",
                found: "end of input".into(),
            })
        })?;

        match &tok.token {
            Token::Word(_) => {
                let span = tok.span;
                self.advance();
                Ok(self.byte_str_at(span))
            }
            Token::Str(cow) => {
                let span = tok.span;
                match cow {
                    Cow::Borrowed(_) => {
                        // No escapes — sub-slice source at content (trim quotes)
                        self.advance();
                        Ok(
                            ByteStr::from_utf8(self.source.slice(span.start + 1..span.end - 1))
                                .unwrap(),
                        )
                    }
                    Cow::Owned(s) => {
                        // Has escapes — must allocate
                        let bs = ByteStr::from(s.clone());
                        self.advance();
                        Ok(bs)
                    }
                }
            }
            Token::Minus => {
                let minus_span = tok.span;
                self.advance(); // consume -
                // `-` followed by a Word forms a negative value (e.g. `-91.4`)
                match self.peek() {
                    Some(Spanned {
                        token: Token::Word(_),
                        span: word_span,
                    }) if minus_span.end == word_span.start => {
                        // Contiguous — zero-copy sub-slice covers `-91.4`
                        let combined_end = word_span.end;
                        self.advance();
                        Ok(
                            ByteStr::from_utf8(self.source.slice(minus_span.start..combined_end))
                                .unwrap(),
                        )
                    }
                    _ => {
                        // Bare `-` as a value
                        Ok(self.byte_str_at(minus_span))
                    }
                }
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
            Token::Question => "'?'".into(),
            Token::Dot => "'.'".into(),
            Token::LBrace => "'{'".into(),
            Token::RBrace => "'}'".into(),
            Token::Tilde => "'~'".into(),
            Token::Eq => "'='".into(),
            Token::Comma => "','".into(),
            Token::Hash => "'#'".into(),
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

/// `[a-zA-Z_][a-zA-Z0-9_:-]*` — slot names allow `:` for qualified names.
fn is_valid_slot_name(s: &str) -> bool {
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
            Command::Upsert { kind, id, props } if kind == "view" && id == "sidebar" && props.is_empty()
        ));
    }

    #[test]
    fn upsert_with_props() {
        let cmds = parse("+view sidebar class=\"w-64\" order=0 hidden").unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Upsert { kind, id, props } => {
                assert_eq!(kind, "view");
                assert_eq!(id, "sidebar");
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
            Command::Upsert { id, .. } => assert_eq!(id, "_"),
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn upsert_with_children() {
        let cmds = parse("+view root { +text child }").unwrap();
        assert_eq!(cmds.len(), 4); // Upsert, Push, Upsert, Pop
        assert!(matches!(
            &cmds[0],
            Command::Upsert { kind, id, .. } if kind == "view" && id == "root"
        ));
        assert!(matches!(&cmds[1], Command::Push { .. }));
        assert!(matches!(
            &cmds[2],
            Command::Upsert { kind, id, .. } if kind == "text" && id == "child"
        ));
        assert!(matches!(&cmds[3], Command::Pop));
    }

    #[test]
    fn nested_children() {
        let cmds = parse("+view a { +view b { +text c } }").unwrap();
        // +view a → Upsert, { → Push, +view b → Upsert, { → Push,
        // +text c → Upsert, } → Pop, } → Pop = 7
        assert_eq!(cmds.len(), 7);
        assert!(matches!(&cmds[0], Command::Upsert { id, .. } if id == "a"));
        assert!(matches!(&cmds[1], Command::Push { .. }));
        assert!(matches!(&cmds[2], Command::Upsert { id, .. } if id == "b"));
        assert!(matches!(&cmds[3], Command::Push { .. }));
        assert!(matches!(&cmds[4], Command::Upsert { id, .. } if id == "c"));
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
        assert!(matches!(
            &cmds[0],
            Command::Destroy { kind, id } if kind == "view" && id == "item3"
        ));
    }

    // -- Patch ----------------------------------------------------------------

    #[test]
    fn patch_with_remove() {
        let cmds = parse("@view sidebar hidden ~tooltip").unwrap();
        match &cmds[0] {
            Command::Patch { kind, id, props } => {
                assert_eq!(kind, "view");
                assert_eq!(id, "sidebar");
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
        assert!(matches!(&cmds[1], Command::Push { .. }));
        assert!(matches!(&cmds[2], Command::Upsert { .. }));
        assert!(matches!(&cmds[3], Command::Pop));
    }

    // -- Events ---------------------------------------------------------------

    #[test]
    fn event_click() {
        let cmds = parse("!click 0 save").unwrap();
        match &cmds[0] {
            Command::Event {
                kind,
                seq,
                id,
                props,
            } => {
                assert_eq!(*kind, EventKind::Click);
                assert_eq!(*seq, 0);
                assert_eq!(id, "save");
                assert!(props.is_empty());
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_with_props() {
        let cmds = parse("!keydown 0 editor key=a mod=ctrl").unwrap();
        match &cmds[0] {
            Command::Event {
                kind,
                seq,
                id,
                props,
            } => {
                assert_eq!(*kind, EventKind::KeyDown);
                assert_eq!(*seq, 0);
                assert_eq!(id, "editor");
                assert_eq!(props[0], Prop::val("key", "a"));
                assert_eq!(props[1], Prop::val("mod", "ctrl"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn event_pointermove() {
        let cmds = parse("!pointermove 5 content x=120 y=45").unwrap();
        match &cmds[0] {
            Command::Event { kind, seq, .. } => {
                assert_eq!(*kind, EventKind::PointerMove);
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
                assert_eq!(
                    *kind,
                    EventKind::Other(ByteStr::from("com.example.spell-check"))
                );
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
    fn claim() {
        let cmds = parse("#claim button").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets.len(), 1);
                assert_eq!(targets[0], "button");
            }
            _ => panic!("expected Claim pragma"),
        }
    }

    #[test]
    fn unclaim() {
        let cmds = parse("#unclaim checkbox").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Unclaim);
                assert_eq!(targets[0], "checkbox");
            }
            _ => panic!("expected Unclaim pragma"),
        }
    }

    #[test]
    fn observe() {
        let cmds = parse("#observe view").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Observe);
                assert_eq!(targets[0], "view");
            }
            _ => panic!("expected Observe pragma"),
        }
    }

    #[test]
    fn unobserve() {
        let cmds = parse("#unobserve text").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Unobserve);
                assert_eq!(targets[0], "text");
            }
            _ => panic!("expected Unobserve pragma"),
        }
    }

    #[test]
    fn multi_type_observe() {
        let cmds = parse("#observe view,text,layer").unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Observe);
                assert_eq!(targets.len(), 3);
                assert_eq!(targets[0], "view");
                assert_eq!(targets[1], "text");
                assert_eq!(targets[2], "layer");
            }
            _ => panic!("expected multi-type Observe"),
        }
    }

    #[test]
    fn multi_type_observe_spaces() {
        let cmds = parse("#observe view, text, layer").unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Observe);
                assert_eq!(targets.len(), 3);
                assert_eq!(targets[0], "view");
                assert_eq!(targets[1], "text");
                assert_eq!(targets[2], "layer");
            }
            _ => panic!("expected multi-type Observe with spaces"),
        }
    }

    #[test]
    fn multi_type_claim() {
        let cmds = parse("#claim button,slider,checkbox").unwrap();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets.len(), 3);
                assert_eq!(targets[0], "button");
                assert_eq!(targets[1], "slider");
                assert_eq!(targets[2], "checkbox");
            }
            _ => panic!("expected multi-type Claim"),
        }
    }

    #[test]
    fn redirect() {
        let cmds = parse("#redirect term").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Redirect);
                assert_eq!(targets.len(), 1);
                assert_eq!(targets[0], "term");
            }
            _ => panic!("expected Redirect pragma"),
        }
    }

    #[test]
    fn redirect_discard() {
        let cmds = parse("#redirect _").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Redirect);
                assert_eq!(targets[0], "_");
            }
            _ => panic!("expected Redirect pragma"),
        }
    }

    #[test]
    fn unredirect() {
        let cmds = parse("#unredirect").unwrap();
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Unredirect);
                assert!(targets.is_empty());
            }
            _ => panic!("expected Unredirect pragma"),
        }
    }

    #[test]
    fn request_expand() {
        let cmds = parse("?expand 0 notes-app:save kind=button label=\"Save\"").unwrap();
        match &cmds[0] {
            Command::Request {
                kind,
                seq,
                targets,
                props,
            } => {
                assert_eq!(*kind, RequestKind::Expand);
                assert_eq!(*seq, 0);
                assert_eq!(targets[0], "notes-app:save");
                assert_eq!(props[0], Prop::val("kind", "button"));
                assert_eq!(props[1], Prop::val("label", "Save"));
            }
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn request_custom() {
        let cmds = parse("?render-frame 0 viewport").unwrap();
        match &cmds[0] {
            Command::Request {
                kind,
                seq,
                targets,
                props,
            } => {
                assert_eq!(*kind, RequestKind::Other(ByteStr::from("render-frame")));
                assert_eq!(*seq, 0);
                assert_eq!(targets[0], "viewport");
                assert!(props.is_empty());
            }
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn response_expand() {
        let cmds = parse(".expand 0 { +view root { +text label } }").unwrap();
        match &cmds[0] {
            Command::Response {
                kind,
                seq,
                body: Some(body),
                ..
            } => {
                assert_eq!(*kind, ResponseKind::Expand);
                assert_eq!(*seq, 0);
                // body: Upsert(root), Push, Upsert(label), Pop
                // (no outer Push/Pop — braces are grammar syntax, not commands)
                assert_eq!(body.len(), 4);
            }
            _ => panic!("expected Response with body"),
        }
    }

    #[test]
    fn response_custom_no_body() {
        let cmds = parse(".render-frame 0 status=ok").unwrap();
        match &cmds[0] {
            Command::Response {
                kind,
                seq,
                props,
                body,
            } => {
                assert_eq!(*kind, ResponseKind::Other(ByteStr::from("render-frame")));
                assert_eq!(*seq, 0);
                assert_eq!(props[0], Prop::val("status", "ok"));
                assert!(body.is_none());
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn claim_under_bang_is_event() {
        // !claim should parse as a generic event, not a request
        let cmds = parse("!claim 0 button").unwrap();
        assert!(matches!(
            &cmds[0],
            Command::Event {
                kind,
                ..
            } if kind == &EventKind::Other(ByteStr::from("claim"))
        ));
    }

    #[test]
    fn sub_parses_as_other_request() {
        // ?sub is no longer a keyword — parses as Other("sub") via generic path
        let cmds = parse("?sub 0 button").unwrap();
        match &cmds[0] {
            Command::Request {
                kind, seq, targets, ..
            } => {
                assert_eq!(*kind, RequestKind::Other(ByteStr::from("sub")));
                assert_eq!(*seq, 0);
                assert_eq!(targets[0], "button");
            }
            _ => panic!("expected Other(sub) request"),
        }
    }

    // -- Multi-command batches ------------------------------------------------

    #[test]
    fn multiple_commands() {
        let cmds = parse("+view a +view b -view c").unwrap();
        assert_eq!(cmds.len(), 3);
        assert!(matches!(&cmds[0], Command::Upsert { id, .. } if id == "a"));
        assert!(matches!(&cmds[1], Command::Upsert { id, .. } if id == "b"));
        assert!(matches!(&cmds[2], Command::Destroy { id, .. } if id == "c"));
    }

    #[test]
    fn mixed_batch() {
        let cmds = parse("#claim button #claim slider #claim checkbox").unwrap();
        assert_eq!(cmds.len(), 3);
        match &cmds[0] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets[0], "button");
            }
            _ => panic!("expected Claim"),
        }
        match &cmds[1] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets[0], "slider");
            }
            _ => panic!("expected Claim"),
        }
        match &cmds[2] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets[0], "checkbox");
            }
            _ => panic!("expected Claim"),
        }
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
        assert!(matches!(
            &cmds[0],
            Command::Upsert { kind, id, .. } if kind == "layer" && id == "content"
        ));
        assert!(matches!(
            &cmds[1],
            Command::Upsert { kind, id, .. } if kind == "view" && id == "greeting"
        ));
        assert!(matches!(&cmds[2], Command::Push { .. }));
        assert!(matches!(
            &cmds[3],
            Command::Upsert { kind, id, .. } if kind == "text" && id == "label"
        ));
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

    // -- Slot blocks ----------------------------------------------------------

    #[test]
    fn slot_block_named() {
        let cmds = parse("+view r { ::header { +text t } }").unwrap();
        // Upsert, Push{None}, Push{header}, Upsert, Pop, Pop
        assert_eq!(cmds.len(), 6);
        assert!(matches!(&cmds[0], Command::Upsert { id, .. } if id == "r"));
        assert!(matches!(&cmds[1], Command::Push { slot: None }));
        match &cmds[2] {
            Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "header"),
            other => panic!("expected slotted Push, got: {other:?}"),
        }
        assert!(matches!(&cmds[3], Command::Upsert { id, .. } if id == "t"));
        assert!(matches!(&cmds[4], Command::Pop));
        assert!(matches!(&cmds[5], Command::Pop));
    }

    #[test]
    fn slot_block_default() {
        let cmds = parse("+view r { ::_ { +text t } }").unwrap();
        // Upsert, Push{None}, Push{_}, Upsert, Pop, Pop
        assert_eq!(cmds.len(), 6);
        match &cmds[2] {
            Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "_"),
            other => panic!("expected default slot Push, got: {other:?}"),
        }
    }

    #[test]
    fn slot_block_empty_children() {
        let cmds = parse("+view r {}").unwrap();
        assert_eq!(cmds.len(), 3); // Upsert, Push{slot:None}, Pop
        match &cmds[1] {
            Command::Push { slot: None } => {}
            other => panic!("expected non-slotted Push, got: {other:?}"),
        }
    }

    #[test]
    fn slot_block_multiple() {
        // Multiple slot blocks inside a children block:
        //   +dialog d { ::header { +text t } ::_ { +button ok } }
        let cmds = parse("+dialog d { ::header { +text t } ::_ { +button ok } }").unwrap();
        // Upsert d, Push{None}
        //   Push{header}, Upsert t, Pop
        //   Push{_}, Upsert ok, Pop
        // Pop
        assert_eq!(cmds.len(), 9);
        assert!(matches!(&cmds[0], Command::Upsert { id, .. } if id == "d"));
        assert!(matches!(&cmds[1], Command::Push { slot: None }));
        match &cmds[2] {
            Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "header"),
            other => panic!("expected header slot, got: {other:?}"),
        }
        assert!(matches!(&cmds[3], Command::Upsert { id, .. } if id == "t"));
        assert!(matches!(&cmds[4], Command::Pop));
        match &cmds[5] {
            Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "_"),
            other => panic!("expected default slot, got: {other:?}"),
        }
        assert!(matches!(&cmds[6], Command::Upsert { id, .. } if id == "ok"));
        assert!(matches!(&cmds[7], Command::Pop));
        assert!(matches!(&cmds[8], Command::Pop));
    }

    #[test]
    fn slot_block_empty() {
        let cmds = parse("+view r { ::header {} }").unwrap();
        // Upsert, Push{None}, Push{header}, Pop, Pop
        assert_eq!(cmds.len(), 5);
        match &cmds[2] {
            Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "header"),
            other => panic!("expected slotted Push, got: {other:?}"),
        }
    }

    #[test]
    fn regular_push_no_slot() {
        let cmds = parse("+view root { +text child }").unwrap();
        match &cmds[1] {
            Command::Push { slot: None } => {}
            other => panic!("expected non-slotted Push, got: {other:?}"),
        }
    }

    #[test]
    fn slot_in_expand_response() {
        // .expand responses can contain slot declarations
        let cmds = parse(".expand 0 { +view root { ::_ { +text fallback } } }").unwrap();
        // Response body: Upsert root, Push{None}, Push{_}, Upsert fallback, Pop, Pop
        match &cmds[0] {
            Command::Response {
                body: Some(body), ..
            } => {
                assert_eq!(body.len(), 6);
                assert!(matches!(&body[0], Command::Upsert { id, .. } if id == "root"));
                assert!(matches!(&body[1], Command::Push { slot: None }));
                match &body[2] {
                    Command::Push { slot: Some(name) } => assert_eq!(name.as_ref(), "_"),
                    other => panic!("expected default slot push, got: {other:?}"),
                }
                assert!(matches!(&body[3], Command::Upsert { id, .. } if id == "fallback"));
                assert!(matches!(&body[4], Command::Pop));
                assert!(matches!(&body[5], Command::Pop));
            }
            other => panic!("expected Response, got: {other:?}"),
        }
    }

    #[test]
    fn slot_block_missing_brace() {
        // ::header without { should be an error
        let err = parse("+view r { ::header }");
        assert!(err.is_err());
    }

    #[test]
    fn slot_block_at_top_level_is_error() {
        // ::foo {} at the top level (not inside a children block) is a parse error
        let err = parse("::header { +text t }");
        assert!(err.is_err());
    }

    #[test]
    fn slot_block_in_upsert_children() {
        // +foo id { ::slot {} } works
        let cmds = parse("+view r { ::header { +text t } }").unwrap();
        assert!(matches!(&cmds[2], Command::Push { slot: Some(name) } if name == "header"));
    }

    #[test]
    fn slot_block_in_patch_children() {
        // @foo id { ::slot {} } works
        let cmds = parse("@view r { ::header { +text t } }").unwrap();
        assert!(matches!(&cmds[2], Command::Push { slot: Some(name) } if name == "header"));
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
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected { expected: "id", .. }
        ));
    }

    #[test]
    fn invalid_seqnum() {
        let err = parse("!click abc save").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::InvalidSeqnum("abc".into()));
    }

    #[test]
    fn invalid_event_type() {
        // Event type must match [a-zA-Z][a-zA-Z0-9._-]*, not a number
        let err = parse("!123 0 target").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected {
                expected: "type name",
                ..
            }
        ));
    }

    #[test]
    fn invalid_ack_event_type() {
        let err = parse("!ack 123 0").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected {
                expected: "type name",
                ..
            }
        ));
    }

    #[test]
    fn anon_id_rejected_in_destroy() {
        let err = parse("-view _").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected {
                expected: "named id (not '_')",
                ..
            }
        ));
    }

    #[test]
    fn anon_id_rejected_in_patch() {
        let err = parse("@view _").unwrap_err();
        assert!(matches!(
            err.kind,
            ParseErrorKind::Expected {
                expected: "named id (not '_')",
                ..
            }
        ));
    }

    #[test]
    fn anon_id_allowed_in_upsert() {
        let cmds = parse("+view _").unwrap();
        match &cmds[0] {
            Command::Upsert { id, .. } => assert_eq!(id, "_"),
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn negative_value_bare() {
        let cmds = parse("!pointerenter 0 root x=-91.4 y=-101.0").unwrap();
        match &cmds[0] {
            Command::Event { props, .. } => {
                assert_eq!(props[0], Prop::val("x", "-91.4"));
                assert_eq!(props[1], Prop::val("y", "-101.0"));
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn negative_value_in_upsert() {
        let cmds = parse("+view x translate-x=-100 translate-y=-50.5").unwrap();
        match &cmds[0] {
            Command::Upsert { props, .. } => {
                assert_eq!(props[0], Prop::val("translate-x", "-100"));
                assert_eq!(props[1], Prop::val("translate-y", "-50.5"));
            }
            _ => panic!("expected Upsert"),
        }
    }

    #[test]
    fn negative_value_does_not_affect_destroy() {
        // `-view id` must still parse as a destroy command, not a negative value
        let cmds = parse("+view root class=test -view root").unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], Command::Upsert { .. }));
        assert!(
            matches!(&cmds[1], Command::Destroy { kind, id } if kind == "view" && id == "root")
        );
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
            Command::Upsert { id, .. } => assert_eq!(id, "notes-app:sidebar"),
            _ => panic!("expected Upsert"),
        }
    }

    // -- Dot-qualified types --------------------------------------------------

    #[test]
    fn dot_qualified_type() {
        let cmds = parse("+org.mybrowser.WebView wv1").unwrap();
        match &cmds[0] {
            Command::Upsert { kind, .. } => assert_eq!(kind, "org.mybrowser.WebView"),
            _ => panic!("expected Upsert"),
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
                assert_eq!(kind, "view");
                assert_eq!(id, "sidebar");
                assert_eq!(props[0], Prop::val("class", "w-64"));
            }
            _ => panic!("expected Upsert"),
        }
        assert!(matches!(&cmds[1], Command::Push { .. }));
        match &cmds[2] {
            Command::Upsert { kind, id, props } => {
                assert_eq!(kind, "text");
                assert_eq!(id, "label");
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
            em.claim("button")?;
            em.unclaim("slider")
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
        assert!(matches!(
            &cmds[0],
            Command::Event {
                kind,
                seq: 0,
                id,
                ..
            } if kind == &EventKind::Click && id == "save"
        ));
        assert!(matches!(
            &cmds[1],
            Command::Ack {
                kind,
                seq: 0,
                ..
            } if kind == &EventKind::Click
        ));
        match &cmds[2] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Claim);
                assert_eq!(targets[0], "button");
            }
            _ => panic!("expected Claim"),
        }
        match &cmds[3] {
            Command::Pragma { kind, targets } => {
                assert_eq!(*kind, PragmaKind::Unclaim);
                assert_eq!(targets[0], "slider");
            }
            _ => panic!("expected Unclaim"),
        }
    }

    #[test]
    fn round_trip_escape_encoding() {
        use crate::emitter::Emitter;

        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| em.upsert("text", "msg", &[Prop::val("content", "it's a \"test\"")]))
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
