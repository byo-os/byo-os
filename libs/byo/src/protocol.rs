//! Core types for the BYO/OS protocol.
//!
//! These types are shared by both the parser and emitter. The parser
//! produces [`Command`] values with [`ByteStr`] fields for zero-copy
//! owned strings; the emitter consumes [`Prop`] slices to write
//! wire-format output.
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

use crate::byte_str::ByteStr;

/// APC introducer: ESC _
pub const APC_START: &[u8] = b"\x1b_";

/// String Terminator: ESC \
pub const ST: &[u8] = b"\x1b\\";

/// BYO/OS protocol identifier (first byte after APC start)
pub const PROTOCOL_ID: u8 = b'B';

/// Graphics protocol identifier (kitty graphics protocol, `G` prefix)
pub const KITTY_GFX_PROTOCOL_ID: u8 = b'G';

/// Anonymous object ID (`_`). Cannot be updated, deleted, or referenced.
pub const ANON: &str = "_";

/// Strip APC framing (`ESC _ B ... \n ESC \`) from a BYO payload string.
///
/// Returns the inner payload if framing is present, or the original
/// string unchanged if not.
pub fn strip_apc(s: &str) -> &str {
    s.strip_prefix("\x1b_B")
        .and_then(|s| s.strip_suffix("\x1b\\"))
        .and_then(|s| s.strip_suffix('\n'))
        .unwrap_or(s)
}

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
pub enum Prop {
    /// `key=value` — set a property
    Value { key: ByteStr, value: ByteStr },
    /// `key` (bare) — boolean flag
    Boolean { key: ByteStr },
    /// `~key` — remove a property (no-op in upsert/event context)
    Remove { key: ByteStr },
}

impl Prop {
    /// Create a key=value property.
    pub fn val(key: impl Into<ByteStr>, value: impl Into<ByteStr>) -> Self {
        Self::Value {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Create a boolean flag property.
    pub fn flag(key: impl Into<ByteStr>) -> Self {
        Self::Boolean { key: key.into() }
    }

    /// Remove a property. No-op in upsert/event context.
    pub fn remove(key: impl Into<ByteStr>) -> Self {
        Self::Remove { key: key.into() }
    }

    /// Returns the key name, regardless of variant.
    pub fn key(&self) -> &str {
        match self {
            Self::Value { key, .. } | Self::Boolean { key } | Self::Remove { key } => key,
        }
    }
}

/// A parsed BYO/OS protocol command.
///
/// Each variant carries only the fields valid for that operation,
/// making invalid states unrepresentable. Uses [`ByteStr`] for
/// zero-copy owned strings that can cross thread boundaries.
///
/// # Variants
///
/// | Variant   | Wire syntax           | Description                          |
/// |-----------|-----------------------|--------------------------------------|
/// | `Upsert`  | `+type id props...`   | Create or full-replace (idempotent)  |
/// | `Destroy` | `-type id`            | Remove object and its children       |
/// | `Push`    | `{` / `{name`         | Begin children (optionally slotted)  |
/// | `Pop`     | `}`                   | End children block                   |
/// | `Patch`   | `@type id props...`   | Update specific props on an object   |
/// | `Event`   | `!type seq id props`  | Input or system event                |
/// | `Ack`     | `!ack type seq props` | Acknowledge a received event         |
/// | `Pragma`  | `#kind targets`       | Stream pragma (fire-and-forget)      |
/// | `Request` | `?kind seq target`    | Request (expand, custom)             |
/// | `Response`| `.kind seq props body`| Response (expand, custom)            |
/// | `Message` | `.kind target props`  | Standalone message (fire-and-forget) |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `+type id props...` — Create or update (full replace, idempotent).
    /// ID is `_` for anonymous objects.
    Upsert {
        kind: ByteStr,
        id: ByteStr,
        props: Vec<Prop>,
    },
    /// `-type id` — Destroy an object and its children.
    Destroy { kind: ByteStr, id: ByteStr },
    /// `{` / `{name` — Push (begin children of the preceding `+`/`@` target).
    ///
    /// When `slot` is `Some(name)`, this is a **slot push** (`{name`) — a
    /// tagged children block used for content projection during daemon
    /// expansion. Slot names are consumed by the orchestrator during
    /// rewrite and never reach the compositor. `{_}` is the default slot.
    ///
    /// When `slot` is `None`, this is a regular push (`{`).
    Push { slot: Option<ByteStr> },
    /// `}` — Pop (end children context).
    Pop,
    /// `@type id props...` — Patch props and/or set context on an existing object.
    Patch {
        kind: ByteStr,
        id: ByteStr,
        props: Vec<Prop>,
    },
    /// `!type seq id props...` — Event. Known built-in event names
    /// parse as keywords; unknown events use the generic form.
    Event {
        kind: EventKind,
        seq: u64,
        id: ByteStr,
        props: Vec<Prop>,
    },
    /// `!ack type seq props...` — Acknowledge a received event.
    Ack {
        kind: EventKind,
        seq: u64,
        props: Vec<Prop>,
    },
    /// `#kind ...` — Stream pragma (fire-and-forget, no seq).
    Pragma(PragmaKind),
    /// `?kind seq target props...` — Request (expand, custom).
    Request {
        kind: RequestKind,
        seq: u64,
        targets: Vec<ByteStr>,
        props: Vec<Prop>,
    },
    /// `.kind seq props... [{ body }]` — Response (expand, custom).
    Response {
        kind: ResponseKind,
        seq: u64,
        props: Vec<Prop>,
        body: Option<Vec<Command>>,
    },
    /// `.kind target props... [{ body }]` — Standalone message (no seq, no response expected).
    ///
    /// Routed like a request but fire-and-forget. Distinguished from
    /// `Response` on the wire by the absence of a sequence number:
    /// the token after the kind name starts with a letter (target ID)
    /// rather than a digit (seq number).
    Message {
        kind: MessageKind,
        target: ByteStr,
        props: Vec<Prop>,
        body: Option<Vec<Command>>,
    },
}

/// Known built-in event types and a generic fallback for
/// unknown/third-party events.
///
/// Built-in event names are unqualified (e.g. `click`, `keydown`).
/// Third-party events use dot-qualified names (e.g. `com.example.spell-check`)
/// and parse as [`Other`](EventKind::Other).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventKind {
    // Pointer events (W3C Pointer Events spec)
    PointerDown,
    PointerUp,
    PointerMove,
    PointerOver,
    PointerOut,
    PointerEnter,
    PointerLeave,
    PointerCancel,
    GotPointerCapture,
    LostPointerCapture,

