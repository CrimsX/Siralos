//! Provider adapters (Stage 3R R7.1).
//!
//! This module owns the concrete provider side of the R7.1 contract:
//! the deterministic fake provider (identity 'deterministic-fake',
//! deterministic echo, 16-code-point chunking, and the generic
//! workspace list/read/search scenarios) and the strict bounded-turn
//! collector used by planner/reviewer-style call sites. Both build on
//! the provider-neutral contracts and the shared bounded accounting
//! core in 'siralos-core::provider'.

use siralos_core::provider::{ModelEvent, ProviderEvent, ToolCallInput};

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
mod streaming_redactor_tests {
    use super::{
        MAX_RESPONSE_BYTES, StreamingSecretRedactor,
        bounded_body_text_from_bytes,
    };
    use siralos_core::provider::{ModelEvent, ProviderEvent};

    fn text_delta_text(event: ProviderEvent) -> String {
        match event {
            ProviderEvent::Event(ModelEvent::TextDelta { text }) => text,
            _ => panic!("expected a text delta"),
        }
    }

    fn reasoning_delta_text(event: ProviderEvent) -> String {
        match event {
            ProviderEvent::Event(ModelEvent::ReasoningDelta { text }) => text,
            _ => panic!("expected a reasoning delta"),
        }
    }

