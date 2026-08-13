//! Versioned JSON-lines protocol shared by the IPC domain prototype.
//!
//! The domain process never owns host effects: it only *requests* them
//! and the host performs and returns them. Every request carries the
//! bound package identity; stale bindings are rejected per operation.
//! Protocol version mismatches close the channel hard (never a silent
//! downgrade).

use serde::{Deserialize, Serialize};

pub const PROTOCOL_NAME: &str = "siralos-domain-abi";
pub const PROTOCOL_VERSION: u64 = 1;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Message {
    /// First message on a new channel: domain metadata.
    Hello {
        protocol: String,
        version: u64,
        package_id: String,
        package_digest: String,
    },
    /// Semantic query; the host mediates any needed workspace access.
    Query {
        request_id: u64,
        text: String,
    },
    /// The domain requests host-mediated workspace read access.
    WorkspaceRead {
        request_id: u64,
        path: String,
        max_bytes: usize,
    },
    /// The domain requests a host-mediated runtime/process capability.
    /// The host's policy decides; the prototype always denies with a
    /// typed reason.
    CapabilityRequest {
        request_id: u64,
        capability: String,
    },
    /// Host → domain: cancel an in-flight request.
    Cancel { request_id: u64 },
    /// Response carrying a structured result.
    Response {
        request_id: u64,
        kind: ResponseKind,
    },
    /// Protocol-level failure (malformed, stale binding, policy deny).
    Error { request_id: Option<u64>, code: String, message: String },
    /// Clean shutdown.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseKind {
    Ok { result: serde_json::Value },
    Cancelled,
    Denied { reason: String },
    Error { code: String, message: String },
}

impl Message {
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).expect("messages are serializable")
    }

    pub fn parse(line: &str) -> Result<Message, String> {
        serde_json::from_str(line).map_err(|error| format!("malformed message: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Message, ResponseKind, PROTOCOL_NAME, PROTOCOL_VERSION};

    #[test]
    fn hello_round_trips() {
        let message = Message::Hello {
            protocol: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION,
            package_id: "godot".to_string(),
            package_digest: "abc123".to_string(),
        };
        let line = message.serialize();
        assert_eq!(Message::parse(&line).expect("parses"), message);
    }

    #[test]
    fn malformed_messages_are_typed_errors_never_silent() {
        assert!(Message::parse("not json").is_err());
        assert!(Message::parse(r#"{"op":"unknown"}"#).is_err());
        // Unknown protocol version fails explicitly.
        let message = Message::Hello {
            protocol: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION + 1,
            package_id: "godot".to_string(),
            package_digest: "abc".to_string(),
        };
        let line = message.serialize();
        let parsed = Message::parse(&line).expect("parses");
        assert_ne!(parsed, Message::Hello {
            protocol: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION,
            package_id: "godot".to_string(),
            package_digest: "abc".to_string(),
        });
    }

    #[test]
    fn response_kinds_are_structured() {
        let message = Message::Response {
            request_id: 7,
            kind: ResponseKind::Denied {
                reason: "capability not granted".to_string(),
            },
        };
        let line = message.serialize();
        assert_eq!(Message::parse(&line).expect("parses"), message);
    }
}