    // Derived pointer events
    Click,
    AuxClick,
    DblClick,

    // Scroll
    Scroll,

    // Keyboard events
    KeyDown,
    KeyUp,

    // Focus events
    Focus,
    Blur,

    // Window/surface events
    Resize,

    // Semantic control events (synthesized by daemons)
    Press,
    Change,
    Input,

    // Timer events (synthesized by the timer virtual service)
    Fire,

    /// Unknown or third-party event (e.g. `com.example.spell-check`)
    Other(ByteStr),
}

impl EventKind {
    /// Returns the wire-format string for this event kind.
    pub fn as_str(&self) -> &str {
        match self {
            EventKind::PointerDown => "pointerdown",
            EventKind::PointerUp => "pointerup",
            EventKind::PointerMove => "pointermove",
            EventKind::PointerOver => "pointerover",
            EventKind::PointerOut => "pointerout",
            EventKind::PointerEnter => "pointerenter",
            EventKind::PointerLeave => "pointerleave",
            EventKind::PointerCancel => "pointercancel",
            EventKind::GotPointerCapture => "gotpointercapture",
            EventKind::LostPointerCapture => "lostpointercapture",
            EventKind::Click => "click",
            EventKind::AuxClick => "auxclick",
            EventKind::DblClick => "dblclick",
            EventKind::Scroll => "scroll",
            EventKind::KeyDown => "keydown",
            EventKind::KeyUp => "keyup",
            EventKind::Focus => "focus",
            EventKind::Blur => "blur",
            EventKind::Resize => "resize",
            EventKind::Press => "press",
            EventKind::Change => "change",
            EventKind::Input => "input",
            EventKind::Fire => "fire",
            EventKind::Other(s) => s,
        }
    }

