//! Escape code emitter for producing BYO/OS protocol messages.
//!
//! Provides a typed [`Emitter`] struct that writes well-formed BYO/OS APC
//! sequences to any [`io::Write`] sink.

use std::io;

use crate::protocol::{APC_START, Command, PROTOCOL_ID, Prop, ST};

/// Returns `true` if `value` can be written bare (unquoted).
pub fn is_bare(value: &str) -> bool {
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

/// Writes a prop value, auto-quoting when necessary.
///
/// When the value contains both single and double quotes, falls back to
/// double-quoting with backslash escaping.
pub fn write_value<W: io::Write>(w: &mut W, value: &str) -> io::Result<()> {
    if is_bare(value) {
        write!(w, "{value}")
    } else if !value.contains('"') {
        write!(w, "\"{value}\"")
    } else if !value.contains('\'') {
        write!(w, "'{value}'")
    } else {
        // Both quote types present — double-quote with escaping
        w.write_all(b"\"")?;
        for ch in value.chars() {
            match ch {
                '"' => w.write_all(b"\\\"")?,
                '\\' => w.write_all(b"\\\\")?,
                '\n' => w.write_all(b"\\n")?,
                '\r' => w.write_all(b"\\r")?,
                '\t' => w.write_all(b"\\t")?,
                '\0' => w.write_all(b"\\0")?,
                _ => write!(w, "{ch}")?,
            }
        }
        w.write_all(b"\"")
    }
}

/// Writes a slice of [`Prop`].
///
/// [`Prop::Remove`] writes `~key` on the wire. In upsert/event context
/// this is a no-op for the receiver, but we emit it unconditionally to
/// keep the emitter simple.
pub fn write_props<W: io::Write>(w: &mut W, props: &[Prop]) -> io::Result<()> {
    for prop in props {
        match prop {
            Prop::Value { key, value } => {
                write!(w, " {key}=")?;
                write_value(w, value)?;
            }
            Prop::Boolean { key } => {
                write!(w, " {key}")?;
            }
            Prop::Remove { key } => {
                write!(w, " ~{key}")?;
            }
        }
    }
    Ok(())
}

/// Typed emitter for writing BYO/OS protocol commands.
///
/// Wraps any [`io::Write`] sink. Use [`frame`](Emitter::frame) to emit
/// a complete APC batch, calling command methods inside the closure.
///
/// # Example
///
/// ```
/// use byo::emitter::Emitter;
/// use byo::protocol::Prop;
///
/// let mut buf = Vec::new();
/// let mut em = Emitter::new(&mut buf);
///
/// em.frame(|em| {
///     em.upsert("view", "sidebar", &[
///         Prop::val("class", "w-64"),
///     ])
/// }).unwrap();
///
/// let out = String::from_utf8(buf).unwrap();
/// assert!(out.starts_with("\x1b_B"));
/// assert!(out.ends_with("\x1b\\"));
/// ```
pub struct Emitter<W: io::Write> {
    writer: W,
}

impl<W: io::Write> Emitter<W> {
    /// Creates a new emitter wrapping the given writer.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Consumes the emitter and returns the inner writer.
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Write pre-serialized BYO command bytes verbatim.
    ///
    /// Use this to splice a previously-built buffer into the output stream.
    /// Intended for use inside `_with` closures to inject pre-computed
    /// children, e.g.:
    ///
    /// ```
    /// # use byo::emitter::Emitter;
    /// # let mut buf = Vec::new();
    /// # let mut em = Emitter::new(&mut buf);
    /// // Build children into a buffer first...
    /// let child_buf = b"\n+view child class=inner";
    ///
    /// // ...then splice via raw() inside a _with closure:
    /// em.upsert_with("view", "root", &[], |em| em.raw(child_buf)).unwrap();
    /// # let out = String::from_utf8(buf).unwrap();
    /// # assert!(out.contains("+view root"));
    /// # assert!(out.contains("{"));
    /// # assert!(out.contains("+view child"));
    /// # assert!(out.contains("}"));
    /// ```
    ///
    /// In debug builds, the bytes are parsed and validated to catch
    /// malformed content early. In release builds, they are written
    /// verbatim with no overhead.
    pub fn raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(debug_assertions)]
        {
            if let Ok(s) = std::str::from_utf8(bytes) {
                debug_assert!(
                    crate::parser::parse(s).is_ok(),
                    "raw() received invalid BYO payload: {s:?}"
                );
            }
        }
        self.writer.write_all(bytes)
    }

    // -- Batch framing -------------------------------------------------------

    /// Writes a complete APC batch (`ESC _ B ... \nESC \`).
    ///
    /// The closure receives `&mut Emitter<W>` and may call any command
    /// methods. The APC introducer and String Terminator are emitted
    /// automatically, guaranteeing well-formed framing at compile time.
    ///
    /// ```
    /// use byo::emitter::Emitter;
    /// use byo::protocol::Prop;
    ///
    /// let mut buf = Vec::new();
    /// let mut em = Emitter::new(&mut buf);
    ///
    /// em.frame(|em| {
    ///     em.upsert("layer", "content", &[Prop::val("order", "0")])?;
    ///     em.upsert("view", "greeting", &[])
    /// }).unwrap();
    ///
    /// let out = String::from_utf8(buf).unwrap();
    /// assert!(out.contains("+layer content"));
    /// assert!(out.contains("+view greeting"));
    /// ```
    pub fn frame(&mut self, commands: impl FnOnce(&mut Self) -> io::Result<()>) -> io::Result<()> {
        self.writer.write_all(APC_START)?;
        self.writer.write_all(&[PROTOCOL_ID])?;
        commands(self)?;
        self.writer.write_all(b"\n")?;
        self.writer.write_all(ST)?;
        self.writer.flush()
    }

    // -- Object commands -----------------------------------------------------

    /// `+type id props...` — Create or full-replace an object.
    pub fn upsert(&mut self, kind: &str, id: &str, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n+{kind} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `+type id props... { children }` — Upsert with children.
    ///
    /// The closure receives `&mut Emitter<W>` and may call any command
    /// methods. `{` and `}` are emitted automatically, guaranteeing
    /// balanced braces at compile time.
    ///
    /// ```
    /// use byo::emitter::Emitter;
    /// use byo::protocol::Prop;
    ///
    /// let mut buf = Vec::new();
    /// let mut em = Emitter::new(&mut buf);
    ///
    /// em.frame(|em| {
    ///     em.upsert_with("view", "sidebar", &[
    ///         Prop::val("class", "w-64"),
    ///     ], |em| {
    ///         em.upsert("text", "label", &[
    ///             Prop::val("content", "Hello"),
    ///         ])
    ///     })
    /// }).unwrap();
    ///
    /// let out = String::from_utf8(buf).unwrap();
    /// assert!(out.contains("+view sidebar"));
    /// assert!(out.contains("+text label"));
    /// ```
    pub fn upsert_with(
        &mut self,
        kind: &str,
        id: &str,
        props: &[Prop],
        children: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<()> {
        write!(self.writer, "\n+{kind} {id}")?;
        write_props(&mut self.writer, props)?;
        self.writer.write_all(b" {")?;
        children(self)?;
        self.writer.write_all(b"\n}")
    }

    /// `-type id` — Destroy an object and its children.
    pub fn destroy(&mut self, kind: &str, id: &str) -> io::Result<()> {
        write!(self.writer, "\n-{kind} {id}")
    }

    /// `@type id props...` — Patch props on an existing object.
    pub fn patch(&mut self, kind: &str, id: &str, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n@{kind} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `@type id props... { children }` — Patch with children.
    pub fn patch_with(
        &mut self,
        kind: &str,
        id: &str,
        props: &[Prop],
        children: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<()> {
        write!(self.writer, "\n@{kind} {id}")?;
        write_props(&mut self.writer, props)?;
        self.writer.write_all(b" {")?;
        children(self)?;
        self.writer.write_all(b"\n}")
    }

    // -- Events --------------------------------------------------------------

    /// `!kind seq id props...` — Emit an event.
    pub fn event(&mut self, kind: &str, seq: u64, id: &str, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n!{kind} {seq} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `!ack kind seq props...` — Acknowledge a received event.
    pub fn ack(&mut self, kind: &str, seq: u64, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n!ack {kind} {seq}")?;
        write_props(&mut self.writer, props)
    }

    // -- Pragmas (#) ---------------------------------------------------------

    /// `#claim type` — Claim ownership of an object type.
    pub fn claim(&mut self, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n#claim {target_type}")
    }

    /// `#claim type,type,...` — Claim multiple types.
    pub fn claim_many(&mut self, types: &[&str]) -> io::Result<()> {
        write!(self.writer, "\n#claim ")?;
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                write!(self.writer, ",")?;
            }
            write!(self.writer, "{t}")?;
        }
        Ok(())
    }

    /// `#unclaim type` — Release claim on an object type.
    pub fn unclaim(&mut self, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n#unclaim {target_type}")
    }

    /// `#unclaim type,type,...` — Release claim on multiple types.
    pub fn unclaim_many(&mut self, types: &[&str]) -> io::Result<()> {
        write!(self.writer, "\n#unclaim ")?;
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                write!(self.writer, ",")?;
            }
            write!(self.writer, "{t}")?;
        }
        Ok(())
    }

    /// `#observe type` — Observe final output for an object type.
    pub fn observe(&mut self, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n#observe {target_type}")
    }

    /// `#observe type,type,...` — Observe multiple types.
    pub fn observe_many(&mut self, types: &[&str]) -> io::Result<()> {
        write!(self.writer, "\n#observe ")?;
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                write!(self.writer, ",")?;
            }
            write!(self.writer, "{t}")?;
        }
        Ok(())
    }

    /// `#unobserve type` — Stop observing an object type.
    pub fn unobserve(&mut self, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n#unobserve {target_type}")
    }

    /// `#unobserve type,type,...` — Stop observing multiple types.
    pub fn unobserve_many(&mut self, types: &[&str]) -> io::Result<()> {
        write!(self.writer, "\n#unobserve ")?;
        for (i, t) in types.iter().enumerate() {
            if i > 0 {
                write!(self.writer, ",")?;
            }
            write!(self.writer, "{t}")?;
        }
        Ok(())
    }

    /// `#redirect target` — Route passthrough to named tty.
    pub fn redirect(&mut self, target: &str) -> io::Result<()> {
        write!(self.writer, "\n#redirect {target}")
    }

    /// `#unredirect` — Restore default passthrough routing.
    pub fn unredirect(&mut self) -> io::Result<()> {
        write!(self.writer, "\n#unredirect")
    }

    // -- Requests/Responses ---------------------------------------------------

    /// `?expand seq id props...` — Request daemon expansion.
    pub fn expand(&mut self, seq: u64, id: &str, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n?expand {seq} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `.expand seq { body }` — Expansion response (closure-based).
    pub fn expanded_with(
        &mut self,
        seq: u64,
        body: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<()> {
        write!(self.writer, "\n.expand {seq} {{")?;
        body(self)?;
        self.writer.write_all(b"\n}")
    }

    /// `?kind seq target props...` — Generic request.
    pub fn request(
        &mut self,
        kind: &str,
        seq: u64,
        target: &str,
        props: &[Prop],
    ) -> io::Result<()> {
        write!(self.writer, "\n?{kind} {seq} {target}")?;
        write_props(&mut self.writer, props)
    }

    /// `.kind seq props... { body }` — Generic response with body (closure-based).
    pub fn response_with(
        &mut self,
        kind: &str,
        seq: u64,
        props: &[Prop],
        body: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<()> {
        write!(self.writer, "\n.{kind} {seq}")?;
        write_props(&mut self.writer, props)?;
        self.writer.write_all(b" {")?;
        body(self)?;
        self.writer.write_all(b"\n}")
    }

    /// `.kind seq props...` — Generic response without body.
    pub fn response(&mut self, kind: &str, seq: u64, props: &[Prop]) -> io::Result<()> {
        write!(self.writer, "\n.{kind} {seq}")?;
        write_props(&mut self.writer, props)
    }

    // -- Bulk emission --------------------------------------------------------

    /// Emit a slice of parsed [`Command`] values.
    ///
    /// Writes each command in order, translating `Push`/`Pop` into `{`/`}`.
    /// This enables round-tripping: `parse()` → `commands()`.
    ///
    /// This does NOT write APC framing — call it inside a [`frame`](Self::frame)
    /// closure or write framing separately.
    ///
    /// ```
    /// use byo::emitter::Emitter;
    /// use byo::parser::parse;
    ///
    /// let cmds = parse("+view root { +text child }").unwrap();
    ///
    /// let mut buf = Vec::new();
    /// let mut em = Emitter::new(&mut buf);
    /// em.frame(|em| em.commands(&cmds)).unwrap();
    ///
    /// let out = String::from_utf8(buf).unwrap();
    /// assert!(out.contains("+view root"));
    /// assert!(out.contains("+text child"));
    /// ```
    pub fn commands(&mut self, cmds: &[Command]) -> io::Result<()> {
        for cmd in cmds {
            match cmd {
                Command::Upsert { kind, id, props } => {
                    write!(self.writer, "\n+{kind} {id}")?;
                    write_props(&mut self.writer, props)?;
                }
                Command::Destroy { kind, id } => {
                    write!(self.writer, "\n-{kind} {id}")?;
                }
                Command::Push => {
                    self.writer.write_all(b" {")?;
                }
                Command::Pop => {
                    self.writer.write_all(b"\n}")?;
                }
                Command::Patch { kind, id, props } => {
                    write!(self.writer, "\n@{kind} {id}")?;
                    write_props(&mut self.writer, props)?;
                }
                Command::Event {
                    kind,
                    seq,
                    id,
                    props,
                } => {
                    write!(self.writer, "\n!{} {seq} {id}", kind.as_str())?;
                    write_props(&mut self.writer, props)?;
                }
                Command::Ack { kind, seq, props } => {
                    write!(self.writer, "\n!ack {} {seq}", kind.as_str())?;
                    write_props(&mut self.writer, props)?;
                }
                Command::Pragma { kind, targets } => {
                    write!(self.writer, "\n#{}", kind.as_str())?;
                    match kind {
                        crate::protocol::PragmaKind::Claim
                        | crate::protocol::PragmaKind::Unclaim
                        | crate::protocol::PragmaKind::Observe
                        | crate::protocol::PragmaKind::Unobserve => {
                            for (i, t) in targets.iter().enumerate() {
                                if i > 0 {
                                    write!(self.writer, ",")?;
                                } else {
                                    write!(self.writer, " ")?;
                                }
                                write!(self.writer, "{t}")?;
                            }
                        }
                        crate::protocol::PragmaKind::Redirect => {
                            if let Some(target) = targets.first() {
                                write!(self.writer, " {target}")?;
                            }
                        }
                        _ => {
                            for (i, t) in targets.iter().enumerate() {
                                if i > 0 {
                                    write!(self.writer, ",")?;
                                } else {
                                    write!(self.writer, " ")?;
                                }
                                write!(self.writer, "{t}")?;
                            }
                        }
                    }
                }
                Command::Request {
                    kind,
                    seq,
                    targets,
                    props,
                } => {
                    write!(self.writer, "\n?{} {seq}", kind.as_str())?;
                    if let Some(target) = targets.first() {
                        write!(self.writer, " {target}")?;
                    }
                    write_props(&mut self.writer, props)?;
                }
                Command::Response {
                    kind,
                    seq,
                    props,
                    body,
                } => {
                    write!(self.writer, "\n.{} {seq}", kind.as_str())?;
                    write_props(&mut self.writer, props)?;
                    if let Some(body) = body {
                        self.writer.write_all(b" {")?;
                        self.commands(body)?;
                        self.writer.write_all(b"\n}")?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: run emitter commands inside a frame and return the output string.
    fn emit(f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(f).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn upsert_bare_props() {
        let out = emit(|em| {
            em.upsert(
                "view",
                "sidebar",
                &[Prop::val("class", "w-64"), Prop::val("order", "0")],
            )
        });
        assert_eq!(out, "\x1b_B\n+view sidebar class=w-64 order=0\n\x1b\\");
    }

    #[test]
    fn upsert_quoted_value() {
        let out = emit(|em| em.upsert("view", "sidebar", &[Prop::val("class", "px-4 py-2")]));
        assert_eq!(out, "\x1b_B\n+view sidebar class=\"px-4 py-2\"\n\x1b\\");
    }

    #[test]
    fn upsert_single_quote_fallback() {
        let out = emit(|em| em.upsert("text", "msg", &[Prop::val("content", "say \"hello\"")]));
        assert_eq!(out, "\x1b_B\n+text msg content='say \"hello\"'\n\x1b\\");
    }

    #[test]
    fn value_with_both_quotes_escapes() {
        let out = emit(|em| em.upsert("text", "msg", &[Prop::val("content", "it's a \"test\"")]));
        assert_eq!(
            out,
            "\x1b_B\n+text msg content=\"it's a \\\"test\\\"\"\n\x1b\\"
        );
    }

    #[test]
    fn upsert_boolean_flag() {
        let out = emit(|em| em.upsert("view", "sidebar", &[Prop::flag("hidden")]));
        assert_eq!(out, "\x1b_B\n+view sidebar hidden\n\x1b\\");
    }

    #[test]
    fn upsert_anonymous() {
        let out = emit(|em| em.upsert("view", "_", &[]));
        assert_eq!(out, "\x1b_B\n+view _\n\x1b\\");
    }

    #[test]
    fn destroy() {
        let out = emit(|em| em.destroy("view", "item3"));
        assert_eq!(out, "\x1b_B\n-view item3\n\x1b\\");
    }

    #[test]
    fn patch_with_remove() {
        let out = emit(|em| {
            em.patch(
                "view",
                "sidebar",
                &[Prop::flag("hidden"), Prop::remove("tooltip")],
            )
        });
        assert_eq!(out, "\x1b_B\n@view sidebar hidden ~tooltip\n\x1b\\");
    }

    #[test]
    fn patch_set_value() {
        let out = emit(|em| em.patch("view", "sidebar", &[Prop::val("order", "1")]));
        assert_eq!(out, "\x1b_B\n@view sidebar order=1\n\x1b\\");
    }

    #[test]
    fn upsert_with_children() {
        let out = emit(|em| {
            em.upsert_with("view", "sidebar", &[Prop::val("class", "w-64")], |em| {
                em.upsert("view", "child1", &[])?;
                em.upsert("view", "child2", &[])
            })
        });
        assert_eq!(
            out,
            "\x1b_B\n+view sidebar class=w-64 {\n+view child1\n+view child2\n}\n\x1b\\"
        );
    }

    #[test]
    fn patch_with_children() {
        let out = emit(|em| {
            em.patch_with("view", "sidebar", &[], |em| {
                em.upsert("view", "item4", &[Prop::val("class", "px-4")])
            })
        });
        assert_eq!(
            out,
            "\x1b_B\n@view sidebar {\n+view item4 class=px-4\n}\n\x1b\\"
        );
    }

    #[test]
    fn event() {
        let out = emit(|em| em.event("click", 0, "save", &[]));
        assert_eq!(out, "\x1b_B\n!click 0 save\n\x1b\\");
    }

    #[test]
    fn event_with_props() {
        let out = emit(|em| {
            em.event(
                "keydown",
                0,
                "editor",
                &[Prop::val("key", "a"), Prop::val("mod", "ctrl")],
            )
        });
        assert_eq!(out, "\x1b_B\n!keydown 0 editor key=a mod=ctrl\n\x1b\\");
    }

    #[test]
    fn ack() {
        let out = emit(|em| em.ack("click", 0, &[Prop::val("handled", "true")]));
        assert_eq!(out, "\x1b_B\n!ack click 0 handled=true\n\x1b\\");
    }

    #[test]
    fn claim_unclaim() {
        let out = emit(|em| {
            em.claim("button")?;
            em.claim("slider")?;
            em.unclaim("checkbox")
        });
        assert_eq!(
            out,
            "\x1b_B\n#claim button\n#claim slider\n#unclaim checkbox\n\x1b\\"
        );
    }

    #[test]
    fn observe_unobserve() {
        let out = emit(|em| {
            em.observe("view")?;
            em.observe("text")?;
            em.unobserve("view")
        });
        assert_eq!(
            out,
            "\x1b_B\n#observe view\n#observe text\n#unobserve view\n\x1b\\"
        );
    }

    #[test]
    fn redirect() {
        let out = emit(|em| em.redirect("term"));
        assert_eq!(out, "\x1b_B\n#redirect term\n\x1b\\");
    }

    #[test]
    fn unredirect() {
        let out = emit(|em| em.unredirect());
        assert_eq!(out, "\x1b_B\n#unredirect\n\x1b\\");
    }

    #[test]
    fn expand_request() {
        let out = emit(|em| em.expand(0, "notes-app:save", &[Prop::val("kind", "button")]));
        assert_eq!(out, "\x1b_B\n?expand 0 notes-app:save kind=button\n\x1b\\");
    }

    #[test]
    fn expanded_with_body() {
        let out = emit(|em| {
            em.expanded_with(0, |em| {
                em.upsert("view", "save-root", &[Prop::val("class", "btn")])
            })
        });
        assert_eq!(
            out,
            "\x1b_B\n.expand 0 {\n+view save-root class=btn\n}\n\x1b\\"
        );
    }

    #[test]
    fn generic_request() {
        let out = emit(|em| em.request("render-frame", 0, "viewport", &[]));
        assert_eq!(out, "\x1b_B\n?render-frame 0 viewport\n\x1b\\");
    }

    #[test]
    fn generic_response_no_body() {
        let out = emit(|em| em.response("render-frame", 0, &[Prop::val("status", "ok")]));
        assert_eq!(out, "\x1b_B\n.render-frame 0 status=ok\n\x1b\\");
    }

    #[test]
    fn generic_response_with_body() {
        let out = emit(|em| {
            em.response_with("render-frame", 0, &[Prop::val("status", "ok")], |em| {
                em.upsert("view", "frame", &[])
            })
        });
        assert_eq!(
            out,
            "\x1b_B\n.render-frame 0 status=ok {\n+view frame\n}\n\x1b\\"
        );
    }

    #[test]
    fn full_batch_round_trip() {
        let out = emit(|em| {
            em.upsert_with("layer", "content", &[Prop::val("order", "0")], |em| {
                em.upsert_with("view", "greeting", &[Prop::val("class", "p-4")], |em| {
                    em.upsert(
                        "text",
                        "label",
                        &[
                            Prop::val("content", "Hello, world"),
                            Prop::val("class", "text-2xl text-white"),
                        ],
                    )
                })
            })
        });
        assert_eq!(
            out,
            concat!(
                "\x1b_B",
                "\n+layer content order=0 {",
                "\n+view greeting class=p-4 {",
                "\n+text label content=\"Hello, world\" class=\"text-2xl text-white\"",
                "\n}",
                "\n}",
                "\n\x1b\\",
            )
        );
    }

    #[test]
    fn value_quoting_special_chars() {
        let out = emit(|em| em.upsert("view", "x", &[Prop::val("data", "a=b")]));
        assert_eq!(out, "\x1b_B\n+view x data=\"a=b\"\n\x1b\\");
    }

    #[test]
    fn value_quoting_braces() {
        let out = emit(|em| em.upsert("view", "x", &[Prop::val("data", "{hi}")]));
        assert_eq!(out, "\x1b_B\n+view x data=\"{hi}\"\n\x1b\\");
    }

    #[test]
    fn value_quoting_tilde() {
        let out = emit(|em| em.upsert("view", "x", &[Prop::val("path", "~user")]));
        assert_eq!(out, "\x1b_B\n+view x path=\"~user\"\n\x1b\\");
    }

    #[test]
    fn empty_value_quoted() {
        let out = emit(|em| em.upsert("view", "x", &[Prop::val("label", "")]));
        assert_eq!(out, "\x1b_B\n+view x label=\"\"\n\x1b\\");
    }

    #[test]
    fn value_with_single_quotes() {
        let out = emit(|em| em.upsert("text", "x", &[Prop::val("content", "it's here")]));
        assert_eq!(out, "\x1b_B\n+text x content=\"it's here\"\n\x1b\\");
    }

    #[test]
    fn into_inner() {
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| em.upsert("view", "x", &[])).unwrap();
        let inner = em.into_inner();
        assert!(!inner.is_empty());
    }

    #[test]
    fn multiple_frames() {
        let mut buf = Vec::new();
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| em.upsert("view", "a", &[])).unwrap();
        em.frame(|em| em.destroy("view", "a")).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out, "\x1b_B\n+view a\n\x1b\\\x1b_B\n-view a\n\x1b\\");
    }

    #[test]
    fn patch_with_props_and_children() {
        let out = emit(|em| {
            em.patch_with("view", "sidebar", &[Prop::flag("hidden")], |em| {
                em.upsert("view", "child", &[])
            })
        });
        assert_eq!(
            out,
            "\x1b_B\n@view sidebar hidden {\n+view child\n}\n\x1b\\"
        );
    }

    #[test]
    fn upsert_no_props() {
        let out = emit(|em| em.upsert("view", "x", &[]));
        assert_eq!(out, "\x1b_B\n+view x\n\x1b\\");
    }

    #[test]
    fn upsert_with_anon_constant() {
        use crate::protocol::ANON;
        let out = emit(|em| em.upsert("view", ANON, &[]));
        assert_eq!(out, "\x1b_B\n+view _\n\x1b\\");
    }

    #[test]
    fn prop_constructors() {
        use crate::byte_str::ByteStr;
        assert_eq!(
            Prop::val("k", "v"),
            Prop::Value {
                key: ByteStr::from("k"),
                value: ByteStr::from("v"),
            }
        );
        assert_eq!(
            Prop::flag("k"),
            Prop::Boolean {
                key: ByteStr::from("k")
            }
        );
        assert_eq!(
            Prop::remove("k"),
            Prop::Remove {
                key: ByteStr::from("k")
            }
        );
    }

    #[test]
    fn event_kind_as_str() {
        use crate::byte_str::ByteStr;
        use crate::protocol::EventKind;
        assert_eq!(EventKind::PointerDown.as_str(), "pointerdown");
        assert_eq!(EventKind::PointerUp.as_str(), "pointerup");
        assert_eq!(EventKind::PointerMove.as_str(), "pointermove");
        assert_eq!(EventKind::PointerOver.as_str(), "pointerover");
        assert_eq!(EventKind::PointerOut.as_str(), "pointerout");
        assert_eq!(EventKind::PointerEnter.as_str(), "pointerenter");
        assert_eq!(EventKind::PointerLeave.as_str(), "pointerleave");
        assert_eq!(EventKind::PointerCancel.as_str(), "pointercancel");
        assert_eq!(EventKind::GotPointerCapture.as_str(), "gotpointercapture");
        assert_eq!(EventKind::LostPointerCapture.as_str(), "lostpointercapture");
        assert_eq!(EventKind::Click.as_str(), "click");
        assert_eq!(EventKind::AuxClick.as_str(), "auxclick");
        assert_eq!(EventKind::DblClick.as_str(), "dblclick");
        assert_eq!(EventKind::Scroll.as_str(), "scroll");
        assert_eq!(EventKind::KeyDown.as_str(), "keydown");
        assert_eq!(EventKind::KeyUp.as_str(), "keyup");
        assert_eq!(EventKind::Focus.as_str(), "focus");
        assert_eq!(EventKind::Blur.as_str(), "blur");
        assert_eq!(EventKind::Resize.as_str(), "resize");
        assert_eq!(
            EventKind::Other(ByteStr::from("com.example.foo")).as_str(),
            "com.example.foo"
        );
    }

    #[test]
    fn pragma_kind_as_str() {
        use crate::byte_str::ByteStr;
        use crate::protocol::PragmaKind;
        assert_eq!(PragmaKind::Claim.as_str(), "claim");
        assert_eq!(PragmaKind::Unclaim.as_str(), "unclaim");
        assert_eq!(PragmaKind::Observe.as_str(), "observe");
        assert_eq!(PragmaKind::Unobserve.as_str(), "unobserve");
        assert_eq!(PragmaKind::Redirect.as_str(), "redirect");
        assert_eq!(PragmaKind::Unredirect.as_str(), "unredirect");
        assert_eq!(
            PragmaKind::Other(ByteStr::from("custom")).as_str(),
            "custom"
        );
    }

    #[test]
    fn request_kind_as_str() {
        use crate::byte_str::ByteStr;
        use crate::protocol::RequestKind;
        assert_eq!(RequestKind::Expand.as_str(), "expand");
        assert_eq!(
            RequestKind::Other(ByteStr::from("render-frame")).as_str(),
            "render-frame"
        );
    }

    #[test]
    fn response_kind_as_str() {
        use crate::byte_str::ByteStr;
        use crate::protocol::ResponseKind;
        assert_eq!(ResponseKind::Expand.as_str(), "expand");
        assert_eq!(
            ResponseKind::Other(ByteStr::from("render-frame")).as_str(),
            "render-frame"
        );
    }

    #[test]
    fn event_with_event_kind() {
        use crate::protocol::EventKind;
        let out = emit(|em| em.event(EventKind::Click.as_str(), 42, "save", &[]));
        assert_eq!(out, "\x1b_B\n!click 42 save\n\x1b\\");
    }

    #[test]
    fn ack_no_props() {
        let out = emit(|em| em.ack("keydown", 7, &[]));
        assert_eq!(out, "\x1b_B\n!ack keydown 7\n\x1b\\");
    }

    #[test]
    fn commands_round_trip() {
        use crate::parser::parse;

        let input = concat!(
            "+view sidebar class=\"w-64\" {",
            "\n+text label content=\"Hello\"",
            "\n}",
            "\n-view old",
            "\n@view sidebar hidden",
            "\n!click 0 save",
            "\n!ack click 0 handled=true",
            "\n#claim button",
        );
        let cmds = parse(input).unwrap();

        let out = emit(|em| em.commands(&cmds));

        // Parse the re-emitted output and compare
        let payload = out
            .strip_prefix("\x1b_B")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();
        let cmds2 = parse(payload).unwrap();
        assert_eq!(cmds.len(), cmds2.len());
    }

    #[test]
    fn commands_empty() {
        let out = emit(|em| em.commands(&[]));
        assert_eq!(out, "\x1b_B\n\x1b\\");
    }

    #[test]
    fn observe_many_output() {
        let out = emit(|em| em.observe_many(&["view", "text", "layer"]));
        assert_eq!(out, "\x1b_B\n#observe view,text,layer\n\x1b\\");
    }

    #[test]
    fn claim_many_output() {
        let out = emit(|em| em.claim_many(&["button", "slider"]));
        assert_eq!(out, "\x1b_B\n#claim button,slider\n\x1b\\");
    }

    #[test]
    fn commands_round_trip_multi_type() {
        use crate::parser::parse;

        let cmds = parse("#observe view,text,layer").unwrap();
        let out = emit(|em| em.commands(&cmds));
        let payload = out
            .strip_prefix("\x1b_B")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();
        let cmds2 = parse(payload).unwrap();
        assert_eq!(cmds.len(), cmds2.len());
        match &cmds2[0] {
            Command::Pragma {
                kind: crate::protocol::PragmaKind::Observe,
                targets,
            } => {
                assert_eq!(targets.len(), 3);
                assert_eq!(targets[0], "view");
                assert_eq!(targets[1], "text");
                assert_eq!(targets[2], "layer");
            }
            _ => panic!("expected Observe"),
        }
    }

    #[test]
    fn value_with_comma_quoted() {
        let out = emit(|em| em.upsert("view", "x", &[Prop::val("data", "a,b")]));
        assert_eq!(out, "\x1b_B\n+view x data=\"a,b\"\n\x1b\\");
    }
}
