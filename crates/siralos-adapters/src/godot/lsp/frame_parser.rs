//! Deterministic incremental LSP frame parser (JSON-RPC over
//! `Content-Length` headers).
//!
//! Handles fragmented headers, fragmented bodies, and multiple messages
//! per socket read; bounds the header block and the message body; rejects
//! malformed, missing, duplicate, and absurd `Content-Length` values.
//! Newline-delimited JSON assumptions are never made. A protocol error
//! fails the stream deterministically: the parser reports the error once
//! and ignores all subsequent input, so Siralos never mis-parses a
//! hostile stream.

use siralos_core::godot::GODOT_LIMITS;

const SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;

/// One fed-chunk outcome: a completed frame payload or a stream-fatal
/// protocol error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspFrameOutcome {
    /// A completed frame's exact JSON payload bytes.
    Frame(Vec<u8>),
    /// A deterministic protocol error; the stream is dead afterwards.
    ProtocolError(String),
}

/// Incremental frame parser state machine.
#[derive(Debug)]
pub struct LspFrameParser {
    max_header_bytes: usize,
    max_body_bytes: u64,
    pending: Vec<u8>,
    expected_body_bytes: Option<usize>,
    failed: Option<String>,
}

impl LspFrameParser {
    /// Create a parser with the immutable reference bounds.
    pub fn new() -> Self {
        Self::with_limits(
            GODOT_LIMITS.lsp_header_bytes,
            GODOT_LIMITS.lsp_message_body_bytes as u64,
        )
    }

    /// Create a parser with explicit bounds (test seam).
    pub fn with_limits(max_header_bytes: usize, max_body_bytes: u64) -> Self {
        Self {
            max_header_bytes,
            max_body_bytes,
            pending: Vec::new(),
            expected_body_bytes: None,
            failed: None,
        }
    }

    /// Feed raw bytes; returns every completed frame plus at most one
    /// error. After a protocol error every later call replays the same
    /// error and ignores the input.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<LspFrameOutcome> {
        if let Some(failed) = &self.failed {
            return vec![LspFrameOutcome::ProtocolError(failed.clone())];
        }
        let mut outcomes: Vec<LspFrameOutcome> = Vec::new();
        self.pending.extend_from_slice(chunk);
        loop {
            let consumed = self.try_consume_one(&mut outcomes);
            if let Some(failed) = &self.failed {
                outcomes.push(LspFrameOutcome::ProtocolError(failed.clone()));
                break;
            }
            if consumed == 0 {
                break;
            }
        }
        outcomes
    }

    /// The fatal protocol error message once the stream has failed.
    pub fn failed_message(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// Consume at most one frame or one state transition; returns 1 on
    /// progress and 0 when more input is required.
    fn try_consume_one(&mut self, outcomes: &mut Vec<LspFrameOutcome>) -> u32 {
        if let Some(expected) = self.expected_body_bytes {
            if self.pending.len() < expected {
                return 0;
            }
            let payload: Vec<u8> = self.pending.drain(..expected).collect();
            self.expected_body_bytes = None;
            outcomes.push(LspFrameOutcome::Frame(payload));
            return 1;
        }
        let header_end = match self
            .pending
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
        {
            Some(position) => position,
            None => {
                if self.pending.len() > self.max_header_bytes {
                    self.fail("LSP header block exceeds the bound");
                }
                return 0;
            }
        };
        if header_end > self.max_header_bytes {
            self.fail("LSP header block exceeds the bound");
            return 0;
        }
        let header_bytes: Vec<u8> =
            self.pending.drain(..header_end + 4).collect();
        let header_text = String::from_utf8_lossy(&header_bytes).into_owned();
        let content_length = match parse_content_length(&header_text) {
            Some(length) => length,
            None => {
                self.fail("missing or malformed Content-Length header");
                return 0;
            }
        };
        if content_length > self.max_body_bytes {
            self.fail(format!(
                "LSP message body exceeds the {}-byte bound",
                self.max_body_bytes
            ));
            return 0;
        }
        self.expected_body_bytes = Some(content_length as usize);
        // The body may already be fully buffered (one chunk carries
        // header+body).
        self.try_consume_one(outcomes)
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.failed = Some(message.into());
        self.pending.clear();
        self.expected_body_bytes = None;
    }
}