    fn assert_split_secret_redacted(secret: &str, first: &str, second: &str) {
        let mut redactor = StreamingSecretRedactor::new(Some(secret));
        let first = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: first.to_owned() },
        ));
        let second = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: second.to_owned() },
        ));
        assert_eq!(text_delta_text(first), "");
        assert_eq!(text_delta_text(second), "[REDACTED]");
        assert_eq!(redactor.finish(), None);
    }

    #[test]
    fn masks_a_credential_split_across_short_stream_prefixes() {
        let mut redactor = StreamingSecretRedactor::new(Some("split-secret"));
        let first = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "s".to_owned() },
        ));
        let second = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::ReasoningDelta { text: "pl".to_owned() },
        ));
        let third = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "it-secret".to_owned() },
        ));
        let tail = redactor.finish();
        assert_eq!(text_delta_text(first), "");
        assert_eq!(reasoning_delta_text(second), "");
        assert_eq!(text_delta_text(third), "[REDACTED]");
        assert_eq!(tail, None);
    }

    #[test]
    fn streaming_redaction_cost_is_paid_once_not_per_chunk() {
        // A 4 KiB control-heavy `key:` value is a credential shape the config
        // boundary accepts, and it yields hundreds of encodings. The matcher is
        // compiled once in `new`, so streaming many chunks must not multiply
        // the construction cost by the chunk count. The ceiling is a DEBUG
        // build allowance: with per-call construction the same 16 chunks take
        // tens of seconds, with the compiled matcher a few.
        let secret: String = (0..4096u32)
            .map(|index| char::from_u32(0x01 + (index % 30)).expect("char"))
            .collect();
        let chunk = "a".repeat(8 * 1024);
        let started = std::time::Instant::now();
        let mut redactor = StreamingSecretRedactor::new(Some(&secret));
        for _ in 0..16 {
            let _ = redactor.redact_event(ProviderEvent::Event(
                ModelEvent::TextDelta { text: chunk.clone() },
            ));
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(20),
            "streaming redaction took {elapsed:?} for 16 chunks of a 4 KiB credential"
        );
    }

    #[test]
    fn body_redaction_is_exact_so_replayable_bodies_survive() {
        let secret = "secret-token";
        // A complete body with no credential anywhere must survive byte-exact:
        // prefix holding belongs to the streaming boundary, not here.
        let body = r#"{"choices":[{"message":{"content":"ordinary s"}}]}"#;
        let safe = super::redact_body_with_secret(body, Some(secret));
        assert_eq!(safe, body, "{safe}");

        // The same structural pass still removes a nested literal credential.
        let leaking = r#"{"a":{"b":"prefix secret-token suffix"}}"#;
        let safe = super::redact_body_with_secret(leaking, Some(secret));
        assert!(!safe.contains("secret-token"), "{safe}");
        assert!(safe.contains("[REDACTED]"), "{safe}");
    }

    #[test]
    fn redacts_json_escaped_literal_credentials() {
        let secret = "a\"b\\c\n\t";
        let body = serde_json::json!({ "text": secret }).to_string();
        let redacted = super::redact_sensitive(&body, Some(secret));
        assert!(!redacted.contains(secret), "{redacted}");
        assert!(!redacted.contains("a\\\"b\\\\c\\n\\t"), "{redacted}");
        assert!(redacted.contains("[REDACTED]"), "{redacted}");
    }

    #[test]
    fn redacts_arbitrary_length_selective_json_and_percent_forms() {
        let secret = "abcdefghij/é";
        let json = r#"ab\u0063defghij\/é"#;
        let percent = "%61%62%63%64%65%66%67%68%69%6A%2F%C3%A9";
        for body in [json, percent] {
            let redacted = super::redact_sensitive(body, Some(secret));
            assert!(!redacted.contains(secret), "{redacted}");
            assert!(!redacted.contains("\\u0063"), "{redacted}");
            assert!(redacted.contains("[REDACTED]"), "{redacted}");
        }
    }

    #[test]
    fn encoded_matchers_preserve_unmatched_prefixes() {
        let secret = "test-cred";
        let json = super::redact_sensitive("anthropic request", Some(secret));
        assert_eq!(json, "anthropic request");
        let percent =
            super::redact_sensitive("request %20 body", Some(secret));
        assert_eq!(percent, "request %20 body");
    }

    #[test]
    fn encoded_matchers_find_overlapping_prefix_matches() {
        let secret = "aab";
        let json = super::redact_sensitive(r"aaab", Some(secret));
        assert!(json.ends_with("[REDACTED]"));
        let percent = super::redact_sensitive("%61%61%61%62", Some(secret));
        assert!(percent.ends_with("[REDACTED]"));
    }

    #[test]
    fn variant_matcher_redacts_offset_overlapping_matches() {
        let variants = vec!["abcab".to_owned()];
        assert_eq!(
            super::replace_variant_matches(
                "abcabcab",
                &variants,
                "[REDACTED]",
            ),
            "[REDACTED][REDACTED]"
        );
    }

    #[test]
    fn selective_json_does_not_reflect_a_long_secret_or_encoded_body() {
        let secret = "D]D]abcde";
        let body = r"\u0044]D]\u0061bcdeD]\u0061bcde";
        let redacted = super::redact_sensitive(body, Some(secret));
        assert!(!redacted.contains(secret), "{redacted}");
        assert!(!redacted.contains(body), "{redacted}");
        assert_eq!(redacted, r"[REDACTED:1]D]\u0061bcde");
    }

    #[test]
    fn selective_json_redacts_border_overlapping_matches() {
        let secret = "D]abcdeD]";
        let body = r"\u0044]abcdeD]\u0061bcdeD]";
        let redacted = super::redact_sensitive(body, Some(secret));
        assert!(!redacted.contains(secret), "{redacted}");
        assert!(!redacted.contains(body), "{redacted}");
        assert_eq!(redacted, "[REDACTED:1][REDACTED:1]");
    }

    #[test]
    fn redacts_non_ascii_credentials_without_panicking_on_unrelated_text() {
        let secret = "pässwördé";
        let body = format!("before {secret} after");
        let redacted = super::redact_sensitive(&body, Some(secret));
        assert_eq!(redacted, "before [REDACTED] after");
        assert_eq!(
            super::redact_sensitive("ordinary text", Some(secret)),
            "ordinary text"
        );
    }

    #[test]
    fn redacts_long_encoded_credentials_in_large_bodies_without_truncation() {
        let secret = "a".repeat(4096);
        let mut body = "prefix-".to_owned();
        body.push_str(&"%61".repeat(4096));
        body.push_str("-suffix");
        let redacted = super::redact_sensitive(&body, Some(&secret));
        assert!(!redacted.contains(&secret));
        assert!(redacted.contains("[REDACTED]"));
        assert!(redacted.ends_with("-suffix"));
        assert!(redacted.starts_with("prefix-"));
    }

    #[test]
    fn redacts_mixed_percent_space_and_utf8_encodings() {
        assert_eq!(
            super::redact_sensitive("a+b%20cdefgh", Some("a b cdefgh")),
            "[REDACTED]"
        );
        assert_eq!(
            super::redact_sensitive("a%2Bb%20cdefgh", Some("a+b cdefgh"),),
            "[REDACTED]"
        );

        let ambiguous_secret = " +a";
        assert_eq!(
            super::redact_sensitive("++%2Ba", Some(ambiguous_secret)),
            "+[REDACTED]"
        );
        let mut redactor =
            StreamingSecretRedactor::new(Some(ambiguous_secret));
        let first = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "++%2B".to_owned() },
        ));
        let second = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::ReasoningDelta { text: "a".to_owned() },
        ));
        assert_eq!(text_delta_text(first), "+");
        assert_eq!(reasoning_delta_text(second), "[REDACTED]");
        assert_eq!(redactor.finish(), None);

        assert_eq!(
            super::redact_sensitive("+ab%2Bcdefgh", Some(" ab+cdefgh"),),
            "[REDACTED]"
        );
        assert_eq!(
            super::redact_sensitive("%2B++a", Some("+  a")),
            "[REDACTED]"
        );

        let mut partial_redactor = StreamingSecretRedactor::new(Some("a+b"));
        let first = partial_redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "a%2".to_owned() },
        ));
        let second = partial_redactor.redact_event(ProviderEvent::Event(
            ModelEvent::ReasoningDelta { text: "Bb".to_owned() },
        ));
        assert_eq!(text_delta_text(first), "");
        assert_eq!(reasoning_delta_text(second), "[REDACTED]");
        assert_eq!(partial_redactor.finish(), None);

        let secret = "é".repeat(5);
        let encoded_prefix = "%C3%A9".repeat(4);
        let mut redactor = StreamingSecretRedactor::new(Some(&secret));
        let first = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: encoded_prefix },
        ));
        let second = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "é".to_owned() },
        ));
        assert_eq!(text_delta_text(first), "");
        assert_eq!(text_delta_text(second), "[REDACTED]");
        assert_eq!(redactor.finish(), None);
    }

    #[test]
    fn holds_raw_prefixes_before_encoded_credentials_across_chunks() {
        for encoded in [r"\u0063defghij", "%63defghij"] {
            assert_split_secret_redacted("abcdefghij", "ab", encoded);
        }
    }

    #[test]
    fn holds_selective_encoded_prefixes_across_stream_chunks() {
        for (first, second) in [(r"ab\u0063", "defghij"), ("ab%63", "defghij")]
        {
            assert_split_secret_redacted("abcdefghij", first, second);
        }
    }

    #[test]
    fn holds_incomplete_encoded_prefixes_across_stream_chunks() {
        for (first, second) in
            [(r"ab\u00", r"63defghij"), ("ab%6", "3defghij")]
        {
            assert_split_secret_redacted("abcdefghij", first, second);
        }
    }

    #[test]
    fn finish_redacts_ambiguous_raw_and_encoded_eof_prefixes() {
        for suffix in ["ab", r"ab\u0063", "ab%63", r"ab\u00", "ab%6"] {
            let mut redactor =
                StreamingSecretRedactor::new(Some("abcdefghij"));
            let event = redactor.redact_event(ProviderEvent::Event(
                ModelEvent::TextDelta { text: format!("ordinary {suffix}") },
            ));
            assert_eq!(text_delta_text(event), "ordinary ");
            assert_eq!(redactor.finish(), Some("[REDACTED]".to_owned()));
            assert_eq!(redactor.finish(), None);
        }
    }

    #[test]
    fn finish_uses_a_collision_safe_marker_for_a_held_prefix() {
        let mut redactor = StreamingSecretRedactor::new(Some("[REDACTED]"));
        let event = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: "ordinary [".to_owned() },
        ));
        assert_eq!(text_delta_text(event), "ordinary ");
        assert_eq!(redactor.finish(), Some("[REDACTED:1]".to_owned()));
    }

    #[test]
    fn finish_does_not_split_a_marker_at_a_held_raw_prefix() {
        let secret = "]x";
        let mut redactor = StreamingSecretRedactor::new(Some(secret));
        let event = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text: secret.to_owned() },
        ));
        let emitted = text_delta_text(event);
        let finish = redactor.finish();
        let output =
            format!("{emitted}{}", finish.as_deref().unwrap_or_default());
        assert!(!output.contains(secret), "{output}");
        assert!(!output.contains("[REDACTED["), "{output}");
        assert_ne!(output, "]");
        assert_ne!(output, "[REDACTED");
        assert_eq!(finish, None);
    }

    #[test]
    fn releases_a_non_prefix_ordinary_eof_suffix() {
        for (secret, text) in [
            ("test-cred", "first-block-z"),
            ("aabx", "aabaz"),
            ("\u{1F600}a", "\u{1F600}x"),
        ] {
            let mut redactor = StreamingSecretRedactor::new(Some(secret));
            let event = redactor.redact_event(ProviderEvent::Event(
                ModelEvent::TextDelta { text: text.to_owned() },
            ));
            assert_eq!(text_delta_text(event), text);
            assert_eq!(redactor.finish(), None);
        }
    }

    #[test]
    fn preserves_non_prefix_text_just_over_the_former_limit() {
        const FORMER_LIMIT_PLUS_ONE: usize = 8 * 1024 * 1024 + 1;
        let ordinary = "z".repeat(FORMER_LIMIT_PLUS_ONE);
        let redacted = super::redact_sensitive(&ordinary, Some("test-cred"));
        assert_eq!(redacted, ordinary);
    }

    #[test]
    fn replaces_only_an_oversized_held_suffix() {
        let held_len = 64 * 1024 + 1;
        let secret = "a".repeat(held_len + 1);
        let text = format!("ordinary {}", &secret[..held_len]);
        let mut redactor = StreamingSecretRedactor::new(Some(&secret));
        let event = redactor.redact_event(ProviderEvent::Event(
            ModelEvent::TextDelta { text },
        ));
        let text = text_delta_text(event);
        assert_eq!(text.len(), "ordinary [REDACTED]".len());
        assert!(text.starts_with("ordinary "));
        assert!(text.ends_with("[REDACTED]"));
        assert_eq!(redactor.finish(), None);
    }

    #[test]
    fn bounded_body_discards_cross_cap_credential_fragments() {
        let cases = [
            ("raw", "abc", "abc", 2, 17),
            ("JSON", "abc", r"\u0061\u0062\u0063", 17, 17),
            ("percent", "\u{0800}\u{0800}a", "%E0%A0%80%E0%A0%80%61", 20, 20),
        ];
        for (label, secret, encoded, pre_len, overlap) in cases {
            let prefix_len = MAX_RESPONSE_BYTES - pre_len;
            let mut bytes = vec![b'x'; prefix_len];
            bytes.extend_from_slice(encoded.as_bytes());
            bytes.extend_from_slice(b"tail");

            let text = bounded_body_text_from_bytes(bytes, Some(secret));
            let body_len = MAX_RESPONSE_BYTES - overlap;
            assert_eq!(text.len(), body_len + "...[truncated]".len());
            assert!(
                text[..body_len].bytes().all(|byte| byte == b'x'),
                "{label} retained a pre-cap credential fragment"
            );
            assert!(
                text.ends_with("...[truncated]"),
                "{label} omitted the truncation marker"
            );
        }
    }

    #[test]
    fn bounded_body_redacts_truncation_disclosure_collisions() {
        for secret in ["...[truncated]", "truncated]"] {
            let bytes = vec![b'x'; MAX_RESPONSE_BYTES + 1];
            let text = bounded_body_text_from_bytes(bytes, Some(secret));
            assert!(!text.contains(secret), "truncation disclosed secret");
            assert!(text.ends_with("[REDACTED]"));
        }
    }
}

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
    /// Canonical digest of the request currently being served, if any.
    pub request_sha256: core::cell::RefCell<Option<String>>,
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

