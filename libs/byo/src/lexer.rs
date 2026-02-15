//! Lexer for BYO/OS protocol payloads.
//!
//! Tokenizes a `&str` payload (the content between `ESC _ B` and `ESC \`)
//! into a sequence of [`Spanned`] tokens. The token stream is then consumed
//! by the [parser](crate::parser).
//!
//! The [`Token`] type is the shared interface between the runtime lexer and
//! a future proc-macro tokenizer — both produce the same token vocabulary.

use std::borrow::Cow;
use std::fmt;

// -- Position / Span ----------------------------------------------------------

/// Byte offset into the input string.
pub type Pos = usize;

/// A byte range within the input string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Span {
    /// Creates a zero-width span at the given offset (used for EOF errors).
    pub fn at(pos: Pos) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }
}

/// A token annotated with its source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<'a> {
    pub token: Token<'a>,
    pub span: Span,
}

// -- Token --------------------------------------------------------------------

/// A lexical token from a BYO/OS protocol payload.
///
/// The parser determines the semantic role of each token (type name, ID,
/// prop name, sequence number, value) from its position in the grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    /// `+` — upsert operator
    Plus,
    /// `-` — destroy operator
    Minus,
    /// `@` — patch operator
    At,
    /// `!` — event operator
    Bang,
    /// `{` — begin children
    LBrace,
    /// `}` — end children
    RBrace,
    /// `~` — remove prop prefix
    Tilde,
    /// `=` — prop assignment
    Eq,
    /// Bare word: type names, IDs, sequence numbers, unquoted values.
    Word(&'a str),
    /// Quoted string (decoded). Borrowed if no escapes, owned if escapes present.
    Str(Cow<'a, str>),
}

// -- Error --------------------------------------------------------------------

/// A parse/lex error with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at byte {}..{}",
            self.kind, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ParseError {}

/// The kind of parse/lex error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    // -- Lexer errors ---------------------------------------------------------
    /// Quoted string reached end of input without closing quote.
    UnterminatedString,
    /// Unrecognized escape sequence (e.g. `\q`).
    InvalidEscape(char),
    /// Unexpected byte that doesn't match any token pattern.
    UnexpectedByte(u8),

    // -- Parser errors --------------------------------------------------------
    /// Expected a specific token kind but found something else.
    Expected {
        expected: &'static str,
        found: String,
    },
    /// Sequence number is not a valid `u64`.
    InvalidSeqnum,
    /// `{` without matching `}`.
    UnclosedBrace,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseErrorKind::UnterminatedString => write!(f, "unterminated string"),
            ParseErrorKind::InvalidEscape(ch) => write!(f, "invalid escape sequence: \\{ch}"),
            ParseErrorKind::UnexpectedByte(b) => write!(f, "unexpected byte: 0x{b:02x}"),
            ParseErrorKind::Expected { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            ParseErrorKind::InvalidSeqnum => write!(f, "invalid sequence number"),
            ParseErrorKind::UnclosedBrace => write!(f, "unclosed '{{' without matching '}}'"),
        }
    }
}

// -- Tokenizer ----------------------------------------------------------------

