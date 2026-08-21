//! Bounded JSON-RPC 2.0 payload classification for the LSP channel.
//!
//! Frame payloads are parsed and classified into server-initiated
//! requests, notifications, and responses; malformed payloads report the
//! reference protocol-error messages without ever crashing Siralos. The
//! connection machinery (pending-request correlation, timeouts,
//! cancellation) lands with the milestone that can actually start an LSP
//! session.

use serde_json::Value;

/// JSON-RPC 2.0 error codes used by the LSP adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonRpcCode(pub i64);

impl JsonRpcCode {
    /// Parse error.
    pub const PARSE_ERROR: Self = Self(-32700);
    /// Invalid request.
    pub const INVALID_REQUEST: Self = Self(-32600);
    /// Method not found.
    pub const METHOD_NOT_FOUND: Self = Self(-32601);
    /// Invalid params.
    pub const INVALID_PARAMS: Self = Self(-32602);
    /// Internal error.
    pub const INTERNAL_ERROR: Self = Self(-32603);
    /// Request cancelled.
    pub const REQUEST_CANCELLED: Self = Self(-32800);
    /// Content modified.
    pub const CONTENT_MODIFIED: Self = Self(-32801);

    /// The numeric code.
    #[must_use]
    pub const fn code(self) -> i64 {
        self.0
    }
}

/// One classified incoming JSON-RPC message.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonRpcMessage {
    /// A server-initiated request carrying any JSON id.
    ServerRequest {
        /// Request id.
        id: Value,
        /// Method name.
        method: String,
        /// Params, when present.
        params: Option<Value>,
    },
    /// A notification; no response is expected.
    Notification {
        /// Method name.
        method: String,
        /// Params, when present.
        params: Option<Value>,
    },
    /// A response correlated by id.
    Response {
        /// Numeric or string id.
        id: Value,
    },
}

/// Classify one frame payload; `Err` carries the exact protocol-error
/// message the reference reports for that shape.
pub fn classify_json_rpc_payload(
    payload: &[u8],
) -> Result<JsonRpcMessage, String> {
    let parsed: Value = match serde_json::from_slice(payload) {
        Ok(value) => value,
        Err(_) => return Err("The LSP peer sent invalid JSON.".to_owned()),
    };
    let Some(record) = parsed.as_object() else {
        return Err(
            "The LSP peer sent a non-object JSON-RPC message.".to_owned()
        );
    };
    let method = record.get("method").and_then(Value::as_str);
    let id_present = record.contains_key("id");
    if let Some(method) = method {
        let id = record.get("id").cloned();
        if let Some(id) = id {
            return Ok(JsonRpcMessage::ServerRequest {
                id,
                method: method.to_owned(),
                params: record.get("params").cloned(),
            });
        }
        return Ok(JsonRpcMessage::Notification {
            method: method.to_owned(),
            params: record.get("params").cloned(),
        });
    }
    if id_present {
        let id = record.get("id").cloned().unwrap_or(Value::Null);
        if !id.is_number() && !id.is_string() {
            return Err(
                "The LSP peer sent a response with an invalid id.".to_owned()
            );
        }
        return Ok(JsonRpcMessage::Response { id });
    }
    Err("The LSP peer sent an unidentifiable JSON-RPC message.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{JsonRpcCode, classify_json_rpc_payload};
    use serde_json::json;

    #[test]
    fn codes_match_the_reference_table() {
        assert_eq!(JsonRpcCode::PARSE_ERROR.code(), -32700);
        assert_eq!(JsonRpcCode::INVALID_REQUEST.code(), -32600);
        assert_eq!(JsonRpcCode::METHOD_NOT_FOUND.code(), -32601);
        assert_eq!(JsonRpcCode::INVALID_PARAMS.code(), -32602);
        assert_eq!(JsonRpcCode::INTERNAL_ERROR.code(), -32603);
        assert_eq!(JsonRpcCode::REQUEST_CANCELLED.code(), -32800);
        assert_eq!(JsonRpcCode::CONTENT_MODIFIED.code(), -32801);
    }

    #[test]
    fn classifies_requests_notifications_and_responses() {
        assert_eq!(
            classify_json_rpc_payload(
                br#"{"jsonrpc":"2.0","id":7,"method":"workspace/applyEdit"}"#
            ),
            Ok(super::JsonRpcMessage::ServerRequest {
                id: json!(7),
                method: "workspace/applyEdit".to_owned(),
                params: None,
            })
        );
        assert_eq!(
            classify_json_rpc_payload(
                br#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"res://x.gd"}}"#
            ),
            Ok(super::JsonRpcMessage::Notification {
                method: "textDocument/publishDiagnostics".to_owned(),
                params: Some(json!({"uri": "res://x.gd"})),
            })
        );
        assert_eq!(
            classify_json_rpc_payload(br#"{"id":"abc","result":null}"#),
            Ok(super::JsonRpcMessage::Response { id: json!("abc") })
        );
    }

    #[test]
    fn protocol_errors_use_reference_messages() {
        assert_eq!(
            classify_json_rpc_payload(b"not json"),
            Err("The LSP peer sent invalid JSON.".to_owned())
        );
        assert_eq!(
            classify_json_rpc_payload(b"[1,2]"),
            Err("The LSP peer sent a non-object JSON-RPC message.".to_owned())
        );
        assert_eq!(
            classify_json_rpc_payload(br#"{"result":1}"#),
            Err("The LSP peer sent an unidentifiable JSON-RPC message."
                .to_owned())
        );
        assert_eq!(
            classify_json_rpc_payload(br#"{"id":null,"result":1}"#),
            Err("The LSP peer sent a response with an invalid id.".to_owned())
        );
    }
}