/// Compute a stable, content-derived digest of a provider request. The raw
/// request is never retained; only this digest is attached to a recording.
pub(crate) fn request_sha256(
    request: &siralos_core::provider::ModelRequest,
) -> String {
    use siralos_core::provider::conversation::ConversationItem;
    use siralos_core::provider::result::ToolExecutionResult;

    let messages = request
        .messages
        .iter()
        .map(|item| match item {
            ConversationItem::UserMessage { content } => {
                serde_json::json!({"kind": "user", "content": content})
            }
            ConversationItem::AssistantMessage { content } => {
                serde_json::json!({"kind": "assistant", "content": content})
            }
            ConversationItem::AssistantToolCall {
                call_id,
                tool_name,
                input,
            } => {
                let input = match input {
                    siralos_core::provider::conversation::AssistantToolCallInput::Present(value) => {
                        serde_json::json!({"present": true, "value": value})
                    }
                    siralos_core::provider::conversation::AssistantToolCallInput::Omitted => {
                        serde_json::json!({"present": false})
                    }
                };
                serde_json::json!({
                    "kind": "assistant_tool_call",
                    "call_id": call_id,
                    "tool_name": tool_name,
                    "input": input,
                })
            }
            ConversationItem::ToolResult { call_id, tool_name, result } => {
                let result = match result {
                    ToolExecutionResult::Success { output, summary } => {
                        serde_json::json!({
                            "status": "success",
                            "output": output,
                            "summary": summary,
                        })
                    }
                    other => serde_json::json!({
                        "status": other.status_str(),
                        "message": other.message(),
                    }),
                };
                serde_json::json!({
                    "kind": "tool_result",
                    "call_id": call_id,
                    "tool_name": tool_name,
                    "result": result,
                })
            }
        })
        .collect::<Vec<_>>();
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            })
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::json!({
        "messages": messages,
        "tools": tools,
        "system": request.system,
    });
    let bytes = serde_json::to_vec(&canonical)
        .expect("serde_json::Value is always serializable");
    siralos_core::identity::sha256_hex(&bytes)
}

/// Bind a request digest to the effective provider route. The route is
/// deliberately credential-free; model remains a live label and is not part
/// of playback identity.
pub(crate) fn request_sha256_for_route(
    request: &siralos_core::provider::ModelRequest,
    provider_id: &str,
    endpoint: &str,
    protocol: &str,
) -> String {
    let canonical = serde_json::json!({
        "request": request_sha256(request),
        "provider": provider_id,
        "endpoint": endpoint,
        "protocol": protocol,
    });
    let bytes = serde_json::to_vec(&canonical)
        .expect("serde_json::Value is always serializable");
    siralos_core::identity::sha256_hex(&bytes)
}

fn sensitive_variants(secret: &str) -> Vec<String> {
    let mut variants = vec![secret.to_owned()];
    if let Some(escaped) =
        serde_json::to_string(secret).ok().and_then(|quoted| {
            quoted
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_owned)
        })
    {
        variants.push(escaped);
    }
    // JSON permits either short escapes or UTF-16 `\uXXXX` escapes. Include
    // both cases, plus the common mixed form where a control/quote/backslash
    // is escaped while the remaining credential bytes stay literal.
    for upper in [false, true] {
        let mut full = String::new();
        for unit in secret.encode_utf16() {
            let mut digits = format!("{unit:04X}");
            if !upper {
                digits.make_ascii_lowercase();
            }
            full.push_str("\\u");
            full.push_str(&digits);
        }
        variants.push(full);
    }
    // One mixed form per escapable character is linear in the credential, but a
    // control-heavy credential is attacker-sized. Bound the aggregate variant
    // bytes and count so building the matcher stays O(1) in the input length;
    // the exact full-escaped and raw forms above are always retained.
    const MAX_VARIANT_TOTAL_BYTES: usize = 1024 * 1024;
    const MAX_VARIANT_COUNT: usize = 512;
    let mut variant_bytes = variants.iter().map(String::len).sum::<usize>();
    for (index, ch) in secret.char_indices() {
        if variants.len() >= MAX_VARIANT_COUNT
            || variant_bytes >= MAX_VARIANT_TOTAL_BYTES
        {
            break;
        }
        let replacement = match ch {
            '"' => "\\\"".to_owned(),
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            '\u{08}' => "\\b".to_owned(),
            '\u{0c}' => "\\f".to_owned(),
            _ if ch.is_control() => {
                let mut units = [0u16; 2];
                let encoded = ch.encode_utf16(&mut units);
                if encoded.len() == 1 {
                    format!("\\u{:04x}", units[0])
                } else {
                    format!("\\u{:04x}\\u{:04x}", units[0], units[1])
                }
            }
            _ => continue,
        };
        let mut mixed =
            String::with_capacity(secret.len() + replacement.len());
        mixed.push_str(&secret[..index]);
        mixed.push_str(&replacement);
        mixed.push_str(&secret[index + ch.len_utf8()..]);
        variant_bytes = variant_bytes.saturating_add(mixed.len());
        variants.push(mixed);
    }
    // Mixed JSON escape combinations (up to eight escapable characters)
    // cover credentials containing several quoted/control characters without
    // allowing an attacker-controlled long secret to create an exponential
    // variant set. Longer secrets retain the raw, full-escaped, and
    // single-character forms above.
    let escapable: Vec<(usize, char, String)> = secret
        .char_indices()
        .filter_map(|(index, ch)| {
            let replacement = match ch {
                '"' => Some("\\\"".to_owned()),
                '\\' => Some("\\\\".to_owned()),
                '\n' => Some("\\n".to_owned()),
                '\r' => Some("\\r".to_owned()),
                '\t' => Some("\\t".to_owned()),
                '\u{08}' => Some("\\b".to_owned()),
                '\u{0c}' => Some("\\f".to_owned()),
                _ if ch.is_control() => {
                    let mut units = [0u16; 2];
                    let encoded = ch.encode_utf16(&mut units);
                    if encoded.len() == 1 {
                        Some(format!("\\u{:04x}", units[0]))
                    } else {
                        Some(format!("\\u{:04x}\\u{:04x}", units[0], units[1]))
                    }
                }
                _ => None,
            };
            replacement.map(|replacement| (index, ch, replacement))
        })
        .collect();
    if escapable.len() <= 8 {
        for mask in 0u16..(1u16 << escapable.len()) {
            let mut mixed = String::with_capacity(secret.len() + 16);
            let mut cursor = 0usize;
            for (bit, (index, ch, replacement)) in escapable.iter().enumerate()
            {
                let end = *index;
                mixed.push_str(&secret[cursor..end]);
                if mask & (1u16 << bit) != 0 {
                    mixed.push_str(replacement);
                } else {
                    mixed.push(*ch);
                }
                cursor = end + ch.len_utf8();
            }
            mixed.push_str(&secret[cursor..]);
            variants.push(mixed);
        }
    }

    // Selective Unicode escapes can encode ordinary characters (`a\u0062c`)
    // while preserving the decoded secret. For short credentials, enumerate
    // the same bounded mask space; long credentials still get the complete
    // UTF-16 form and raw form.
    let unicode_units: Vec<(usize, usize, Vec<u16>)> = secret
        .char_indices()
        .map(|(start, ch)| {
            let end = start + ch.len_utf8();
            let mut buf = [0u16; 2];
            let units = ch.encode_utf16(&mut buf).to_vec();
            (start, end, units)
        })
        .collect();
    if unicode_units.len() <= 8 {
        for mask in 0u16..(1u16 << unicode_units.len()) {
            let mut mixed = String::with_capacity(secret.len() * 2);
            for (index, (start, end, units)) in
                unicode_units.iter().enumerate()
            {
                if mask & (1u16 << index) != 0 {
                    for unit in units {
                        mixed.push_str(&format!("\\u{unit:04x}"));
                    }
                } else {
                    mixed.push_str(&secret[*start..*end]);
                }
            }
            variants.push(mixed);
        }
        if unicode_units.len() <= 4 {
            for selected in 0u16..(1u16 << unicode_units.len()) {
                for case_mask in 0u16..(1u16 << unicode_units.len()) {
                    let mut mixed = String::with_capacity(secret.len() * 2);
                    for (index, (start, end, units)) in
                        unicode_units.iter().enumerate()
                    {
                        if selected & (1u16 << index) != 0 {
                            for unit in units {
                                let mut digits = format!("{unit:04X}");
                                if case_mask & (1u16 << index) != 0 {
                                    digits.make_ascii_lowercase();
                                }
                                mixed.push_str("\\u");
                                mixed.push_str(&digits);
                            }
                        } else {
                            mixed.push_str(&secret[*start..*end]);
                        }
                    }
                    variants.push(mixed);
                }
            }
        }
    }

    let mut percent_upper = String::new();
    let mut percent_lower = String::new();
    let mut form = String::new();
    for byte in secret.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            percent_upper.push(ch);
            percent_lower.push(ch);
            form.push(ch);
        } else if *byte == b' ' {
            percent_upper.push_str("%20");
            percent_lower.push_str("%20");
            form.push('+');
        } else {
            percent_upper.push_str(&format!("%{byte:02X}"));
            percent_lower.push_str(&format!("%{byte:02x}"));
            form.push_str(&format!("%{byte:02X}"));
        }
    }
    // Percent escapes are case-insensitive per byte. Enumerate mixed case for
    // short encoded credentials (`%2f%2B`), where a single all-upper or
    // all-lower pass would otherwise miss a reflected credential.
    let percent_tokens: Vec<Option<String>> = secret
        .as_bytes()
        .iter()
        .map(|byte| {
            let ch = *byte as char;
            if ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '_' | '.' | '~')
            {
                None
            } else if *byte == b' ' {
                Some("%20".to_owned())
            } else {
                Some(format!("%{byte:02X}"))
            }
        })
        .collect();
    if percent_tokens.len() <= 8 {
        for mask in 0u16..(1u16 << percent_tokens.len()) {
            let mut mixed = String::with_capacity(secret.len() * 3);
            for (index, token) in percent_tokens.iter().enumerate() {
                if let Some(token) = token {
                    if mask & (1u16 << index) != 0 {
                        mixed.push_str(&token.to_ascii_lowercase());
                    } else {
                        mixed.push_str(token);
                    }
                } else {
                    let byte = secret.as_bytes()[index];
                    mixed.push(byte as char);
                }
            }
            variants.push(mixed);
        }
    }

    variants.push(percent_upper);
    variants.push(percent_lower);
    variants.push(form);
    variants.sort_by_key(|variant| std::cmp::Reverse(variant.len()));
    variants.dedup();
    variants
}