/// Tokenizes a BYO/OS protocol payload string into a sequence of spanned tokens.
///
/// The input should be the payload content between `ESC _ B` and `ESC \`
/// (not including the protocol identifier byte).
///
/// # Operator characters inside words
///
/// Operator characters (`+`, `-`, `@`, `!`) are only recognized as operators
/// at the start of a token. Inside a word they are regular characters, so
/// hyphenated names and qualified IDs tokenize as single words:
///
/// ```
/// use byo::lexer::{tokenize, Token};
///
/// let tokens = tokenize("+view root baz-boop").unwrap();
/// let words: Vec<_> = tokens.iter().map(|s| &s.token).collect();
/// assert_eq!(words, &[&Token::Plus, &Token::Word("view"), &Token::Word("root"), &Token::Word("baz-boop")]);
/// ```
///
/// # Quoted strings with escapes
///
/// Quoted strings decode backslash escapes. When no escapes are present
/// the token borrows directly from the input (zero-copy):
///
/// ```
/// use byo::lexer::{tokenize, Token};
/// use std::borrow::Cow;
///
/// let tokens = tokenize(r#""hello" "say \"hi\"""#).unwrap();
/// assert_eq!(tokens[0].token, Token::Str(Cow::Borrowed("hello")));
/// assert_eq!(tokens[1].token, Token::Str(Cow::Owned("say \"hi\"".into())));
/// ```
///
/// # Errors
///
/// Returns a [`ParseError`] on unterminated strings, invalid escape sequences,
/// or unexpected bytes (e.g. bare backslash).
///
/// ```
/// use byo::lexer::{tokenize, ParseErrorKind};
///
/// let err = tokenize("\"unterminated").unwrap_err();
/// assert_eq!(err.kind, ParseErrorKind::UnterminatedString);
/// ```
pub fn tokenize(input: &str) -> Result<Vec<Spanned<'_>>, ParseError> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip whitespace
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;

        match bytes[i] {
            b'+' => {
                tokens.push(Spanned {
                    token: Token::Plus,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'-' => {
                tokens.push(Spanned {
                    token: Token::Minus,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'@' => {
                tokens.push(Spanned {
                    token: Token::At,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'!' => {
                tokens.push(Spanned {
                    token: Token::Bang,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'{' => {
                tokens.push(Spanned {
                    token: Token::LBrace,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'}' => {
                tokens.push(Spanned {
                    token: Token::RBrace,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'~' => {
                tokens.push(Spanned {
                    token: Token::Tilde,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'=' => {
                tokens.push(Spanned {
                    token: Token::Eq,
                    span: Span { start, end: i + 1 },
                });
                i += 1;
            }
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1; // skip opening quote
                let mut has_escapes = false;
                let content_start = i;

                // Scan to find end, checking for escapes
                loop {
                    if i >= bytes.len() {
                        return Err(ParseError {
                            kind: ParseErrorKind::UnterminatedString,
                            span: Span {
                                start,
                                end: bytes.len(),
                            },
                        });
                    }
                    if bytes[i] == quote {
                        break;
                    }
                    if bytes[i] == b'\\' {
                        has_escapes = true;
                        i += 1; // skip past backslash
                        if i >= bytes.len() {
                            return Err(ParseError {
                                kind: ParseErrorKind::UnterminatedString,
                                span: Span {
                                    start,
                                    end: bytes.len(),
                                },
                            });
                        }
                        // Validate escape character
                        match bytes[i] {
                            b'"' | b'\'' | b'\\' | b'/' | b'n' | b'r' | b't' | b'0' => {}
                            _ => {
                                let ch = input[i..].chars().next().unwrap_or('?');
                                return Err(ParseError {
                                    kind: ParseErrorKind::InvalidEscape(ch),
                                    span: Span {
                                        start: i - 1,
                                        end: i + ch.len_utf8(),
                                    },
                                });
                            }
                        }
                    }
                    i += 1;
                }

                let content_end = i;
                i += 1; // skip closing quote

                let value = if has_escapes {
                    Cow::Owned(decode_escapes(&input[content_start..content_end]))
                } else {
                    Cow::Borrowed(&input[content_start..content_end])
                };

                tokens.push(Spanned {
                    token: Token::Str(value),
                    span: Span {
                        start,
                        end: i, // includes both quotes
                    },
                });
            }
            b'\\' => {
                return Err(ParseError {
                    kind: ParseErrorKind::UnexpectedByte(b'\\'),
                    span: Span {
                        start,
                        end: start + 1,
                    },
                });
            }
            _ => {
                // Bare word: consume until whitespace or special char
                while i < bytes.len() && !is_special_or_ws(bytes[i]) {
                    i += 1;
                }
                tokens.push(Spanned {
                    token: Token::Word(&input[start..i]),
                    span: Span { start, end: i },
                });
            }
        }
    }

    Ok(tokens)
}

/// Returns `true` if the byte terminates a bare word.
///
/// Operator characters (`+`, `-`, `@`, `!`) are NOT included here — they
/// are valid inside bare words (e.g. `notes-app:save`, `w-64`). They're
/// only recognized as operators at token start via the main match arm.
fn is_special_or_ws(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(b, b'{' | b'}' | b'~' | b'=' | b'"' | b'\'' | b'\\')
}

/// Decodes backslash escape sequences in a string slice.
///
/// Assumes all escapes have already been validated by the lexer.
fn decode_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next().expect("validated by lexer") {
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                _ => unreachable!("validated by lexer"),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: tokenize and return just the tokens (no spans).
    fn toks(input: &str) -> Vec<Token<'_>> {
        tokenize(input).unwrap().into_iter().map(|s| s.token).collect()
    }

    #[test]
    fn empty_input() {
        assert_eq!(toks(""), vec![]);
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(toks("   \n\t  "), vec![]);
    }

    #[test]
    fn single_char_operators() {
        assert_eq!(
            toks("+ - @ ! { } ~ ="),
            vec![
                Token::Plus,
                Token::Minus,
                Token::At,
                Token::Bang,
                Token::LBrace,
                Token::RBrace,
                Token::Tilde,
                Token::Eq,
            ]
        );
    }

    #[test]
    fn operators_no_spaces() {
        assert_eq!(
            toks("+view{"),
            vec![Token::Plus, Token::Word("view"), Token::LBrace]
        );
    }

    #[test]
    fn bare_words() {
        assert_eq!(
            toks("view sidebar 42 com.example.foo"),
            vec![
                Token::Word("view"),
                Token::Word("sidebar"),
                Token::Word("42"),
                Token::Word("com.example.foo"),
            ]
        );
    }

    #[test]
    fn double_quoted_string_no_escapes() {
        assert_eq!(
            toks("\"hello world\""),
            vec![Token::Str(Cow::Borrowed("hello world"))]
        );
    }

    #[test]
    fn single_quoted_string_no_escapes() {
        assert_eq!(
            toks("'hello world'"),
            vec![Token::Str(Cow::Borrowed("hello world"))]
        );
    }

    #[test]
    fn double_quoted_string_with_escapes() {
        assert_eq!(
            toks(r#""say \"hello\"""#),
            vec![Token::Str(Cow::Owned("say \"hello\"".to_string()))]
        );
    }

    #[test]
    fn all_escape_sequences() {
        assert_eq!(
            toks(r#""\"\'\\\n\r\t\0\/""#),
            vec![Token::Str(Cow::Owned(
                "\"\'\\\n\r\t\0/".to_string()
            ))]
        );
    }

    #[test]
    fn empty_quoted_string() {
        assert_eq!(toks("\"\""), vec![Token::Str(Cow::Borrowed(""))]);
    }

    #[test]
    fn unterminated_string() {
        let err = tokenize("\"hello").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnterminatedString);
    }

    #[test]
    fn invalid_escape() {
        let err = tokenize(r#""\q""#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::InvalidEscape('q'));
    }

    #[test]
    fn bare_backslash_error() {
        let err = tokenize("hello \\ world").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnexpectedByte(b'\\'));
    }

    #[test]
    fn upsert_command() {
        assert_eq!(
            toks("+view sidebar class=\"w-64\" order=0 hidden"),
            vec![
                Token::Plus,
                Token::Word("view"),
                Token::Word("sidebar"),
                Token::Word("class"),
                Token::Eq,
                Token::Str(Cow::Borrowed("w-64")),
                Token::Word("order"),
                Token::Eq,
                Token::Word("0"),
                Token::Word("hidden"),
            ]
        );
    }

    #[test]
    fn patch_with_remove() {
        assert_eq!(
            toks("@view sidebar ~tooltip hidden"),
            vec![
                Token::At,
                Token::Word("view"),
                Token::Word("sidebar"),
                Token::Tilde,
                Token::Word("tooltip"),
                Token::Word("hidden"),
            ]
        );
    }

    #[test]
    fn event_command() {
        assert_eq!(
            toks("!click 0 save"),
            vec![
                Token::Bang,
                Token::Word("click"),
                Token::Word("0"),
                Token::Word("save"),
            ]
        );
    }

    #[test]
    fn children_block() {
        assert_eq!(
            toks("+view root { +text child }"),
            vec![
                Token::Plus,
                Token::Word("view"),
                Token::Word("root"),
                Token::LBrace,
                Token::Plus,
                Token::Word("text"),
                Token::Word("child"),
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn spans_are_correct() {
        let spanned = tokenize("+view x").unwrap();
        assert_eq!(spanned[0].span, Span { start: 0, end: 1 }); // +
        assert_eq!(spanned[1].span, Span { start: 1, end: 5 }); // view
        assert_eq!(spanned[2].span, Span { start: 6, end: 7 }); // x
    }

    #[test]
    fn string_span_includes_quotes() {
        let spanned = tokenize("\"ab\"").unwrap();
        assert_eq!(spanned[0].span, Span { start: 0, end: 4 });
    }

    #[test]
    fn qualified_id_is_word() {
        assert_eq!(
            toks("notes-app:save"),
            vec![Token::Word("notes-app:save")]
        );
    }

    #[test]
    fn minus_as_operator_before_word() {
        assert_eq!(
            toks("-view item3"),
            vec![Token::Minus, Token::Word("view"), Token::Word("item3")]
        );
    }

    #[test]
    fn full_batch_payload() {
        // A realistic batch payload (sans APC framing)
        let input = concat!(
            "+layer content order=0\n",
            "+view greeting class=\"p-4\" {\n",
            "  +text label content=\"Hello, world\" class=\"text-2xl text-white\"\n",
            "}\n",
        );
        let tokens = toks(input);
        assert_eq!(tokens[0], Token::Plus);
        assert_eq!(tokens[1], Token::Word("layer"));
        // Just verify it parsed without error and has reasonable length
        assert!(tokens.len() > 15);
    }
}