    /// Maps a wire-format event name to the corresponding variant.
    ///
    /// Known built-in names map to their keyword variants; everything
    /// else (including third-party dot-qualified names) maps to `Other`.
    pub fn from_wire(s: impl Into<ByteStr>) -> Self {
        let s = s.into();
        match s.as_ref() {
            "pointerdown" => EventKind::PointerDown,
            "pointerup" => EventKind::PointerUp,
            "pointermove" => EventKind::PointerMove,
            "pointerover" => EventKind::PointerOver,
            "pointerout" => EventKind::PointerOut,
            "pointerenter" => EventKind::PointerEnter,
            "pointerleave" => EventKind::PointerLeave,
            "pointercancel" => EventKind::PointerCancel,
            "gotpointercapture" => EventKind::GotPointerCapture,
            "lostpointercapture" => EventKind::LostPointerCapture,
            "click" => EventKind::Click,
            "auxclick" => EventKind::AuxClick,
            "dblclick" => EventKind::DblClick,
            "scroll" => EventKind::Scroll,
            "keydown" => EventKind::KeyDown,
            "keyup" => EventKind::KeyUp,
            "focus" => EventKind::Focus,
            "blur" => EventKind::Blur,
            "resize" => EventKind::Resize,
            "press" => EventKind::Press,
            "change" => EventKind::Change,
            "input" => EventKind::Input,
            "fire" => EventKind::Fire,
            _ => EventKind::Other(s),
        }
    }

    /// Returns `true` if this event type bubbles (participates in propagation).
    pub fn bubbles(&self) -> bool {
        !matches!(
            self,
            EventKind::PointerEnter
                | EventKind::PointerLeave
                | EventKind::Focus
                | EventKind::Blur
                | EventKind::Resize
                | EventKind::Fire
        )
    }

    /// Returns `true` if this event type is cancelable (can be handled/stopped).
    pub fn cancelable(&self) -> bool {
        !matches!(
            self,
            EventKind::PointerEnter
                | EventKind::PointerLeave
                | EventKind::PointerCancel
                | EventKind::GotPointerCapture
                | EventKind::LostPointerCapture
                | EventKind::Focus
                | EventKind::Blur
                | EventKind::Resize
                | EventKind::Fire
        )
    }
}

/// Known pragma types for `#` commands.
///
/// Pragmas are fire-and-forget stream directives (no sequence number,
/// no response expected). Each variant carries its own data, matching
/// the semantics of that pragma kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PragmaKind {
    /// `#claim type,...` — claim ownership of object type(s)
    Claim(Vec<ByteStr>),
    /// `#unclaim type,...` — release claim on object type(s)
    Unclaim(Vec<ByteStr>),
    /// `#observe type,...` — observe final output for object type(s)
    Observe(Vec<ByteStr>),
    /// `#unobserve type,...` — stop observing object type(s)
    Unobserve(Vec<ByteStr>),
    /// `#redirect id` — route passthrough to named tty
    Redirect(ByteStr),
    /// `#unredirect` — restore default passthrough routing
    Unredirect,
    /// `#handle type?request,...` — register as handler for (type, request) pairs
    Handle(Vec<(ByteStr, ByteStr)>),
    /// `#unhandle type?request,...` — release handler registration
    Unhandle(Vec<(ByteStr, ByteStr)>),
    /// `#tap type!event,...` — tap (eavesdrop) events for (type, event) pairs
    Tap(Vec<(ByteStr, ByteStr)>),
    /// `#untap type!event,...` — stop tapping events for (type, event) pairs
    Untap(Vec<(ByteStr, ByteStr)>),
    /// Custom pragma with raw targets
    Other {
        name: ByteStr,
        targets: Vec<ByteStr>,
    },
}

impl PragmaKind {
    /// Returns the wire-format name for this pragma kind.
    pub fn as_str(&self) -> &str {
        match self {
            PragmaKind::Claim(_) => "claim",
            PragmaKind::Unclaim(_) => "unclaim",
            PragmaKind::Observe(_) => "observe",
            PragmaKind::Unobserve(_) => "unobserve",
            PragmaKind::Redirect(_) => "redirect",
            PragmaKind::Unredirect => "unredirect",
            PragmaKind::Handle(_) => "handle",
            PragmaKind::Unhandle(_) => "unhandle",
            PragmaKind::Tap(_) => "tap",
            PragmaKind::Untap(_) => "untap",
            PragmaKind::Other { name, .. } => name,
        }
    }
}