fn redaction_marker(text: &str, variants: &[String]) -> String {
    for suffix in 0..32usize {
        let marker = if suffix == 0 {
            "[REDACTED]".to_owned()
        } else {
            format!("[REDACTED:{suffix}]")
        };
        let variant_collision = variants.iter().any(|variant| {
            if variant.is_empty() {
                return false;
            }
            let ends_with_proper_prefix = variant
                .char_indices()
                .skip(1)
                .any(|(end, _)| marker.ends_with(&variant[..end]));
            marker.contains(variant.as_str())
                || variant.contains(&marker)
                || ends_with_proper_prefix
        });
        if !text.contains(&marker) && !variant_collision {
            return marker;
        }
    }
    // Empty output is the fail-closed fallback when every fixed marker has a
    // substring collision with a deliberately chosen credential.
    String::new()
}

fn hex_unit(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 4 {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..4]).ok()?;
    u16::from_str_radix(text, 16).ok()
}

fn json_escape_at(text: &str, at: usize) -> Option<(u16, usize)> {
    let rest = text.get(at..)?;
    let bytes = rest.as_bytes();
    if bytes.first().copied()? != b'\\' {
        return None;
    }
    let escaped = *bytes.get(1)?;
    let value = match escaped {
        b'"' => b'"' as u16,
        b'\\' => b'\\' as u16,
        b'/' => b'/' as u16,
        b'n' => b'\n' as u16,
        b'r' => b'\r' as u16,
        b't' => b'\t' as u16,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'u' => return hex_unit(bytes.get(2..)?).map(|unit| (unit, 6)),
        _ => return None,
    };
    Some((value, 2))
}

fn hex_byte(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 2 {
        return None;
    }
    let text = std::str::from_utf8(&bytes[..2]).ok()?;
    u8::from_str_radix(text, 16).ok()
}

#[derive(Clone, Copy)]
struct RedactionToken {
    value: u16,
    start: usize,
    end: usize,
}

const NON_UTF16_TOKEN: u16 = u16::MAX;
const PERCENT_LITERAL_PLUS_TOKEN: u16 = u16::MAX - 1;

fn redact_token_matches(
    text: &str,
    pattern: &[u16],
    tokens: &[RedactionToken],
    marker: &str,
) -> String {
    if pattern.is_empty() || tokens.is_empty() {
        return text.to_owned();
    }
    let failure = kmp_failure(pattern);
    let mut output = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut matched = 0usize;
    let mut starts = std::collections::VecDeque::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let token = tokens[index];
        if token.value == pattern[matched] {
            starts.push_back(token.start);
            matched += 1;
            index += 1;
            if matched == pattern.len() {
                // Replace from the first source token in the KMP match, not
                // from the final token. Encoded credentials often have a
                // multi-token prefix; copying the prefix would disclose it.
                let start = starts.front().copied().unwrap_or(token.start);
                if start >= copied {
                    output.push_str(&text[copied..start]);
                }
                output.push_str(marker);
                copied = token.end;

                // Retain the longest proper border so a subsequent match can
                // begin in source already covered by this replacement.
                let next = failure[matched - 1];
                for _ in 0..(matched - next) {
                    let _ = starts.pop_front();
                }
                matched = next;
            }
        } else if matched > 0 {
            // Re-evaluate this token after the KMP fallback, retaining the
            // source starts for the surviving suffix of the match.
            let next = failure[matched - 1];
            for _ in 0..(matched - next) {
                let _ = starts.pop_front();
            }
            matched = next;
        } else {
            // Copy complete UTF-8 source spans, not individual bytes.
            output.push_str(&text[copied..token.end]);
            copied = token.end;
            index += 1;
        }
    }
    output.push_str(&text[copied..]);
    output
}

fn json_tokens(text: &str, pattern_is_ascii: bool) -> Vec<RedactionToken> {
    let mut tokens = Vec::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        if let Some((unit, width)) = json_escape_at(text, cursor) {
            tokens.push(RedactionToken {
                value: if pattern_is_ascii && unit > u8::MAX as u16 {
                    NON_UTF16_TOKEN
                } else {
                    unit
                },
                start: cursor,
                end: cursor + width,
            });
            cursor += width;
            continue;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor is a character boundary");
        let end = cursor + character.len_utf8();
        if pattern_is_ascii && !character.is_ascii() {
            tokens.push(RedactionToken {
                value: NON_UTF16_TOKEN,
                start: cursor,
                end,
            });
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units) {
                tokens.push(RedactionToken {
                    value: *unit,
                    start: cursor,
                    end,
                });
            }
        }
        cursor = end;
    }
    tokens
}

fn percent_tokens(text: &str) -> Vec<RedactionToken> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%' {
            if let Some(slice) = bytes.get(cursor + 1..cursor + 3) {
                if let Some(value) = hex_byte(slice) {
                    let value = if value == b'+' {
                        PERCENT_LITERAL_PLUS_TOKEN
                    } else {
                        u16::from(value)
                    };
                    tokens.push(RedactionToken {
                        value,
                        start: cursor,
                        end: cursor + 3,
                    });
                    cursor += 3;
                    continue;
                }
            }
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor is a character boundary");
        let end = cursor + character.len_utf8();
        if character == '+' {
            tokens.push(RedactionToken {
                value: u16::from(b' '),
                start: cursor,
                end,
            });
        } else {
            let mut encoded = [0u8; 4];
            for byte in character.encode_utf8(&mut encoded).as_bytes() {
                tokens.push(RedactionToken {
                    value: u16::from(*byte),
                    start: cursor,
                    end,
                });
            }
        }
        cursor = end;
    }
    tokens
}

fn redact_encoded_json_ascii(
    text: &str,
    secret: &str,
    marker: &str,
) -> String {
    if secret.is_empty() || !secret.is_ascii() {
        return text.to_owned();
    }
    let pattern: Vec<u16> = secret.bytes().map(u16::from).collect();
    let tokens = json_tokens(text, true);
    redact_token_matches(text, &pattern, &tokens, marker)
}

fn redact_encoded_json(text: &str, secret: &str, marker: &str) -> String {
    if secret.is_ascii() {
        return redact_encoded_json_ascii(text, secret, marker);
    }
    let mut pattern = Vec::new();
    for character in secret.chars() {
        let mut encoded = [0u16; 2];
        pattern.extend_from_slice(character.encode_utf16(&mut encoded));
    }
    if pattern.is_empty() {
        return text.to_owned();
    }
    let tokens = json_tokens(text, false);
    redact_token_matches(text, &pattern, &tokens, marker)
}

fn kmp_failure<T: PartialEq>(pattern: &[T]) -> Vec<usize> {
    let mut failure = vec![0usize; pattern.len()];
    let mut prefix = 0usize;
    for index in 1..pattern.len() {
        while prefix > 0 && pattern[index] != pattern[prefix] {
            prefix = failure[prefix - 1];
        }
        if pattern[index] == pattern[prefix] {
            prefix += 1;
        }
        failure[index] = prefix;
    }
    failure
}

