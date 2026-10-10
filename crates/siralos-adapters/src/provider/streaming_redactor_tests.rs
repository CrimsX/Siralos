//! Tests for streaming and complete-body credential redaction.

use super::{
    MAX_RESPONSE_BYTES, StreamingSecretRedactor, bounded_body_text_from_bytes,
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
    let first =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: first.to_owned(),
        }));
    let second =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: second.to_owned(),
        }));
    assert_eq!(text_delta_text(first), "");
    assert_eq!(text_delta_text(second), "[REDACTED]");
    assert_eq!(redactor.finish(), None);
}

#[test]
fn masks_a_credential_split_across_short_stream_prefixes() {
    let mut redactor = StreamingSecretRedactor::new(Some("split-secret"));
    let first =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: "s".to_owned(),
        }));
    let second = redactor.redact_event(ProviderEvent::Event(
        ModelEvent::ReasoningDelta { text: "pl".to_owned() },
    ));
    let third =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: "it-secret".to_owned(),
        }));
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
    let percent = super::redact_sensitive("request %20 body", Some(secret));
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
        super::replace_variant_matches("abcabcab", &variants, "[REDACTED]",),
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
    let mut redactor = StreamingSecretRedactor::new(Some(ambiguous_secret));
    let first =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: "++%2B".to_owned(),
        }));
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
    assert_eq!(super::redact_sensitive("%2B++a", Some("+  a")), "[REDACTED]");

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
    let first =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: encoded_prefix,
        }));
    let second =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: "é".to_owned(),
        }));
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
    for (first, second) in [(r"ab\u0063", "defghij"), ("ab%63", "defghij")] {
        assert_split_secret_redacted("abcdefghij", first, second);
    }
}

#[test]
fn holds_incomplete_encoded_prefixes_across_stream_chunks() {
    for (first, second) in [(r"ab\u00", r"63defghij"), ("ab%6", "3defghij")] {
        assert_split_secret_redacted("abcdefghij", first, second);
    }
}

#[test]
fn finish_redacts_ambiguous_raw_and_encoded_eof_prefixes() {
    for suffix in ["ab", r"ab\u0063", "ab%63", r"ab\u00", "ab%6"] {
        let mut redactor = StreamingSecretRedactor::new(Some("abcdefghij"));
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
    let event =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: "ordinary [".to_owned(),
        }));
    assert_eq!(text_delta_text(event), "ordinary ");
    assert_eq!(redactor.finish(), Some("[REDACTED:1]".to_owned()));
}

#[test]
fn finish_does_not_split_a_marker_at_a_held_raw_prefix() {
    let secret = "]x";
    let mut redactor = StreamingSecretRedactor::new(Some(secret));
    let event =
        redactor.redact_event(ProviderEvent::Event(ModelEvent::TextDelta {
            text: secret.to_owned(),
        }));
    let emitted = text_delta_text(event);
    let finish = redactor.finish();
    let output = format!("{emitted}{}", finish.as_deref().unwrap_or_default());
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
    let event = redactor
        .redact_event(ProviderEvent::Event(ModelEvent::TextDelta { text }));
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