impl Default for LspFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_content_length(header_text: &str) -> Option<u64> {
    let mut found: Option<u64> = None;
    for raw_line in header_text.split("\r\n") {
        if raw_line.is_empty() {
            continue;
        }
        let Some(colon) = raw_line.find(':') else {
            continue;
        };
        if colon == 0 {
            continue;
        }
        let name = raw_line[..colon].trim();
        let value = raw_line[colon + 1..].trim();
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if found.is_some() {
            // Duplicate Content-Length headers are ambiguous; fail
            // deterministically.
            return None;
        }
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let parsed: u64 = value.parse().ok()?;
        if parsed > SAFE_INTEGER_MAX {
            return None;
        }
        found = Some(parsed);
    }
    found
}

/// Frame one outgoing JSON-RPC message (LSP framing).
pub fn frame_message(payload: &str) -> Vec<u8> {
    let body = payload.as_bytes();
    let mut out =
        format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{LspFrameOutcome, LspFrameParser, frame_message};

    #[test]
    fn parses_multiple_frames_per_chunk() {
        let mut parser = LspFrameParser::new();
        let mut chunk = frame_message("{\"a\":1}");
        chunk.extend_from_slice(&frame_message("{\"b\":2}"));
        let outcomes = parser.feed(&chunk);
        assert_eq!(
            outcomes,
            vec![
                LspFrameOutcome::Frame(b"{\"a\":1}".to_vec()),
                LspFrameOutcome::Frame(b"{\"b\":2}".to_vec()),
            ]
        );
    }

    #[test]
    fn handles_fragmented_headers_and_bodies() {
        let framed = frame_message("{\"jsonrpc\":\"2.0\"}");
        let split = framed.len() / 2;
        let mut parser = LspFrameParser::new();
        assert!(parser.feed(&framed[..split]).is_empty());
        let outcomes = parser.feed(&framed[split..]);
        assert_eq!(
            outcomes,
            vec![LspFrameOutcome::Frame(b"{\"jsonrpc\":\"2.0\"}".to_vec())]
        );
    }

    #[test]
    fn rejects_malformed_missing_and_duplicate_content_length() {
        for bad in [
            &b"Content-Type: text\r\n\r\n{}"[..],
            &b"Content-Length: abc\r\n\r\n{}"[..],
            &b"Content-Length: -1\r\n\r\n{}"[..],
            &b"content-length: 2\r\ncontent-length: 3\r\n\r\n{}"[..],
        ] {
            let mut parser = LspFrameParser::new();
            let outcomes = parser.feed(bad);
            assert_eq!(
                outcomes,
                vec![LspFrameOutcome::ProtocolError(
                    "missing or malformed Content-Length header".to_owned()
                )]
            );
            assert_eq!(
                parser.failed_message(),
                Some("missing or malformed Content-Length header")
            );
        }
    }

    #[test]
    fn bounds_the_message_body() {
        let mut parser = LspFrameParser::with_limits(1024, 8);
        let outcomes = parser.feed(b"Content-Length: 9\r\n\r\n123456789");
        assert_eq!(
            outcomes,
            vec![LspFrameOutcome::ProtocolError(
                "LSP message body exceeds the 8-byte bound".to_owned()
            )]
        );
    }

    #[test]
    fn bounds_an_unterminated_header_block() {
        let mut parser = LspFrameParser::with_limits(16, 1024);
        let outcomes =
            parser.feed(b"a long line of header noise without any terminator");
        assert_eq!(
            outcomes,
            vec![LspFrameOutcome::ProtocolError(
                "LSP header block exceeds the bound".to_owned()
            )]
        );
    }

    #[test]
    fn failure_is_terminal_and_ignores_subsequent_input() {
        let mut parser = LspFrameParser::with_limits(16, 1024);
        let first = parser.feed(b"garbage without any terminator at all");
        assert!(matches!(
            first.last(),
            Some(LspFrameOutcome::ProtocolError(_))
        ));
        let second = parser.feed(frame_message("{}").as_slice());
        assert_eq!(second, first);
    }

    #[test]
    fn framing_round_trips_through_the_parser() {
        let payload = "{\"method\":\"initialize\"}";
        let framed = frame_message(payload);
        let mut parser = LspFrameParser::new();
        let outcomes = parser.feed(&framed);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(
            outcomes[0],
            LspFrameOutcome::Frame(payload.as_bytes().to_vec())
        );
    }
}