fn redact_encoded_percent(text: &str, secret: &str, marker: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    let pattern: Vec<u16> = secret
        .bytes()
        .map(|byte| {
            if byte == b'+' {
                PERCENT_LITERAL_PLUS_TOKEN
            } else {
                u16::from(byte)
            }
        })
        .collect();
    let tokens = percent_tokens(text);
    redact_token_matches(text, &pattern, &tokens, marker)
}

fn advance_prefix_state<T: PartialEq>(
    pattern: &[T],
    failure: &[usize],
    matched: &mut usize,
    starts: &mut std::collections::VecDeque<usize>,
    value: T,
    start: usize,
) {
    loop {
        if value == pattern[*matched] {
            starts.push_back(start);
            *matched += 1;
            if *matched == pattern.len() {
                *matched = 0;
                starts.clear();
            }
            return;
        }
        if *matched == 0 {
            return;
        }
        let next = failure[*matched - 1];
        for _ in 0..(*matched - next) {
            let _ = starts.pop_front();
        }
        *matched = next;
    }
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn partial_json_escape_at(text: &str, cursor: usize) -> bool {
    let rest = &text.as_bytes()[cursor..];
    if rest.first().copied() != Some(b'\\') {
        return false;
    }
    if rest.len() == 1 {
        return true;
    }
    rest.get(1).copied() == Some(b'u')
        && rest.len() < 6
        && rest[2..].iter().all(|byte| hex_nibble(*byte).is_some())
}

fn partial_json_escape_matches(
    text: &str,
    cursor: usize,
    expected: u16,
) -> bool {
    if !partial_json_escape_at(text, cursor) {
        return false;
    }
    let rest = &text.as_bytes()[cursor..];
    if rest.len() == 1 {
        return true;
    }
    let digits = &rest[2..];
    let expected = format!("{expected:04x}");
    digits
        .iter()
        .zip(expected.as_bytes())
        .all(|(actual, expected)| hex_nibble(*actual) == hex_nibble(*expected))
}

fn json_prefix_start(text: &str, secret: &str) -> Option<usize> {
    let mut pattern = Vec::new();
    for character in secret.chars() {
        let mut encoded = [0u16; 2];
        pattern.extend_from_slice(character.encode_utf16(&mut encoded));
    }
    if pattern.is_empty() {
        return None;
    }

    let mut tokens = Vec::with_capacity(text.len());
    let mut partial_start = None;
    let mut cursor = 0usize;
    while cursor < text.len() {
        if let Some((unit, width)) = json_escape_at(text, cursor) {
            tokens.push((unit, cursor));
            cursor += width;
            continue;
        }
        if partial_json_escape_at(text, cursor) {
            partial_start = Some(cursor);
            break;
        }
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor is a character boundary");
        let start = cursor;
        let end = start + character.len_utf8();
        let mut encoded = [0u16; 2];
        for unit in character.encode_utf16(&mut encoded) {
            tokens.push((*unit, start));
        }
        cursor = end;
    }

    let failure = kmp_failure(&pattern);
    let mut matched = 0usize;
    let mut starts = std::collections::VecDeque::new();
    for (unit, start) in tokens {
        advance_prefix_state(
            &pattern,
            &failure,
            &mut matched,
            &mut starts,
            unit,
            start,
        );
    }
    if let Some(partial_start) = partial_start {
        // A partial JSON escape can complete any surviving KMP border, not
        // just the longest current match. Hold from the earliest viable
        // source start so overlapping prefixes cannot leak across chunks.
        let mut earliest = None;
        for (state, unit) in
            pattern.iter().copied().take(matched + 1).enumerate()
        {
            if partial_json_escape_matches(text, partial_start, unit) {
                let start =
                    starts.get(state).copied().unwrap_or(partial_start);
                earliest =
                    Some(earliest.map_or(start, |old: usize| old.min(start)));
            }
        }
        if earliest.is_some() {
            return earliest;
        }
    }
    starts.front().copied()
}

fn partial_percent_escape_at(bytes: &[u8], cursor: usize) -> bool {
    let rest = &bytes[cursor..];
    rest.first().copied() == Some(b'%')
        && (rest.len() == 1
            || rest.get(1).copied().and_then(hex_nibble).is_some())
}

fn partial_percent_escape_matches(
    bytes: &[u8],
    cursor: usize,
    expected: u8,
) -> bool {
    let rest = &bytes[cursor..];
    if rest.first().copied() != Some(b'%') {
        return false;
    }
    if rest.len() == 1 {
        return true;
    }
    let Some(high) = rest.get(1).copied().and_then(hex_nibble) else {
        return false;
    };
    if high != expected >> 4 {
        return false;
    }
    if rest.len() == 2 {
        return true;
    }
    rest.get(2)
        .copied()
        .and_then(hex_nibble)
        .is_some_and(|low| low == expected & 0x0f)
}

fn percent_prefix_start(text: &str, secret: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let pattern: Vec<u16> = secret
        .bytes()
        .map(|byte| {
            if byte == b'+' {
                PERCENT_LITERAL_PLUS_TOKEN
            } else {
                u16::from(byte)
            }
        })
        .collect();
    if pattern.is_empty() {
        return None;
    }

    let failure = kmp_failure(&pattern);
    let mut matched = 0usize;
    let mut starts = std::collections::VecDeque::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%' {
            if let Some(value) =
                bytes.get(cursor + 1..cursor + 3).and_then(hex_byte)
            {
                let value = if value == b'+' {
                    PERCENT_LITERAL_PLUS_TOKEN
                } else {
                    u16::from(value)
                };
                advance_prefix_state(
                    &pattern,
                    &failure,
                    &mut matched,
                    &mut starts,
                    value,
                    cursor,
                );
                cursor += 3;
                continue;
            }
            if partial_percent_escape_at(bytes, cursor) {
                let mut state = matched;
                loop {
                    let expected =
                        if pattern[state] == PERCENT_LITERAL_PLUS_TOKEN {
                            Some(b'+')
                        } else {
                            u8::try_from(pattern[state]).ok()
                        };
                    if let Some(expected) = expected {
                        if partial_percent_escape_matches(
                            bytes, cursor, expected,
                        ) {
                            return starts.front().copied().or(Some(cursor));
                        }
                    }
                    if state == 0 {
                        break;
                    }
                    state = failure[state - 1];
                }
            }
        }

        let value = if bytes[cursor] == b'+' {
            u16::from(b' ')
        } else {
            u16::from(bytes[cursor])
        };
        advance_prefix_state(
            &pattern,
            &failure,
            &mut matched,
            &mut starts,
            value,
            cursor,
        );
        cursor += 1;
    }
    starts.front().copied()
}

fn source_prefix_start(text: &str, secret: &str) -> Option<usize> {
    json_prefix_start(text, secret)
        .into_iter()
        .chain(percent_prefix_start(text, secret))
        .min()
}

fn variant_prefix_len(text: &str, variants: &[String]) -> usize {
    let bytes = text.as_bytes();
    let mut longest = 0usize;
    for variant in variants {
        let pattern = variant.as_bytes();
        if pattern.len() <= 1 {
            continue;
        }
        let start = bytes.len().saturating_sub(pattern.len() - 1);
        let failure = kmp_failure(pattern);
        let mut matched = 0usize;
        for byte in &bytes[start..] {
            while matched > 0 && *byte != pattern[matched] {
                matched = failure[matched - 1];
            }
            if *byte == pattern[matched] {
                matched += 1;
            }
        }
        longest = longest.max(matched);
    }
    longest
}

/// One node in the small multi-pattern matcher used by credential redaction.
///
/// Keeping the matcher local avoids an unbounded `replace` pass for every
/// JSON/percent variant. The matcher retains exact pattern coverage while its
/// work is linear in the text and the total pattern bytes.
#[derive(Clone, Default)]
struct RedactionTrieNode {
    edges: Vec<(u8, usize)>,
    fail: usize,
    own_len: usize,
    output_len: usize,
}

/// A compiled Aho-Corasick automaton over the credential's representations.
///
/// The automaton is built ONCE and reused for every chunk or field. Building
/// it per call made the redaction cost proportional to `chunks x variant bytes`
/// -- measured at ~540 ms per call for a 4 KiB control-heavy `key:` credential,
/// i.e. ~70 s of CPU for a single 1 MiB streamed response. Construction is now
/// paid once per redactor.
struct VariantMatcher {
    nodes: Vec<RedactionTrieNode>,
}

