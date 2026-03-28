//! byo — escape code library for BYO/OS
//!
//! Provides APC sequence parsing and emission for the BYO/OS protocol.
//! All communication in BYO/OS happens via ECMA-48 APC escape sequences
//! over stdin/stdout.

pub mod assert;
pub mod byte_str;
pub mod channel;
pub mod emitter;
pub mod events;
pub mod kitty_gfx;
pub mod lexer;
pub mod parser;
pub mod props;
pub mod protocol;
pub mod scanner;
pub mod tree;
pub mod types;
pub mod vars;

pub use byte_str::{ByteStr, ByteString};
pub use props::{FromProps, ReadProp, ToProps, WriteProp};
pub use protocol::*;

#[cfg(feature = "macros")]
/// Emit a framed APC batch to stdout. Panics on write error.
///
/// Uses near-wire-syntax with Rust expression interpolation:
///
/// ```
/// use byo::byo;
///
/// fn example() {
///     byo! {
///         +view sidebar class="w-64" {
///             +text label content="Hello"
///         }
///     };
/// }
/// ```
pub use byo_macros::byo;

#[cfg(feature = "macros")]
/// Emit commands to an existing [`Emitter`](emitter::Emitter) (no framing).
/// Returns `io::Result<()>`.
///
/// ```
/// use byo::emitter::Emitter;
/// use byo::byo_write;
///
/// let mut buf = Vec::new();
/// let mut em = Emitter::new(&mut buf);
/// em.frame(|em| {
///     byo_write!(em,
///         +view sidebar class="w-64" {
///             +text label content="Hello"
///         }
///     )
/// }).unwrap();
///
/// let out = String::from_utf8(buf).unwrap();
/// assert!(out.contains("+view sidebar"));
/// assert!(out.contains("+text label"));
/// ```
pub use byo_macros::byo_write;

#[cfg(feature = "macros")]
/// Serialize BYO DSL to a `&'static str` at compile time.
///
/// Same syntax as `byo!` but produces a string literal instead of emitter
/// calls. Does not support interpolation, conditionals, or loops.
///
/// ```
/// use byo::byo_str;
///
/// let expected: &str = byo_str!(+view sidebar class="w-64");
/// assert!(expected.contains("+view sidebar"));
/// ```
pub use byo_macros::byo_str;

#[cfg(feature = "macros")]
/// Assert that actual BYO output matches expected DSL structurally.
///
/// First argument is the actual output (`&str`). Remaining tokens are the
/// expected BYO DSL (literals only). Parses both sides and compares
/// structurally; panics with a clear diff on mismatch.
///
/// ```
/// use byo::byo_assert_eq;
///
/// let actual = "\n+view sidebar class=\"w-64\"";
/// byo_assert_eq!(actual, +view sidebar class="w-64");
/// ```
pub use byo_macros::byo_assert_eq;

#[cfg(feature = "macros")]
/// Build comma-separated `Command` enum expressions from BYO DSL.
///
/// For simple cases, produces bare expressions for splicing into `vec![...]`.
/// For complex cases (control flow, children), produces a block returning `Vec<Command>`.
///
/// ```
/// use byo::byo_commands;
/// use byo::protocol::Command;
///
/// let cmds: Vec<Command> = vec![byo_commands! { +view sidebar class="w-64" }];
/// assert_eq!(cmds.len(), 1);
/// ```
pub use byo_macros::byo_commands;

#[cfg(feature = "macros")]
/// Build a `Vec<Command>` from BYO DSL.
///
/// ```
/// use byo::byo_vec;
/// use byo::protocol::Command;
///
/// let cmds: Vec<Command> = byo_vec! {
///     +view sidebar class="w-64"
///     -view old
/// };
/// assert_eq!(cmds.len(), 2);
/// ```
pub use byo_macros::byo_vec;

#[cfg(feature = "macros")]
pub use byo_macros::FromProps;

#[cfg(feature = "macros")]
pub use byo_macros::ToProps;

#[cfg(feature = "macros")]
pub use byo_macros::ReadProp;

#[cfg(feature = "macros")]
pub use byo_macros::WriteProp;

/// Extract typed values from a `&[Prop]` slice in a single pass, zero allocations.
///
/// Each entry declares a local variable bound to the parsed value. When the prop
/// key is absent, the variable gets [`FromPropValue::absent()`] (typically
/// `Default::default()`) or a custom default specified with `|`.
///
/// Use `Option<T>` to distinguish "absent" from "zero":
///
/// ```
/// use byo::protocol::Prop;
/// use byo::byo_read_props;
///
/// let props = [Prop::val("x", "42"), Prop::val("label", "hi")];
/// byo_read_props!(&props,
///     x: f64 = "x",
///     y: f64 = "y",
///     height: f64 = "height" | 1.0,
///     label: String = "label",
///     missing: Option<f64> = "missing",
/// );
/// assert_eq!(x, 42.0);
/// assert_eq!(y, 0.0);         // absent → default
/// assert_eq!(height, 1.0);    // absent → custom default
/// assert_eq!(label, "hi");
/// assert!(missing.is_none()); // absent → None
/// ```
#[macro_export]
macro_rules! byo_read_props {
    ($props:expr, $($name:ident : $ty:ty = $key:literal $(| $default:expr)?),* $(,)?) => {
        $(let mut $name: $ty = $crate::byo_read_props!(@default $ty $(, $default)?);)*
        for prop in $props {
            if let $crate::Prop::Value { key, value } = prop {
                match key.as_ref() {
                    $($key => {
                        if let Some(v) = <$ty as $crate::FromPropValue>::from_prop(value.as_ref()) {
                            $name = v;
                        }
                    })*
                    _ => {}
                }
            }
        }
    };
    (@default $ty:ty, $default:expr) => { $default };
    (@default $ty:ty) => { <$ty as Default>::default() };
}
