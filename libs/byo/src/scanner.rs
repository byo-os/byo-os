//! APC sequence scanner for BYO/OS protocol messages.
//!
//! Sits above vte: intercepts APC sequences before they reach the
//! terminal emulator, routing BYO/OS protocol messages separately
//! from VT100 content.

use crate::protocol::{GRAPHICS_PROTOCOL_ID, PROTOCOL_ID};

/// Handler for scanner events. All methods have default no-op
/// implementations, so you only need to implement the ones you
/// care about.
pub trait Handler {
    /// Called for each complete BYO/OS APC payload (bytes after
    /// the `B` prefix, before ST).
    fn on_byo(&mut self, _payload: &[u8]) {}

    /// Called for each complete graphics protocol APC payload
    /// (bytes after the `G` prefix, before ST).
    fn on_graphics(&mut self, _payload: &[u8]) {}

    /// Called for bytes that should be forwarded to a VT100 parser
    /// (everything not inside a recognized APC sequence).
    fn on_passthrough(&mut self, _data: &[u8]) {}
}

/// Scanner state for extracting APC sequences from a byte stream.
///
/// Splits a byte stream into three channels via the [`Handler`] trait:
/// - **BYO payloads** (`ESC _ B ... ST`) — protocol commands
/// - **Graphics payloads** (`ESC _ G ... ST`) — graphics data
/// - **Passthrough** — everything else (VT100/plain text)
#[derive(Debug, Default)]
pub struct Scanner {
    state: ScanState,
    buf: Vec<u8>,
}

