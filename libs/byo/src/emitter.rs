//! Escape code emitter for producing BYO/OS protocol messages.
//!
//! Provides a typed [`Emitter`] struct that writes well-formed BYO/OS APC
//! sequences to any [`io::Write`] sink.

use std::io;

use crate::protocol::{APC_START, PROTOCOL_ID, Prop, ST};

/// Returns `true` if `value` can be written bare (unquoted).
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
        })
}

/// Writes a prop value, auto-quoting when necessary.
fn write_value<W: io::Write>(w: &mut W, value: &str) -> io::Result<()> {
    if is_bare(value) {
        write!(w, "{value}")
    } else if !value.contains('"') {
        write!(w, "\"{value}\"")
    } else if !value.contains('\'') {
        write!(w, "'{value}'")
    } else {
        panic!("value contains both single and double quotes, cannot be encoded: {value:?}");
    }
}

/// Writes a slice of [`Prop`].
///
/// [`Prop::Remove`] writes `~key` on the wire. In upsert/event context
/// this is a no-op for the receiver, but we emit it unconditionally to
/// keep the emitter simple.
fn write_props<W: io::Write>(w: &mut W, props: &[Prop<'_>]) -> io::Result<()> {
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
        self.writer.write_all(ST)
    }

    // -- Object commands -----------------------------------------------------

    /// `+type id props...` — Create or full-replace an object.
    pub fn upsert(&mut self, kind: &str, id: &str, props: &[Prop<'_>]) -> io::Result<()> {
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
        props: &[Prop<'_>],
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
    pub fn patch(&mut self, kind: &str, id: &str, props: &[Prop<'_>]) -> io::Result<()> {
        write!(self.writer, "\n@{kind} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `@type id props... { children }` — Patch with children.
    pub fn patch_with(
        &mut self,
        kind: &str,
        id: &str,
        props: &[Prop<'_>],
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
    pub fn event(&mut self, kind: &str, seq: u64, id: &str, props: &[Prop<'_>]) -> io::Result<()> {
        write!(self.writer, "\n!{kind} {seq} {id}")?;
        write_props(&mut self.writer, props)
    }

    /// `!ack kind seq props...` — Acknowledge a received event.
    pub fn ack(&mut self, kind: &str, seq: u64, props: &[Prop<'_>]) -> io::Result<()> {
        write!(self.writer, "\n!ack {kind} {seq}")?;
        write_props(&mut self.writer, props)
    }

    /// `!sub seq type` — Subscribe to an object type.
    pub fn sub(&mut self, seq: u64, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n!sub {seq} {target_type}")
    }

    /// `!unsub seq type` — Unsubscribe from an object type.
    pub fn unsub(&mut self, seq: u64, target_type: &str) -> io::Result<()> {
        write!(self.writer, "\n!unsub {seq} {target_type}")
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
    #[should_panic(expected = "both single and double quotes")]
    fn value_with_both_quotes_panics() {
        emit(|em| em.upsert("text", "msg", &[Prop::val("content", "it's a \"test\"")]));
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
    fn sub_unsub() {
        let out = emit(|em| {
            em.sub(0, "button")?;
            em.sub(1, "slider")?;
            em.unsub(2, "checkbox")
        });
        assert_eq!(
            out,
            "\x1b_B\n!sub 0 button\n!sub 1 slider\n!unsub 2 checkbox\n\x1b\\"
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
        assert_eq!(
            Prop::val("k", "v"),
            Prop::Value {
                key: "k",
                value: "v"
            }
        );
        assert_eq!(Prop::flag("k"), Prop::Boolean { key: "k" });
        assert_eq!(Prop::remove("k"), Prop::Remove { key: "k" });
    }

    #[test]
    fn event_kind_as_str() {
        use crate::protocol::EventKind;
        assert_eq!(EventKind::Click.as_str(), "click");
        assert_eq!(EventKind::KeyDown.as_str(), "keydown");
        assert_eq!(EventKind::KeyUp.as_str(), "keyup");
        assert_eq!(EventKind::Pointer.as_str(), "pointer");
        assert_eq!(EventKind::Scroll.as_str(), "scroll");
        assert_eq!(EventKind::Focus.as_str(), "focus");
        assert_eq!(EventKind::Blur.as_str(), "blur");
        assert_eq!(EventKind::Resize.as_str(), "resize");
        assert_eq!(EventKind::Expand.as_str(), "expand");
        assert_eq!(
            EventKind::Other("com.example.foo").as_str(),
            "com.example.foo"
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
}
