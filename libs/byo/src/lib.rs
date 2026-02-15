//! byo — escape code library for BYO/OS
//!
//! Provides APC sequence parsing and emission for the BYO/OS protocol.
//! All communication in BYO/OS happens via ECMA-48 APC escape sequences
//! over stdin/stdout.

pub mod emitter;
pub mod protocol;
pub mod scanner;
pub mod types;

pub use protocol::*;
