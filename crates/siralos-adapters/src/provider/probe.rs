//! A one-shot loopback HTTP fixture server for the offline provider probe.
//!
//! The probe's purpose is to drive the real `call_*` paths with no live
//! network, so this stands up the smallest HTTP/1.1 responder `reqwest` can
//! talk to: bind `127.0.0.1:0`, accept one connection within a deadline, read
//! the request the client actually sent, write the recorded response, close.
//! The only URL it ever hands out is `http://127.0.0.1:<port>`, and
//! [`Server::recorded`] fails loudly when no request arrived — which is exactly
//! what a probe that reached a real endpoint would look like.
//!
//! This is test-only scaffolding: it is not production code and is compiled
//! only under `cfg(test)`.
//!
//! LIMITS, stated so a reader does not mistake this for more than it is:
//!
//! - it records what the three clients do **today** at each recorded input. It
//!   is the baseline the W4.5 recorded-pair harness starts from, not an
//!   approved-parity claim: passing does not bless the recorded behaviour, and
//!   changing any message or event shape must be a deliberate, reviewed edit of
//!   these assertions rather than a quiet update;
//! - a connect-refused probe proves **transport**-error agreement only.
//!   HTTP-level agreement is what the recorded `(status, body)` fixtures cover,
//!   and neither covers a success path end to end through the streaming readers;
//! - request bodies **are** observable here, because the fixture server reads
//!   what the client actually sent. What is still missing is a pure
//!   constructor: those bodies are built inline inside `call_*`, so a body can
//!   only be compared by standing up a socket, and the generic completions path
//!   hands an open response to its caller instead of parsing it in place;
//! - it speaks HTTP/1.1 and answers `Connection: close`, so it cannot record
//!   HTTP/2 or TLS behaviour. That is harmless while every probe points at
//!   `http://127.0.0.1`, and it must be revisited if a client ever moves to a
//!   secure transport.

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
    /// A placeholder for a server that nothing contacted.
    fn absent() -> Self {
        Self {
            request_line: String::new(),
            headers: Vec::new(),
            body: String::new(),
        }
    }

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
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("loopback addr").port();
    let handle = std::thread::spawn(move || match accept(&listener) {
        Some(stream) => handle_connection(stream, &fixture),
        None => RecordedRequest::absent(),
    });
    Server { base_url: format!("http://127.0.0.1:{port}"), handle }
}

/// Serve one truncated response: `declared` bytes promised, `body` sent.
///
/// The client sees a well-formed status line and headers, a body shorter
/// than the declared length, and then a close — the shape a dropped
/// connection produces. Returns the loopback base URL.
pub(crate) fn serve_truncated(declared: usize, body: &str) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("loopback addr").port();
    let body = body.to_owned();
    let handle = std::thread::spawn(move || match accept(&listener) {
        Some(stream) => handle_truncated(stream, declared, &body),
        None => RecordedRequest::absent(),
    });
    Server { base_url: format!("http://127.0.0.1:{port}"), handle }
}

/// Hooks wired to a recorder that retains what `record_outcome` is handed.
///
/// The recorder is the crate's existing retaining one; the probe only reads
/// its snapshot afterwards, so the observation path is the production one.
pub(crate) fn retaining_hooks() -> (
    crate::provider::ReplayHooks,
    std::rc::Rc<siralos_core::determinism::RetainingReplayRecorder>,
) {
    let recorder = std::rc::Rc::new(
        siralos_core::determinism::RetainingReplayRecorder::new(),
    );
    let hooks = crate::provider::ReplayHooks {
        clock: None,
        recorder: Some(recorder.clone()),
        request_sha256: core::cell::RefCell::new(None),
        usage_observation: core::cell::RefCell::new(None),
        round_started: core::cell::Cell::new(None),
        round_millis: core::cell::Cell::new(None),
    };
    (hooks, recorder)
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

/// Read one HTTP/1.1 request, answer it in full, and record what was read.
fn handle_connection(
    mut stream: TcpStream,
    fixture: &Fixture,
) -> RecordedRequest {
    let request = read_request(&mut stream);
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
    request
}

/// Read one request, promise more than is sent, then close.
fn handle_truncated(
    mut stream: TcpStream,
    declared: usize,
    body: &str,
) -> RecordedRequest {
    let request = read_request(&mut stream);
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {declared}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(headers.as_bytes()).expect("write headers");
    stream.write_all(body.as_bytes()).expect("write partial body");
    // Deliberately short: the declared length is never satisfied, and the
    // close stands in for a dropped connection.
    stream.flush().expect("flush truncated response");
    request
}

/// Read one HTTP/1.1 request off `stream`.
fn read_request(stream: &mut TcpStream) -> RecordedRequest {
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
    RecordedRequest {
        request_line: request_line.trim_end().to_owned(),
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// The reason phrase for the statuses the probe records.
pub(crate) fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}