impl VariantMatcher {
    fn new(variants: &[String]) -> Self {
        let mut nodes = vec![RedactionTrieNode::default()];
        for variant in variants {
            if variant.is_empty() {
                continue;
            }
            let mut state = 0usize;
            for byte in variant.as_bytes() {
                let next =
                    nodes[state].edges.iter().find_map(|(edge, next)| {
                        (*edge == *byte).then_some(*next)
                    });
                state = match next {
                    Some(next) => next,
                    None => {
                        let next = nodes.len();
                        nodes.push(RedactionTrieNode::default());
                        nodes[state].edges.push((*byte, next));
                        next
                    }
                };
            }
            nodes[state].own_len = variant.len();
        }
        if nodes.len() == 1 {
            return Self { nodes };
        }

        let mut queue = std::collections::VecDeque::new();
        for (_, child) in nodes[0].edges.clone() {
            nodes[child].fail = 0;
            nodes[child].output_len = nodes[child].own_len;
            queue.push_back(child);
        }
        while let Some(parent) = queue.pop_front() {
            for (edge, child) in nodes[parent].edges.clone() {
                let mut fallback = nodes[parent].fail;
                loop {
                    if let Some((_, next)) = nodes[fallback]
                        .edges
                        .iter()
                        .find(|(candidate, _)| *candidate == edge)
                    {
                        nodes[child].fail = *next;
                        break;
                    }
                    if fallback == 0 {
                        nodes[child].fail = 0;
                        break;
                    }
                    fallback = nodes[fallback].fail;
                }
                nodes[child].output_len = nodes[child]
                    .own_len
                    .max(nodes[nodes[child].fail].output_len);
                queue.push_back(child);
            }
        }
        Self { nodes }
    }

    fn replace(&self, text: &str, marker: &str) -> String {
        if self.nodes.len() == 1 {
            return text.to_owned();
        }
        let mut state = 0usize;
        let mut output = String::with_capacity(text.len());
        let mut copied_through = 0usize;
        for (index, byte) in text.as_bytes().iter().copied().enumerate() {
            loop {
                if let Some((_, next)) = self.nodes[state]
                    .edges
                    .iter()
                    .find(|(candidate, _)| *candidate == byte)
                {
                    state = *next;
                    break;
                }
                if state == 0 {
                    break;
                }
                state = self.nodes[state].fail;
            }
            let length = self.nodes[state].output_len;
            if length > 0 {
                let end = index + 1;
                let start = end.saturating_sub(length);
                if start >= copied_through {
                    output.push_str(&text[copied_through..start]);
                }
                output.push_str(marker);
                copied_through = end;
            }
        }
        output.push_str(&text[copied_through..]);
        output
    }
}

fn replace_variant_matches(
    text: &str,
    variants: &[String],
    marker: &str,
) -> String {
    VariantMatcher::new(variants).replace(text, marker)
}

fn redact_sensitive_with_variants(
    text: &str,
    variants: &[String],
    secret: Option<&str>,
) -> String {
    let marker = redaction_marker(text, variants);
    let mut redacted = replace_variant_matches(text, variants, &marker);
    if let Some(secret) = secret.filter(|value| !value.is_empty()) {
        redacted = redact_encoded_json(&redacted, secret, &marker);
        redacted = redact_encoded_percent(&redacted, secret, &marker);
    }
    redacted
}

/// The same projection as [`redact_sensitive_with_variants`], reusing a
/// matcher that was compiled once.
fn redact_sensitive_with_matcher(
    text: &str,
    variants: &[String],
    matcher: &VariantMatcher,
    secret: Option<&str>,
) -> String {
    let marker = redaction_marker(text, variants);
    let mut redacted = matcher.replace(text, &marker);
    if let Some(secret) = secret.filter(|value| !value.is_empty()) {
        redacted = redact_encoded_json(&redacted, secret, &marker);
        redacted = redact_encoded_percent(&redacted, secret, &marker);
    }
    redacted
}

/// Redact one exact Host credential from a diagnostic string. The
/// replacement marker is selected so even a credential equal to the ordinary
/// marker cannot be reflected.
pub(crate) fn redact_sensitive(text: &str, secret: Option<&str>) -> String {
    let Some(secret) = secret.filter(|value| !value.is_empty()) else {
        return text.to_owned();
    };
    let variants = sensitive_variants(secret);
    redact_sensitive_with_variants(text, &variants, Some(secret))
}

fn redacted_placeholder(secret: &str) -> String {
    let variants = sensitive_variants(secret);
    redaction_marker("", &variants)
}

/// Redact an exact resolved host credential in a status/display projection.
/// This public seam keeps frontends from reimplementing secret matching.
pub fn redact_host_display(
    text: &str,
    credential: Option<&HostCredential>,
) -> String {
    let secret = credential
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
    redact_sensitive(text, secret.as_deref())
}

/// Redact provider output before it crosses the provider/Host boundary.
/// The credential is exact-literal, so a short/nonstandard key is handled as
/// well as a conventionally-shaped key. The event shape remains unchanged.
pub(crate) fn redact_provider_event(
    event: ProviderEvent,
    secret: Option<&str>,
) -> ProviderEvent {
    if secret.is_none_or(str::is_empty) {
        return event;
    }
    let secret = secret.expect("checked above");
    match event {
        ProviderEvent::Event(ModelEvent::TextDelta { text }) => {
            ProviderEvent::Event(ModelEvent::TextDelta {
                text: redact_body_with_secret(&text, Some(secret)),
            })
        }
        ProviderEvent::Event(ModelEvent::ReasoningDelta { text }) => {
            ProviderEvent::Event(ModelEvent::ReasoningDelta {
                text: redact_body_with_secret(&text, Some(secret)),
            })
        }
        ProviderEvent::Event(ModelEvent::ToolCall {
            call_id,
            tool_name,
            input,
        }) => {
            let serialized = input.serialized_json();
            let safe = redact_body_with_secret(&serialized, Some(secret));
            let value = serde_json::from_str(&safe).unwrap_or_else(|_| {
                serde_json::Value::String(redacted_placeholder(secret))
            });
            ProviderEvent::Event(ModelEvent::ToolCall {
                call_id: redact_sensitive(&call_id, Some(secret)),
                tool_name: redact_sensitive(&tool_name, Some(secret)),
                input: ToolCallInput::from_value(value),
            })
        }
        ProviderEvent::Raw(value) => {
            let serialized = serde_json::to_string(&value).unwrap_or_default();
            let safe = redact_body_with_secret(&serialized, Some(secret));
            ProviderEvent::Raw(serde_json::from_str(&safe).unwrap_or(
                serde_json::Value::String(redacted_placeholder(secret)),
            ))
        }
        ProviderEvent::Failed(message) => {
            ProviderEvent::Failed(redact_sensitive(&message, Some(secret)))
        }
        ProviderEvent::Cancelled { message } => ProviderEvent::Cancelled {
            message: redact_sensitive(&message, Some(secret)),
        },
        ProviderEvent::Event(other) => ProviderEvent::Event(other),
    }
}

/// Stateful stream redactor used at a provider boundary. It retains at most
/// the longest suffix that could be the prefix of the credential, so a secret
/// split across provider chunks is masked once the next chunk arrives.
pub(crate) struct StreamingSecretRedactor {
    secret: Option<String>,
    variants: Vec<String>,
    /// Compiled once here rather than per chunk: see [`VariantMatcher`].
    matcher: VariantMatcher,
    pending: String,
}

impl StreamingSecretRedactor {
    #[must_use]
    pub(crate) fn new(secret: Option<&str>) -> Self {
        let secret =
            secret.filter(|value| !value.is_empty()).map(str::to_owned);
        let variants =
            secret.as_deref().map(sensitive_variants).unwrap_or_default();
        let matcher = VariantMatcher::new(&variants);
        Self { secret, variants, matcher, pending: String::new() }
    }