/// Known request types for `?` commands.
///
/// `Expand` expects a `.expand` response from the daemon.
/// Third-party request types use [`Other`](RequestKind::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestKind {
    /// `?expand` — request daemon expansion
    Expand,
    /// Custom request (e.g. `?render-frame`)
    Other(ByteStr),
}

impl RequestKind {
    /// Returns the wire-format string for this request kind.
    pub fn as_str(&self) -> &str {
        match self {
            RequestKind::Expand => "expand",
            RequestKind::Other(s) => s,
        }
    }

    /// Returns `true` if `name` is a reserved built-in request kind.
    ///
    /// Reserved request kinds (like `"expand"`) cannot be registered
    /// via `#handle` — use `#claim` for expansion handling instead.
    pub fn is_reserved(name: &str) -> bool {
        matches!(name, "expand")
    }

    /// Maps a wire-format request name to the corresponding variant.
    pub fn from_wire(s: impl Into<ByteStr>) -> Self {
        let s = s.into();
        match s.as_ref() {
            "expand" => RequestKind::Expand,
            _ => RequestKind::Other(s),
        }
    }
}

/// Known response types for `.` commands.
///
/// `Expand` carries a body (children block) with the expansion result.
/// Third-party response types use [`Other`](ResponseKind::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseKind {
    /// `.expand` — expansion response with body
    Expand,
    /// Custom response (e.g. `.render-frame`)
    Other(ByteStr),
}

impl ResponseKind {
    /// Returns the wire-format string for this response kind.
    pub fn as_str(&self) -> &str {
        match self {
            ResponseKind::Expand => "expand",
            ResponseKind::Other(s) => s,
        }
    }

    /// Maps a wire-format response name to the corresponding variant.
    pub fn from_wire(s: impl Into<ByteStr>) -> Self {
        let s = s.into();
        match s.as_ref() {
            "expand" => ResponseKind::Expand,
            _ => ResponseKind::Other(s),
        }
    }
}

/// Known message types for `.` messages (fire-and-forget, no seq number).
///
/// Built-in message kinds have dedicated variants; third-party or unknown
/// kinds use [`Other`](MessageKind::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    /// `.scroll-to` — set scroll position
    ScrollTo,
    /// `.scroll-by` — adjust scroll position by delta
    ScrollBy,
    /// Custom message
    Other(ByteStr),
}

impl MessageKind {
    /// Returns the wire-format string for this message kind.
    pub fn as_str(&self) -> &str {
        match self {
            MessageKind::ScrollTo => "scroll-to",
            MessageKind::ScrollBy => "scroll-by",
            MessageKind::Other(s) => s,
        }
    }

    /// Maps a wire-format message name to the corresponding variant.
    pub fn from_wire(s: impl Into<ByteStr>) -> Self {
        let s = s.into();
        match s.as_ref() {
            "scroll-to" => MessageKind::ScrollTo,
            "scroll-by" => MessageKind::ScrollBy,
            _ => MessageKind::Other(s),
        }
    }
}

// ---------------------------------------------------------------------------
// FromPropValue — used by the byo_read_props! macro
// ---------------------------------------------------------------------------

/// Parse a typed value from a prop value string.
///
/// Implemented for common numeric types, `bool`, `String`, and `Option<T>`.
/// Used by [`byo_read_props!`](crate::byo_read_props) to extract typed
/// values from a `&[Prop]` slice in a single pass.
pub trait FromPropValue: Sized + Default {
    /// Parse from a prop value string. Returns `None` if parsing fails.
    fn from_prop(s: &str) -> Option<Self>;
}

macro_rules! impl_from_prop_value {
    ($($ty:ty),*) => {
        $(impl FromPropValue for $ty {
            fn from_prop(s: &str) -> Option<Self> { s.parse().ok() }
        })*
    };
}

impl_from_prop_value!(f64, f32, i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, bool);

impl FromPropValue for String {
    fn from_prop(s: &str) -> Option<Self> { Some(s.to_owned()) }
}

impl<T: FromPropValue> FromPropValue for Option<T> {
    fn from_prop(s: &str) -> Option<Self> { T::from_prop(s).map(Some) }
}
