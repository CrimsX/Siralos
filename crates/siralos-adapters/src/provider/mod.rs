//! Provider adapters (Stage 3R R7.1).
//!
//! This module owns the concrete provider side of the R7.1 contract:
//! the deterministic fake provider (identity 'deterministic-fake',
//! deterministic echo, 16-code-point chunking, and the generic
//! workspace list/read/search scenarios) and the strict bounded-turn
//! collector used by planner/reviewer-style call sites. Both build on
//! the provider-neutral contracts and the shared bounded accounting
//! core in 'siralos-core::provider'.

pub mod anthropic;
pub mod credential;
pub mod deterministic_fake;
pub mod generic;
pub mod openai;
pub mod registry;
pub mod replay;
pub mod sse;
pub mod strict_turn;
pub mod tool_names;

#[cfg(test)]
mod tests;

pub use credential::HostCredential;
pub use deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
pub use registry::{
    HostProvider, ProviderKind, UnknownProvider, provider_kind_from_str,
};
pub use replay::RecordedReplayProvider;
pub use strict_turn::{
    BoundedModelToolCall, BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};

/// Hooks for determinism replay recording of provider HTTP responses.
///
/// Holds an optional clock for `observed_at_ms` and an optional recorder for
/// the response identity. The struct is `pub(crate)` and intentionally keeps
/// providers' derived `Debug` intact via a manual `Debug` impl.
#[derive(Default)]
pub(crate) struct ReplayHooks {
    /// Clock for `observed_at_ms` when recording.
    pub clock: Option<std::rc::Rc<dyn siralos_core::determinism::Clock>>,
    /// Recorder for the response identity.
    pub recorder:
        Option<std::rc::Rc<dyn siralos_core::determinism::ReplayRecorder>>,
}

impl std::fmt::Debug for ReplayHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.clock.is_some() || self.recorder.is_some() {
            f.write_str("ReplayHooks(present)")
        } else {
            f.write_str("ReplayHooks(absent)")
        }
    }
}

/// Compute the `sha256` hex of the sanitized bounded body text.
pub(crate) fn response_body_sha256(text: &str) -> String {
    siralos_core::identity::sha256_hex(text.as_bytes())
}

/// Record one provider HTTP outcome for replay.
///
/// `body_text` must be the sanitized bounded text (never the credential).
/// When a recorder is present and recording, the response identity is recorded
/// and `last_replay` is set to `Recorded { digest }`; on digest failure it is
/// set to `Unavailable { reason: "response identity digest failed" }`.
/// Otherwise `last_replay` is set to `Unavailable { reason: "live call not recorded" }`.
pub(crate) fn record_outcome(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) {
    let body_sha256 = response_body_sha256(body_text);
    let body_bytes = body_text.len() as u64;
    let observed_at_ms = hooks.clock.as_ref().map(|c| c.now_ms());
    let usage =
        siralos_core::determinism::provider_replay::parse_provider_usage(
            body_text,
        );
    if hooks.recorder.as_ref().is_some_and(|r| r.is_recording()) {
        let identity = siralos_core::determinism::ProviderResponseIdentity {
            provider_id: provider_id.to_owned(),
            model: model.to_owned(),
            status,
            body_sha256,
            body_bytes,
            observed_at_ms,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: usage.cached_tokens,
        };
        if let Some(recorder) = hooks.recorder.as_ref() {
            recorder.record_provider_response(&identity);
            recorder.record_provider_response_with_body(&identity, body_text);
        }
        let digest =
            siralos_core::determinism::compute_provider_response_identity_digest(
                &identity,
            );
        match digest {
            Ok(digest) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Recorded {
                        digest,
                    };
            }
            Err(_) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                        reason: "response identity digest failed".to_owned(),
                    };
            }
        }
    } else {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "live call not recorded".to_owned(),
            };
    }
}