    fn redact_text(&mut self, text: &str) -> String {
        if self.secret.is_none() {
            return text.to_owned();
        }
        let mut combined = std::mem::take(&mut self.pending);
        combined.push_str(text);
        let mut redacted = redact_sensitive_with_matcher(
            &combined,
            &self.variants,
            &self.matcher,
            self.secret.as_deref(),
        );
        // Retain every proper prefix of every representation. A provider can
        // split raw, JSON-escaped, or percent-encoded credentials across
        // arbitrary events; emitting a prefix would reconstruct the secret in
        // the frontend transcript.
        let mut hold = variant_prefix_len(&redacted, &self.variants);
        if let Some(secret) = self.secret.as_deref() {
            if let Some(start) = source_prefix_start(&redacted, secret) {
                hold = hold.max(redacted.len().saturating_sub(start));
            }
        }
        const MAX_PENDING_REDACTION_BYTES: usize = 64 * 1024;
        if hold > MAX_PENDING_REDACTION_BYTES {
            let split = redacted.len() - hold;
            self.pending.clear();
            redacted.truncate(split);
            redacted.push_str(&redaction_marker("", &self.variants));
            return redacted;
        }
        if hold > 0 {
            let split = redacted.len() - hold;
            self.pending = redacted.split_off(split);
        }
        redacted
    }

    #[must_use]
    pub(crate) fn redact_event(
        &mut self,
        event: ProviderEvent,
    ) -> ProviderEvent {
        match event {
            ProviderEvent::Event(ModelEvent::TextDelta { text }) => {
                let text = self.redact_text(&text);
                ProviderEvent::Event(ModelEvent::TextDelta { text })
            }
            ProviderEvent::Event(ModelEvent::ReasoningDelta { text }) => {
                let text = self.redact_text(&text);
                ProviderEvent::Event(ModelEvent::ReasoningDelta { text })
            }
            other => redact_provider_event(other, self.secret.as_deref()),
        }
    }

    /// Replace a held ambiguous prefix after the provider has completed.
    #[must_use]
    pub(crate) fn finish(&mut self) -> Option<String> {
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            return None;
        }
        Some(redaction_marker("", &self.variants))
    }
}

fn redact_json_value(
    value: &mut serde_json::Value,
    secret: Option<&str>,
) -> bool {
    match value {
        serde_json::Value::String(text) => {
            let safe = redact_sensitive(text, secret);
            if safe == *text {
                false
            } else {
                *text = safe;
                true
            }
        }
        serde_json::Value::Array(values) => {
            let mut changed = false;
            for value in values.iter_mut() {
                changed |= redact_json_value(value, secret);
            }
            changed
        }
        serde_json::Value::Object(fields) => {
            let entries = std::mem::take(fields);
            let mut changed = false;
            for (key, mut value) in entries {
                let safe_key = redact_sensitive(&key, secret);
                changed |= safe_key != key;
                changed |= redact_json_value(&mut value, secret);
                // A collision can only arise when redaction changes a
                // provider-controlled key. Keep both entries with a
                // deterministic suffix rather than dropping untrusted data.
                let mut key = safe_key;
                if fields.contains_key(&key) {
                    let suffix = secret
                        .filter(|value| !value.is_empty())
                        .map(|value| {
                            redaction_marker("", &sensitive_variants(value))
                        })
                        .unwrap_or_else(|| "?".to_owned());
                    key.push_str(&suffix);
                    let mut index = 1usize;
                    let base = key.clone();
                    while fields.contains_key(&key) {
                        key = format!("{base}-{index}");
                        index += 1;
                    }
                    changed = true;
                }
                fields.insert(key, value);
            }
            changed
        }
        _ => false,
    }
}

/// Redact a complete bounded response body.
///
/// JSON is sanitized structurally so a credential nested in any string, key, or
/// serialized argument is found, and the JSON/percent encoded forms of the
/// literal are still matched. A complete body has no stream boundary to
/// resolve, so an exact full-value match is the contract here: prefix holding
/// belongs to [`StreamingSecretRedactor`], which alone decides what is still
/// ambiguous at a chunk edge. Over-redacting an unrelated trailing word would
/// corrupt a replayable recording.
pub(crate) fn redact_body_with_secret(
    text: &str,
    secret: Option<&str>,
) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) else {
        return redact_sensitive(text, secret);
    };
    if !redact_json_value(&mut value, secret) {
        return text.to_owned();
    }
    serde_json::to_string(&value)
        .unwrap_or_else(|_| redact_sensitive(text, secret))
}

/// Project an endpoint to a non-secret route label. Only the validated scheme
/// and authority remain; userinfo, path, query, and fragment are discarded.
#[must_use]
pub fn safe_endpoint_for_output(endpoint: &str) -> String {
    let without_tail = endpoint.split(['?', '#']).next().unwrap_or(endpoint);
    let Some((scheme, rest)) = without_tail.split_once("://") else {
        return "[invalid endpoint]".to_owned();
    };
    if !matches!(scheme, "http" | "https") {
        return "[invalid endpoint]".to_owned();
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty()
        || authority.contains('@')
        || authority.chars().any(char::is_control)
    {
        return "[invalid endpoint]".to_owned();
    }
    format!("{scheme}://{authority}")
}

/// Project an endpoint and remove the active resolved credential if it
/// happens to occur in the authority label.
#[must_use]
pub fn safe_endpoint_for_credential(
    endpoint: &str,
    credential: Option<&HostCredential>,
) -> String {
    let safe = safe_endpoint_for_output(endpoint);
    credential.map_or(safe.clone(), |value| value.redact_text(&safe))
}

/// Compute the `sha256` hex of the sanitized bounded body text.
pub(crate) fn response_body_sha256(text: &str) -> String {
    siralos_core::identity::sha256_hex(text.as_bytes())
}

/// Cancellation message for a signal tripped before the provider starts.
pub(crate) const CANCELLED_BEFORE_PROVIDER_START: &str =
    "Host cancelled the turn before provider start";

/// Cancellation message for a signal already tripped before the provider call.
pub(crate) const CANCELLED_BEFORE_HTTP_CALL: &str =
    "Host cancelled before HTTP call";

/// Cancellation message for a signal tripped before the request is sent.
pub(crate) const CANCELLED_BEFORE_HTTP_SEND: &str =
    "Host cancelled before HTTP send";

/// Cancellation message for a signal tripped once the response has arrived.
pub(crate) const CANCELLED_AFTER_HTTP_RESPONSE: &str =
    "Host cancelled after HTTP response";

/// The `anthropic-version` header value every Anthropic-shaped path sends.
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The replay reason before any provider response has been observed.
///
/// It is the initial `last_replay` value and the one restored by
/// `take_last_replay_availability`; the probe asserts this exact text.
pub(crate) const NO_PROVIDER_RESPONSE_OBSERVED: &str =
    "no provider response observed yet";

fn response_identity(
    hooks: &ReplayHooks,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) -> siralos_core::determinism::ProviderResponseIdentity {
    let body_sha256 = response_body_sha256(body_text);
    let observed_at_ms = hooks.clock.as_ref().map(|clock| clock.now_ms());
    let usage =
        siralos_core::determinism::provider_replay::parse_provider_usage(
            body_text,
        );
    siralos_core::determinism::ProviderResponseIdentity {
        provider_id: provider_id.to_owned(),
        model: model.to_owned(),
        status,
        body_sha256,
        body_bytes: body_text.len() as u64,
        observed_at_ms,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_tokens: usage.cached_tokens,
    }
}

fn record_evidence_identity(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    identity: &siralos_core::determinism::ProviderResponseIdentity,
    body_text: &str,
) {
    if let Some(recorder) = hooks.recorder.as_ref() {
        let body_retained = recorder
            .try_record_provider_response_with_body_as_evidence(
                identity, body_text,
            );
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: if body_retained && !body_text.is_empty() {
                    "response is not a successful replayable completion"
                        .to_owned()
                } else {
                    "replay body was not retained".to_owned()
                },
            };
    } else {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "live call not recorded".to_owned(),
            };
    }
}

/// Record a provider outcome as terminal evidence, bypassing the replay-shape
/// classifier. Use this for provider-specific protocol/validation failures.
pub(crate) fn record_evidence_outcome(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) {
    let identity =
        response_identity(hooks, provider_id, model, status, body_text);
    record_evidence_identity(hooks, last_replay, &identity, body_text);
}

/// Record terminal evidence after exact-credential redaction.
pub(crate) fn record_evidence_outcome_with_secret(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_evidence_outcome(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        &safe_body,
    );
}

/// Record one provider outcome after exact-credential redaction. The
/// unredacted body is never handed to the recorder.
pub(crate) fn record_outcome_with_secret(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_outcome(hooks, last_replay, provider_id, model, status, &safe_body);
}

/// Record one provider HTTP outcome for replay.
///
/// `body_text` must be the sanitized bounded text (never the credential).
/// A successful response sets `Recorded` only after the recorder accepts its
/// body; a request digest, when present, is retained through the route-bound
/// operation. A low-level caller with no digest uses the explicit legacy body
/// method, so `Recorded` there means legacy playback only, not route binding.
/// Non-replayable or rejected outcomes remain `Unavailable`.
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
    record_outcome_bound(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        body_text,
        None,
    );
}

