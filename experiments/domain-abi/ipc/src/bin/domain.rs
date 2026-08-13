//! Domain process (child) side of the IPC prototype.
//!
//! Executes semantic queries locally and *requests* any host effect
//! (workspace read, process capability) through the protocol. It holds
//! no filesystem or process authority of its own.

use domain_abi_ipc_prototype::{
    Message, PROTOCOL_NAME, PROTOCOL_VERSION, ResponseKind,
};

use std::io::{self, BufRead, Write};

fn respond(
    line_writer: &mut impl Write,
    request_id: Option<u64>,
    kind: ResponseKind,
) {
    let message =
        Message::Response { request_id: request_id.unwrap_or(0), kind };
    let _ = writeln!(line_writer, "{}", message.serialize());
    let _ = line_writer.flush();
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut package_id: Option<String> = None;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let message = match Message::parse(&line) {
            Ok(message) => message,
            Err(error) => {
                // Malformed input closes the channel with a typed error.
                let _ = writeln!(
                    stdout,
                    "{}",
                    Message::Error {
                        request_id: None,
                        code: "protocol_error".to_string(),
                        message: error,
                    }
                    .serialize()
                );
                break;
            }
        };
        match message {
            Message::Hello {
                protocol,
                version,
                package_id: id,
                package_digest: _,
            } => {
                if protocol != PROTOCOL_NAME || version != PROTOCOL_VERSION {
                    let _ = writeln!(
                        stdout,
                        "{}",
                        Message::Error {
                            request_id: None,
                            code: "protocol_mismatch".to_string(),
                            message: format!(
                                "protocol {protocol} v{version} unsupported"
                            ),
                        }
                        .serialize()
                    );
                    break;
                }
                package_id = Some(id);
                respond(
                    &mut stdout,
                    Some(0),
                    ResponseKind::Ok {
                        result: serde_json::json!({ "status": "ready" }),
                    },
                );
            }
            Message::Query { request_id, text } => {
                // Semantic work is local; workspace access is mediated.
                let result = serde_json::json!({
                    "package": package_id.as_deref().unwrap_or("<unbound>"),
                    "query": text,
                    "semantic": { "nodes": 0, "sources": text.len() },
                });
                respond(
                    &mut stdout,
                    Some(request_id),
                    ResponseKind::Ok { result },
                );
            }
            Message::WorkspaceRead { request_id, path, max_bytes } => {
                // The domain ASKS; the host performs. Here the child
                // echoes the request back for the host to fulfill.
                let result = serde_json::json!({
                    "requested_path": path,
                    "requested_max_bytes": max_bytes,
                    "mediated_by": "host",
                });
                respond(
                    &mut stdout,
                    Some(request_id),
                    ResponseKind::Ok { result },
                );
            }
            Message::CapabilityRequest { request_id, capability } => {
                respond(
                    &mut stdout,
                    Some(request_id),
                    ResponseKind::Denied {
                        reason: format!(
                            "capability {capability} not granted by host policy"
                        ),
                    },
                );
            }
            Message::Cancel { .. } | Message::Shutdown => break,
            Message::Response { .. } | Message::Error { .. } => {
                // Domains never send responses/errors; a well-behaved
                // host would flag it. Close defensively.
                break;
            }
        }
    }
}