/// Maximum provider response body bytes accepted before truncation.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Read an HTTP response body bounded at READ time: at most
/// `MAX_RESPONSE_BYTES + 1` bytes are buffered (via `io::Read::take`), so a
/// hostile endpoint cannot exhaust memory through an unbounded body. The
/// returned text is lossily UTF-8, stripped of control characters (newlines
/// and tabs kept), and marked `...[truncated]` when the bound was hit. The
/// `Err` payload is the raw I/O error for the caller to prefix.
pub(crate) fn bounded_body_text(
    response: reqwest::blocking::Response,
) -> Result<String, String> {
    use std::io::Read;
    let mut limited = response.take((MAX_RESPONSE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    bytes.truncate(MAX_RESPONSE_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    text.retain(|c| !c.is_control() || c == '\n' || c == '\t');
    if truncated {
        text.push_str("...[truncated]");
    }
    Ok(text)
}

/// Build the HTTP client the three model-endpoint clients share.
///
/// The openai, anthropic and generic `call_*` paths configure `reqwest`
/// identically — a 60-second request timeout and a 10-second connect timeout —
/// and differ only in the error prefix each puts on a build failure, which its
/// caller supplies. The model-listing probe in `generic` deliberately uses
/// tighter timeouts and does not call this helper.
///
/// # Errors
///
/// Returns the `reqwest` build error unchanged; callers prefix it.
pub(crate) fn build_http_client()
-> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
}

/// A one-shot loopback HTTP fixture server for the offline provider probe.
///
/// The probe's purpose is to drive the real `call_*` paths with no live
/// network, so this stands up the smallest HTTP/1.1 responder `reqwest` can
/// talk to: bind `127.0.0.1:0`, accept one connection within a deadline, read
/// the request the client actually sent, write the recorded response, close.
/// The only URL it ever hands out is `http://127.0.0.1:<port>`, and
/// [`Server::recorded`] fails loudly when no request arrived — which is exactly
/// what a probe that reached a real endpoint would look like.
///
/// This is test-only scaffolding: it is not production code and is compiled
/// only under `cfg(test)`.
///
/// LIMITS, stated so a reader does not mistake this for more than it is:
///
/// - it records what the three clients do **today** at each recorded input. It
///   is the baseline the W4.5 recorded-pair harness starts from, not an
///   approved-parity claim: passing does not bless the recorded behaviour, and
///   changing any message or event shape must be a deliberate, reviewed edit of
///   these assertions rather than a quiet update;
/// - a connect-refused probe proves **transport**-error agreement only.
///   HTTP-level agreement is what the recorded `(status, body)` fixtures cover,
///   and neither covers a success path end to end through the streaming readers;
/// - request bodies **are** observable here, because the fixture server reads
///   what the client actually sent. What is still missing is a pure
///   constructor: those bodies are built inline inside `call_*`, so a body can
///   only be compared by standing up a socket, and the generic completions path
///   hands an open response to its caller instead of parsing it in place;
/// - it speaks HTTP/1.1 and answers `Connection: close`, so it cannot record
///   HTTP/2 or TLS behaviour. That is harmless while every probe points at
///   `http://127.0.0.1`, and it must be revisited if a client ever moves to a
///   secure transport.
#[cfg(test)]
pub(crate) mod probe {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    /// How long a fixture server waits for the client before failing the test.
    const ACCEPT_DEADLINE: Duration = Duration::from_secs(5);

    /// How long a read may stall before the fixture fails the test.
    ///
    /// Without this a client that stalled mid-request would hang the test
    /// instead of failing it, which is the opposite of what a fixture that
    /// exists to make wrong behaviour loud should do.
    const READ_DEADLINE: Duration = Duration::from_secs(5);

    /// The statuses the recorded error matrix covers.
    pub(crate) const ERROR_STATUSES: [u16; 6] = [400, 401, 404, 429, 500, 503];

    /// A short JSON error body.
    pub(crate) const SHORT_ERROR_BODY: &str = r#"{"error":"boom"}"#;

    /// An HTML error body, to exercise each client's HTML handling.
    pub(crate) const HTML_ERROR_BODY: &str =
        "<html><body><h1>Gateway</h1></body></html>";

    /// A ~10 KB error body, to exercise each client's bound.
    pub(crate) fn large_error_body() -> String {
        "x".repeat(10_000)
    }

    /// A large body whose HTML marker sits past the 240-character cut point.
    ///
    /// This is the one pair where the two bounds interact: the generic path cuts
    /// at the first `<` and only then truncates to 240 characters, so the marker
    /// and everything after it must be gone.
    pub(crate) fn large_html_error_body() -> String {
        format!("{}<html>{}", "x".repeat(300), "z".repeat(10_000))
    }

    /// The labelled bodies the recorded error matrix covers.
    pub(crate) fn error_bodies() -> Vec<(&'static str, String)> {
        vec![
            ("short", SHORT_ERROR_BODY.to_owned()),
            ("10kb", large_error_body()),
            ("html", HTML_ERROR_BODY.to_owned()),
            ("10kb-html", large_html_error_body()),
        ]
    }

    /// One request, as the fixture server saw it on the socket.
    #[derive(Debug, Clone)]
    pub(crate) struct RecordedRequest {
        /// The request line, e.g. `POST /v1/chat/completions HTTP/1.1`.
        pub request_line: String,
        /// Lower-cased header names with their raw values, in arrival order.
        pub headers: Vec<(String, String)>,
        /// The request body: exactly the bytes the client sent.
        pub body: String,
    }

    impl RecordedRequest {
        /// The value of `name` (lower-cased) when the client sent it.
        pub(crate) fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
    }

    /// The response the fixture server writes back.
    pub(crate) struct Fixture {
        /// The HTTP status code to answer with.
        pub status: u16,
        /// The response body to answer with.
        pub body: String,
    }

    /// A running fixture server: its loopback base URL and the handle that
    /// yields the request it received.
    pub(crate) struct Server {
        /// The base URL to point a client at; always loopback.
        pub base_url: String,
        handle: JoinHandle<RecordedRequest>,
    }

    impl Server {
        /// Wait for the client and return what the server saw.
        ///
        /// Panics when nothing connected before the deadline: a probe that
        /// contacted anything other than this loopback listener must fail
        /// loudly rather than silently observe a real endpoint.
        pub(crate) fn recorded(self) -> RecordedRequest {
            let recorded =
                self.handle.join().expect("fixture server thread panicked");
            assert!(
                !recorded.request_line.is_empty(),
                "no request reached the loopback fixture server within \
                 {ACCEPT_DEADLINE:?}; the client under test did not contact \
                 127.0.0.1"
            );
            recorded
        }
    }

    /// Serve exactly one request with `fixture`, returning the loopback base URL.
    pub(crate) fn serve(fixture: Fixture) -> Server {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("loopback addr").port();
        let handle = std::thread::spawn(move || match accept(&listener) {
            Some(stream) => handle_connection(stream, &fixture),
            None => RecordedRequest {
                request_line: String::new(),
                headers: Vec::new(),
                body: String::new(),
            },
        });
        Server { base_url: format!("http://127.0.0.1:{port}"), handle }
    }

    /// Accept one connection, or `None` once the deadline passes.
    fn accept(listener: &TcpListener) -> Option<TcpStream> {
        listener.set_nonblocking(true).expect("nonblocking accept");
        let deadline = Instant::now() + ACCEPT_DEADLINE;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    // Windows inherits the non-blocking flag on accept.
                    stream.set_nonblocking(false).expect("blocking stream");
                    // Every read below inherits this deadline, so a client that
                    // stalls mid-request fails the test instead of hanging it.
                    stream
                        .set_read_timeout(Some(READ_DEADLINE))
                        .expect("read timeout");
                    return Some(stream);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(err) => panic!("fixture server accept failed: {err}"),
            }
        }
        None
    }

    /// Read one HTTP/1.1 request, answer it, and record what was read.
    fn handle_connection(
        mut stream: TcpStream,
        fixture: &Fixture,
    ) -> RecordedRequest {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("request line");
        let mut headers = Vec::new();
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header line");
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((name, value)) = trimmed.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_owned();
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.push((name, value));
            }
        }
        let mut bytes = vec![0u8; content_length];
        reader.read_exact(&mut bytes).expect("request body");
        let body = String::from_utf8_lossy(&bytes).into_owned();
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            fixture.status,
            reason(fixture.status),
            fixture.body.len(),
            fixture.body
        );
        stream.write_all(response.as_bytes()).expect("write response");
        stream.flush().expect("flush response");
        RecordedRequest {
            request_line: request_line.trim_end().to_owned(),
            headers,
            body,
        }
    }

    /// The reason phrase for the statuses the probe records.
    pub(crate) fn reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            404 => "Not Found",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Status",
        }
    }
}