#[derive(Debug, Default, PartialEq)]
enum ScanState {
    #[default]
    Ground,
    EscSeen,
    InApc,
    InApcEsc,
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes into the scanner, dispatching events to the handler.
    ///
    /// ```
    /// use byo::scanner::{Scanner, Handler};
    ///
    /// struct MyHandler {
    ///     commands: Vec<String>,
    /// }
    ///
    /// impl Handler for MyHandler {
    ///     fn on_byo(&mut self, payload: &[u8]) {
    ///         self.commands.push(String::from_utf8_lossy(payload).into());
    ///     }
    /// }
    ///
    /// let mut scanner = Scanner::new();
    /// let mut handler = MyHandler { commands: vec![] };
    ///
    /// scanner.feed(b"\x1b_B+view sidebar\x1b\\", &mut handler);
    /// assert_eq!(handler.commands, vec!["+view sidebar"]);
    /// ```
    pub fn feed(&mut self, input: &[u8], handler: &mut impl Handler) {
        if input.is_empty() {
            return;
        }

        let mut passthrough_start: usize = 0;
        let mut loop_start: usize = 0;

        // Resolve EscSeen carried over from a previous feed() call.
        // The ESC itself is NOT in the current input — it was the last
        // byte of the previous call. We need input[0] to decide.
        if self.state == ScanState::EscSeen {
            if input[0] == b'_' {
                // ESC _ → APC start
                self.state = ScanState::InApc;
                self.buf.clear();
                passthrough_start = 1;
                loop_start = 1;
            } else {
                // ESC + non-underscore → the ESC is passthrough
                handler.on_passthrough(&[0x1b]);
                self.state = ScanState::Ground;
                // input[0] might itself be ESC, so let the loop handle it
                passthrough_start = 0;
                loop_start = 0;
            }
        }

        for i in loop_start..input.len() {
            let byte = input[i];
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        // Flush passthrough bytes before this ESC
                        if passthrough_start < i {
                            handler.on_passthrough(&input[passthrough_start..i]);
                        }
                        self.state = ScanState::EscSeen;
                    }
                }
                ScanState::EscSeen => {
                    // ESC is at i-1 in the current input (cross-feed
                    // EscSeen is resolved above, so i >= 1 here).
                    if byte == b'_' {
                        // ESC _ → APC start
                        self.state = ScanState::InApc;
                        self.buf.clear();
                    } else {
                        // Not an APC — ESC and this byte are passthrough.
                        // Rewind passthrough_start to include the ESC.
                        passthrough_start = i - 1;
                        self.state = ScanState::Ground;
                    }
                }
                ScanState::InApc => {
                    if byte == 0x1b {
                        self.state = ScanState::InApcEsc;
                    } else {
                        self.buf.push(byte);
                    }
                }
                ScanState::InApcEsc => {
                    if byte == b'\\' {
                        // APC complete (ESC \) — dispatch based on prefix
                        match self.buf.first().copied() {
                            Some(PROTOCOL_ID) => handler.on_byo(&self.buf[1..]),
                            Some(GRAPHICS_PROTOCOL_ID) => handler.on_graphics(&self.buf[1..]),
                            _ => {} // Unknown APC protocol — silently discard
                        }
                        self.buf.clear();
                        self.state = ScanState::Ground;
                        passthrough_start = i + 1;
                    } else {
                        // False alarm — ESC inside APC that isn't ST
                        self.buf.push(0x1b);
                        self.buf.push(byte);
                        self.state = ScanState::InApc;
                    }
                }
            }
        }

        // End-of-input handling
        match self.state {
            ScanState::Ground => {
                if passthrough_start < input.len() {
                    handler.on_passthrough(&input[passthrough_start..]);
                }
            }
            ScanState::EscSeen => {
                // ESC was the last byte of this feed. The bytes before
                // it (from passthrough_start) were already flushed when
                // we saw the ESC in Ground. The ESC itself is held in
                // state — it will be resolved at the start of the next
                // feed() call.
            }
            ScanState::InApc | ScanState::InApcEsc => {
                // Mid-APC — content accumulates in self.buf until ST.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test handler that collects all events into Vecs.
    #[derive(Default)]
    struct Collector {
        byo: Vec<Vec<u8>>,
        graphics: Vec<Vec<u8>>,
        passthrough: Vec<Vec<u8>>,
    }

    impl Handler for Collector {
        fn on_byo(&mut self, payload: &[u8]) {
            self.byo.push(payload.to_vec());
        }
        fn on_graphics(&mut self, payload: &[u8]) {
            self.graphics.push(payload.to_vec());
        }
        fn on_passthrough(&mut self, data: &[u8]) {
            self.passthrough.push(data.to_vec());
        }
    }

    impl Collector {
        fn all_passthrough(&self) -> Vec<u8> {
            self.passthrough
                .iter()
                .flat_map(|c| c.iter().copied())
                .collect()
        }
    }

    /// Helper: feed all input in one call, collect results.
    fn scan_once(input: &[u8]) -> Collector {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();
        scanner.feed(input, &mut c);
        c
    }

    // ── Basic dispatch ──────────────────────────────────────────

    #[test]
    fn byo_payload_dispatched() {
        let c = scan_once(b"\x1b_B+view foo\x1b\\");
        assert_eq!(c.byo, vec![b"+view foo".to_vec()]);
        assert!(c.graphics.is_empty());
        assert!(c.all_passthrough().is_empty());
    }

    #[test]
    fn graphics_payload_dispatched() {
        let c = scan_once(b"\x1b_Ga=t,f=32;data\x1b\\");
        assert!(c.byo.is_empty());
        assert_eq!(c.graphics, vec![b"a=t,f=32;data".to_vec()]);
    }

    #[test]
    fn unknown_apc_discarded() {
        let c = scan_once(b"\x1b_Xunknown\x1b\\");
        assert!(c.byo.is_empty());
        assert!(c.graphics.is_empty());
        assert!(c.all_passthrough().is_empty());
    }

    // ── Passthrough ─────────────────────────────────────────────

    #[test]
    fn plain_text_passthrough() {
        let c = scan_once(b"hello world");
        assert!(c.byo.is_empty());
        assert_eq!(c.all_passthrough(), b"hello world");
    }

    #[test]
    fn passthrough_before_and_after_apc() {
        let c = scan_once(b"before\x1b_B+view x\x1b\\after");
        assert_eq!(c.byo, vec![b"+view x".to_vec()]);
        assert_eq!(c.all_passthrough(), b"beforeafter");
    }

    #[test]
    fn non_apc_escape_is_passthrough() {
        let input = b"\x1b[31mred\x1b[0m";
        let c = scan_once(input);
        assert!(c.byo.is_empty());
        assert_eq!(c.all_passthrough(), input.as_slice());
    }

    // ── Multi-feed / boundary cases ─────────────────────────────

    #[test]
    fn esc_split_across_feeds_into_apc() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"hello\x1b", &mut c);
        scanner.feed(b"_B+view y\x1b\\world", &mut c);

        assert_eq!(c.byo, vec![b"+view y".to_vec()]);
        assert_eq!(c.all_passthrough(), b"helloworld");
    }

    #[test]
    fn esc_split_across_feeds_not_apc() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"abc\x1b", &mut c);
        scanner.feed(b"[31m", &mut c);

        assert_eq!(c.all_passthrough(), b"abc\x1b[31m");
    }

    #[test]
    fn esc_split_then_another_esc() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"abc\x1b", &mut c);
        scanner.feed(b"\x1b_Btest\x1b\\", &mut c);

        assert_eq!(c.byo, vec![b"test".to_vec()]);
        assert_eq!(c.all_passthrough(), b"abc\x1b");
    }

    #[test]
    fn multiple_apc_in_one_feed() {
        let c = scan_once(b"\x1b_B+view a\x1b\\\x1b_B+view b\x1b\\");
        assert_eq!(c.byo.len(), 2);
        assert_eq!(c.byo[0], b"+view a");
        assert_eq!(c.byo[1], b"+view b");
        assert!(c.all_passthrough().is_empty());
    }

    #[test]
    fn interleaved_byo_and_graphics() {
        let c = scan_once(b"\x1b_B+view z\x1b\\\x1b_Gimgdata\x1b\\");
        assert_eq!(c.byo, vec![b"+view z".to_vec()]);
        assert_eq!(c.graphics, vec![b"imgdata".to_vec()]);
    }

    #[test]
    fn apc_split_across_three_feeds() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"\x1b_B+vi", &mut c);
        assert!(c.byo.is_empty());

        scanner.feed(b"ew foo", &mut c);
        assert!(c.byo.is_empty());

        scanner.feed(b"\x1b\\done", &mut c);
        assert_eq!(c.byo, vec![b"+view foo".to_vec()]);
        assert_eq!(c.all_passthrough(), b"done");
    }

    #[test]
    fn false_esc_inside_apc() {
        let c = scan_once(b"\x1b_Bfoo\x1bXbar\x1b\\");
        assert_eq!(c.byo, vec![b"foo\x1bXbar".to_vec()]);
    }

    #[test]
    fn empty_byo_payload() {
        let c = scan_once(b"\x1b_B\x1b\\");
        assert_eq!(c.byo, vec![b"".to_vec()]);
    }

    #[test]
    fn esc_as_very_first_byte_then_apc() {
        let c = scan_once(b"\x1b_Btest\x1b\\");
        assert_eq!(c.byo, vec![b"test".to_vec()]);
        assert!(c.all_passthrough().is_empty());
    }

    #[test]
    fn only_esc_in_feed() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"\x1b", &mut c);
        assert!(c.all_passthrough().is_empty());

        scanner.feed(b"[0m", &mut c);
        assert_eq!(c.all_passthrough(), b"\x1b[0m");
    }

    #[test]
    fn passthrough_between_two_apcs() {
        let c = scan_once(b"\x1b_Ba\x1b\\mid\x1b_Bb\x1b\\");
        assert_eq!(c.byo, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(c.all_passthrough(), b"mid");
    }

    #[test]
    fn empty_input() {
        let c = scan_once(b"");
        assert!(c.byo.is_empty());
        assert!(c.graphics.is_empty());
        assert!(c.passthrough.is_empty());
    }

    #[test]
    fn st_split_across_feeds() {
        let mut scanner = Scanner::new();
        let mut c = Collector::default();

        scanner.feed(b"\x1b_Bpayload\x1b", &mut c);
        assert!(c.byo.is_empty());

        scanner.feed(b"\\", &mut c);
        assert_eq!(c.byo, vec![b"payload".to_vec()]);
    }
}