fn record_outcome_bound(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    request_sha256_override: Option<&str>,
) {
    let body_sha256 = response_body_sha256(body_text);
    let body_bytes = body_text.len() as u64;
    let observed_at_ms = hooks.clock.as_ref().map(|c| c.now_ms());
    let usage =
        siralos_core::determinism::provider_replay::parse_provider_usage(
            body_text,
        );
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
    let replayable = status.is_some_and(|status| (200..300).contains(&status))
        && !body_text.is_empty()
        && !crate::provider::replay::completion_events_from_body(body_text)
            .iter()
            .any(|event| {
                matches!(
                    event,
                    siralos_core::provider::ProviderEvent::Failed(_)
                )
            });
    if !replayable {
        if let Some(recorder) = hooks.recorder.as_ref() {
            // Preserve sanitized terminal evidence in the recorder, but keep
            // availability unavailable: the shared persistence projection
            // filters it from the replay-only store. Evidence has its own
            // bounded budget, so replay saturation must not suppress it.
            let body_retained = recorder
                .try_record_provider_response_with_body_as_evidence(
                    &identity, body_text,
                );
            *last_replay.borrow_mut() =
                siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                    reason: if body_retained && !body_text.is_empty() {
                        "response is not a successful replayable completion"
                            .to_owned()
                    } else {
                        "replay body was not retained".to_owned()
                    },
                };
        } else {
            *last_replay.borrow_mut() =
                siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                    reason: "live call not recorded".to_owned(),
                };
        }
        return;
    }
    if hooks.recorder.as_ref().is_some_and(|r| r.is_recording()) {
        if let Some(recorder) = hooks.recorder.as_ref() {
            // A digest is bound whenever the normal provider stream path
            // prepared the route. Low-level legacy callers may invoke the
            // transport helper without that preparation; retain their body via
            // the explicitly legacy method instead of manufacturing a binding.
            let request_sha256 = request_sha256_override
                .map(str::to_owned)
                .or_else(|| hooks.request_sha256.borrow().clone());
            let body_recorded = if let Some(request_sha256) = request_sha256 {
                recorder.try_record_provider_response_with_body_and_request(
                    &identity,
                    body_text,
                    &request_sha256,
                )
            } else {
                recorder.try_record_provider_response_with_body(
                    &identity, body_text,
                )
            };
            if !body_recorded {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                        reason: "replay body was not retained".to_owned(),
                    };
                return;
            }
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

/// Record one provider outcome after exact-credential redaction, binding the
/// request digest this turn computed rather than the shared hook cell. The
/// streaming turn owns its digest so two interleaved turns cannot persist
/// response A under request B.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_outcome_with_secret_and_request_digest(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
    secret: Option<&str>,
    request_sha256: &str,
) {
    let safe_body = redact_body_with_secret(body_text, secret);
    record_outcome_bound(
        hooks,
        last_replay,
        provider_id,
        model,
        status,
        &safe_body,
        Some(request_sha256),
    );
}

/// Maximum provider response body bytes accepted before truncation.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn bounded_body_text_from_bytes(
    mut bytes: Vec<u8>,
    secret: Option<&str>,
) -> String {
    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    bytes.truncate(MAX_RESPONSE_BYTES);
    if truncated {
        if let Some(secret) = secret.filter(|value| !value.is_empty()) {
            let max_encoded_len = secret
                .encode_utf16()
                .count()
                .saturating_mul(6)
                .max(secret.len().saturating_mul(3));
            let overlap = max_encoded_len.saturating_sub(1).min(bytes.len());
            bytes.truncate(bytes.len().saturating_sub(overlap));
        }
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("...[truncated]");
    }
    redact_sensitive(&text, secret)
}

/// Read an HTTP response body bounded at read time while retaining its bytes
/// for exact credential redaction. Control characters are not discarded here;
/// callers must redact the raw bounded text before copying any part of it to a
/// diagnostic or sanitized output surface.
pub(crate) fn bounded_body_text_raw(
    response: reqwest::blocking::Response,
    secret: Option<&str>,
) -> Result<String, String> {
    use std::io::Read;
    let mut limited = response.take((MAX_RESPONSE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    Ok(bounded_body_text_from_bytes(bytes, secret))
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
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
}

/// What one shared chat-pipeline call produced.
pub(crate) enum ChatPipelineOutcome {
    /// Terminal: the failure is already recorded, and the caller returns these.
    Events(Vec<siralos_core::provider::ProviderEvent>),
    /// The response parsed. The caller performs its own shape extraction.
    Parsed {
        /// The status the body arrived with.
        status: reqwest::StatusCode,
        /// The bounded, sanitized body text, exactly as recorded for replay.
        text: String,
        /// The parsed JSON body.
        value: serde_json::Value,
    },
}

/// Run the region the two chat clients share once a request is ready to send.
///
/// The caller owns everything up to and including building the request — URL,
/// headers and body differ per client — and its own post-parse extraction. This
/// owns the send and its failure mapping, the post-response cancellation check,
/// the bounded read and its failure, the non-success mapping, and the JSON parse
/// and its failure, recording each terminal outcome through [`record_outcome`].
///
/// The two terminal error paths do not reflect provider-controlled response
/// bodies. Their bounded bodies are still passed to the replay recorder for
/// exact, credential-redacted persistence; only the user/event message carries
/// provider/status classification.
// The shared boundary intentionally keeps the request builder, secret redaction,
// cancellation, replay hooks, and state cell as distinct arguments rather than
// hiding them in a broad context object.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_chat_pipeline(
    provider: &str,
    model: &str,
    url: &str,
    request: reqwest::blocking::RequestBuilder,
    secret: Option<&str>,
    cancellation: siralos_core::provider::CancellationSignal<'_>,
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
) -> ChatPipelineOutcome {
    use siralos_core::provider::ProviderEvent;
    // Do not echo reqwest's transport error: it can contain the complete
    // destination URL (including query/userinfo). Report only the bounded,
    // userinfo/query-stripped URL supplied by the caller.
    let safe_url = safe_endpoint_for_output(url);
    let response = match request.send() {
        Ok(response) => response,
        Err(_err) => {
            let events = vec![ProviderEvent::Failed(redact_sensitive(
                &format!(
                    "{provider} request failed: transport unavailable at {safe_url}"
                ),
                secret,
            ))];
            record_outcome(hooks, last_replay, provider, model, None, "");
            return ChatPipelineOutcome::Events(events);
        }
    };
    if cancellation.is_cancelled() {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "call cancelled".to_owned(),
            };
        return ChatPipelineOutcome::Events(vec![ProviderEvent::Cancelled {
            message: CANCELLED_AFTER_HTTP_RESPONSE.to_owned(),
        }]);
    }
    let status = response.status();
    // Bound the response body at READ time (at most 1 MiB is buffered)
    // and sanitize untrusted data before embedding it in the
    // Host-visible diagnostic.
    let text = if status.as_u16() == 204 {
        String::new()
    } else {
        match bounded_body_text_raw(response, secret) {
            Ok(text) => text,
            Err(_err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} response read failed"
                ))];
                record_outcome(
                    hooks,
                    last_replay,
                    provider,
                    model,
                    Some(status.as_u16()),
                    "",
                );
                return ChatPipelineOutcome::Events(events);
            }
        }
    };
    if !status.is_success() {
        // Do not reflect any provider-controlled body in a user/event string.
        // The bounded, credential-redacted body remains available only to the
        // replay recorder through the typed hook below.
        let events =
            vec![ProviderEvent::Failed(format!("{provider} error {status}"))];
        record_outcome_with_secret(
            hooks,
            last_replay,
            provider,
            model,
            Some(status.as_u16()),
            &text,
            secret,
        );
        return ChatPipelineOutcome::Events(events);
    }
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_err) => {
            let events = vec![ProviderEvent::Failed(format!(
                "{provider} response JSON parse failed"
            ))];
            record_outcome_with_secret(
                hooks,
                last_replay,
                provider,
                model,
                Some(status.as_u16()),
                &text,
                secret,
            );
            return ChatPipelineOutcome::Events(events);
        }
    };
    ChatPipelineOutcome::Parsed { status, text, value }
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
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind loopback");
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
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind loopback");
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
}
