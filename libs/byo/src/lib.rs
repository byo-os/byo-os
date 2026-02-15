//! byo — escape code library for BYO/OS
//!
//! Provides APC sequence parsing and emission for the BYO/OS protocol.
//! All communication in BYO/OS happens via ECMA-48 APC escape sequences
//! over stdin/stdout.

pub mod emitter;
pub mod lexer;
pub mod parser;
pub mod protocol;
pub mod scanner;
pub mod types;

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
