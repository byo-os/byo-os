//! Core types for the BYO/OS protocol.
//!
//! These types are shared by both the parser and emitter. The parser
//! produces [`Command`] values borrowing from the input buffer; the
//! emitter consumes [`Prop`] slices to write wire-format output.
//!
//! # Wire format overview
//!
//! All communication happens inside ECMA-48 APC escape sequences:
//!
//! ```text
//! ESC _ B <commands...> ESC \
//! ```
//!
//! Commands use single-character operators:
//!
//! | Op  | Form             | Meaning                          |
//! |-----|------------------|----------------------------------|
//! | `+` | `+type id props` | Create or full-replace (upsert)  |
//! | `-` | `-type id`       | Destroy (including children)     |
//! | `@` | `@type id props` | Patch props / set context        |
//! | `{` | `{`              | Begin children block             |
//! | `}` | `}`              | End children block               |
//! | `!` | `!type seq id`   | Event                            |
//!
//! # Properties
//!
//! Properties are key-value pairs following the type and ID:
//!
//! ```text
//! +view sidebar class="w-64" order=0 hidden
//! ```
//!
//! - `key=value` — set a property (value auto-quoted if needed)
//! - `key` (bare) — boolean flag
//! - `~key` — remove a property (patch only)
//!
//! All values are strings on the wire. Callers convert numeric or
//! boolean values to strings before passing them (e.g. `&n.to_string()`).

use std::borrow::Cow;

/// APC introducer: ESC _
pub const APC_START: &[u8] = b"\x1b_";

/// String Terminator: ESC \
pub const ST: &[u8] = b"\x1b\\";

/// BYO/OS protocol identifier (first byte after APC start)
pub const PROTOCOL_ID: u8 = b'B';

/// Graphics protocol identifier (kitty graphics protocol, `G` prefix)
pub const GRAPHICS_PROTOCOL_ID: u8 = b'G';

/// Anonymous object ID (`_`). Cannot be updated, deleted, or referenced.
pub const ANON: &str = "_";

/// A property on an object.
///
/// Used by all command types. In upsert (`+`) and event (`!`) contexts,
/// [`Remove`](Prop::Remove) is a no-op (full replace has no prior state
/// to remove from). In patch (`@`) context, all three variants are
/// meaningful.
///
/// # Examples
///
/// ```
/// use byo::protocol::Prop;
///
/// let props = [
///     Prop::val("class", "w-64"),
///     Prop::val("order", "0"),
///     Prop::flag("hidden"),
///     Prop::remove("tooltip"),
/// ];
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prop<'a> {
    /// `key=value` — set a property
    Value { key: &'a str, value: Cow<'a, str> },
    /// `key` (bare) — boolean flag
    Boolean { key: &'a str },
    /// `~key` — remove a property (no-op in upsert/event context)
    Remove { key: &'a str },
}

impl<'a> Prop<'a> {
    /// Create a key=value property.
    pub fn val(key: &'a str, value: impl Into<Cow<'a, str>>) -> Self {
        Self::Value {
            key,
            value: value.into(),
        }
    }

    /// Create a boolean flag property.
    pub fn flag(key: &'a str) -> Self {
        Self::Boolean { key }
    }

    /// Remove a property. No-op in upsert/event context.
    pub fn remove(key: &'a str) -> Self {
        Self::Remove { key }
    }
}

/// A parsed BYO/OS protocol command.
///
/// Each variant carries only the fields valid for that operation,
/// making invalid states unrepresentable. Borrows from the input
/// buffer for zero-copy parsing.
///
/// # Variants
///
/// | Variant   | Wire syntax           | Description                          |
/// |-----------|-----------------------|--------------------------------------|
/// | `Upsert`  | `+type id props...`   | Create or full-replace (idempotent)  |
/// | `Destroy` | `-type id`            | Remove object and its children       |
/// | `Push`    | `{`                   | Begin children of preceding `+`/`@`  |
/// | `Pop`     | `}`                   | End children block                   |
/// | `Patch`   | `@type id props...`   | Update specific props on an object   |
/// | `Event`   | `!type seq id props`  | Input or system event                |
/// | `Ack`     | `!ack type seq props` | Acknowledge a received event         |
/// | `Sub`     | `!sub seq type`       | Subscribe to an object type          |
/// | `Unsub`   | `!unsub seq type`     | Unsubscribe from an object type      |
#[derive(Debug, Clone)]
pub enum Command<'a> {
    /// `+type id props...` — Create or update (full replace, idempotent).
    /// ID is `_` for anonymous objects.
    Upsert {
        kind: &'a str,
        id: &'a str,
        props: Vec<Prop<'a>>,
    },
    /// `-type id` — Destroy an object and its children.
    Destroy { kind: &'a str, id: &'a str },
    /// `{` — Push (begin children of the preceding `+`/`@` target).
    Push,
    /// `}` — Pop (end children context).
    Pop,
    /// `@type id props...` — Patch props and/or set context on an existing object.
    Patch {
        kind: &'a str,
        id: &'a str,
        props: Vec<Prop<'a>>,
    },
    /// `!type seq id props...` — Event. Known built-in event names
    /// parse as keywords; unknown events use the generic form.
    Event {
        kind: EventKind<'a>,
        seq: u64,
        id: &'a str,
        props: Vec<Prop<'a>>,
    },
    /// `!ack type seq props...` — Acknowledge a received event.
    Ack {
        kind: EventKind<'a>,
        seq: u64,
        props: Vec<Prop<'a>>,
    },
    /// `!sub seq type` — Subscribe to an object type.
    Sub { seq: u64, target_type: &'a str },
    /// `!unsub seq type` — Unsubscribe from an object type.
    Unsub { seq: u64, target_type: &'a str },
}

/// Known built-in event types and a generic fallback for
/// unknown/third-party events.
///
/// Built-in event names are unqualified (e.g. `click`, `keydown`).
/// Third-party events use dot-qualified names (e.g. `com.example.spell-check`)
/// and parse as [`Other`](EventKind::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind<'a> {
    // Input events
    Click,
    KeyDown,
    KeyUp,
    Pointer,
    Scroll,
    Focus,
    Blur,
    Resize,

    // System events
    Expand,

    /// Unknown or third-party event (e.g. `com.example.spell-check`)
    Other(&'a str),
}

impl<'a> EventKind<'a> {
    /// Returns the wire-format string for this event kind.
    pub fn as_str(&self) -> &str {
        match self {
            EventKind::Click => "click",
            EventKind::KeyDown => "keydown",
            EventKind::KeyUp => "keyup",
            EventKind::Pointer => "pointer",
            EventKind::Scroll => "scroll",
            EventKind::Focus => "focus",
            EventKind::Blur => "blur",
            EventKind::Resize => "resize",
            EventKind::Expand => "expand",
            EventKind::Other(s) => s,
        }
    }

    /// Maps a wire-format event name to the corresponding variant.
    ///
    /// Known built-in names map to their keyword variants; everything
    /// else (including third-party dot-qualified names) maps to `Other`.
    pub fn from_wire(s: &'a str) -> Self {
        match s {
            "click" => EventKind::Click,
            "keydown" => EventKind::KeyDown,
            "keyup" => EventKind::KeyUp,
            "pointer" => EventKind::Pointer,
            "scroll" => EventKind::Scroll,
            "focus" => EventKind::Focus,
            "blur" => EventKind::Blur,
            "resize" => EventKind::Resize,
            "expand" => EventKind::Expand,
            other => EventKind::Other(other),
        }
    }
}
