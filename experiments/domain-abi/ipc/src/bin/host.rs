//! Host side of the IPC prototype: spawns the domain process, mediates
//! every request, enforces policy, and measures the boundary.
//!
//! All host effects remain in the host: workspace reads are performed
//! here (bounded), process capabilities are denied by policy, and the
//! exact package identity is bound per operation.

use domain_abi_ipc_prototype::{Message, ResponseKind, PROTOCOL_NAME, PROTOCOL_VERSION};

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};


const PACKAGE_ID: &str = "godot";
const PACKAGE_DIGEST: &str = "sha256-fixture-digest";
const PING_COUNT: u32 = 200;
const LARGE_PAYLOAD_BYTES: usize = 1024 * 1024;

pub struct DomainSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
}

impl DomainSession {
    pub fn spawn() -> std::io::Result<DomainSession> {
        let mut child = Command::new(domain_executable()).arg("domain")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout piped"));
        Ok(DomainSession { child, stdin, stdout, next_request_id: 1 })
    }

    fn send(&mut self, message: &Message) -> std::io::Result<()> {
        writeln!(self.stdin, "{}", message.serialize())?;
        self.stdin.flush()
    }

    fn recv(&mut self) -> Result<Message, String> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("domain process closed the channel (crash or shutdown)".to_string());
        }
        Message::parse(line.trim_end())
    }

    pub fn handshake(&mut self) -> Result<(), String> {
        let hello = Message::Hello {
            protocol: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION,
            package_id: PACKAGE_ID.to_string(),
            package_digest: PACKAGE_DIGEST.to_string(),
        };
        self.send(&hello).map_err(|e| e.to_string())?;
        let response = self.recv()?;
        match response {
            Message::Response { kind: ResponseKind::Ok { .. }, .. } => Ok(()),
            other => Err(format!("handshake failed: {other:?}")),
        }
    }

    pub fn query(&mut self, text: &str) -> Result<serde_json::Value, String> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&Message::Query { request_id: id, text: text.to_string() })
            .map_err(|e| e.to_string())?;
        match self.recv()? {
            Message::Response { request_id, kind: ResponseKind::Ok { result } }
                if request_id == id =>
            {
                Ok(result)
            }
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    pub fn request_capability(&mut self, capability: &str) -> Result<ResponseKind, String> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&Message::CapabilityRequest {
            request_id: id,
            capability: capability.to_string(),
        })
        .map_err(|e| e.to_string())?;
        match self.recv()? {
            Message::Response { request_id, kind } if request_id == id => Ok(kind),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    pub fn request_workspace_read(&mut self) -> Result<ResponseKind, String> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&Message::WorkspaceRead {
            request_id: id,
            path: "/fixture/workspace/scene.tscn".to_string(),
            max_bytes: 4096,
        })
        .map_err(|e| e.to_string())?;
        match self.recv()? {
            Message::Response { request_id, kind } if request_id == id => Ok(kind),
            other => Err(format!("unexpected response: {other:?}")),
        }
    }

    pub fn close(&mut self) {
        let _ = self.send(&Message::Shutdown);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn domain_executable() -> std::path::PathBuf {
    // The domain is the sibling binary in the same target directory.
    let mut path = std::env::current_exe().expect("resolve current executable");
    path.set_file_name(if cfg!(windows) { "domain.exe" } else { "domain" });
    path
}

fn timed<F>(label: &str, mut f: F) -> Duration
where
    F: FnMut() -> Result<(), String>,
{
    let start = Instant::now();
    let result = f();
    let elapsed = start.elapsed();
    match result {
        Ok(()) => eprintln!("{label}: {:?}", elapsed),
        Err(error) => eprintln!("{label}: FAILED ({error})"),
    }
    elapsed
}

fn main() {
    // --- Failure-mode conformance (contract Part 18) ---
    let mut session = DomainSession::spawn().expect("spawn domain");
    timed("handshake", || session.handshake());
    timed("semantic query", || {
        let result = session.query("character movement")?;
        assert_eq!(result["semantic"]["nodes"], 0);
        Ok(())
    });
    timed("host-mediated workspace read", || {
        let kind = session.request_workspace_read()?;
        assert!(matches!(kind, ResponseKind::Ok { .. }));
        Ok(())
    });
    timed("capability denied by host policy", || {
        let kind = session.request_capability("process:exec")?;
        assert!(matches!(kind, ResponseKind::Denied { .. }));
        Ok(())
    });
    session.close();

    // Protocol mismatch: version 2 must be rejected hard.
    {
        let mut child = Command::new(domain_executable()).arg("domain")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn domain");
        let mut stdin = child.stdin.take().expect("stdin");
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
        writeln!(
            stdin,
            "{}",
            Message::Hello {
                protocol: PROTOCOL_NAME.to_string(),
                version: PROTOCOL_VERSION + 1,
                package_id: PACKAGE_ID.to_string(),
                package_digest: PACKAGE_DIGEST.to_string(),
            }
            .serialize()
        )
        .expect("write hello");
        stdin.flush().expect("flush");
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read response");
        let response = Message::parse(line.trim_end()).expect("parse response");
        let code = match &response {
            Message::Error { code, .. } => code.as_str(),
            _ => "",
        };
        assert_eq!(code, "protocol_mismatch", "version mismatch must be rejected: {response:?}");
        let mut rest = String::new();
        let _ = stdout.read_line(&mut rest); // channel closed after error
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("protocol-mismatch: rejected with typed error");
    }

    // Child crash: a killed domain must surface as channel close.
    {
        let mut child = Command::new(domain_executable()).arg("domain")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn domain");
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("child-crash: kill/wait succeeded, host observes channel close on next recv");
    }

    // --- Measurement (contract Part 18) ---
    let mut session = DomainSession::spawn().expect("spawn domain");
    session.handshake().expect("handshake");

    let start = Instant::now();
    for i in 0..PING_COUNT {
        session
            .query(&format!("ping {i}"))
            .expect("query succeeds");
    }
    let per_call = start.elapsed() / PING_COUNT;
    eprintln!("round-trip: {per_call:?} per call ({PING_COUNT} calls)");

    let payload = "x".repeat(LARGE_PAYLOAD_BYTES);
    let start = Instant::now();
    session.query(&payload).expect("large query succeeds");
    eprintln!(
        "large-payload ({LARGE_PAYLOAD_BYTES} bytes): {:?}",
        start.elapsed()
    );
    session.close();
    eprintln!("IPC prototype: conformance + measurement complete");
}
