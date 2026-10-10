//! Exact-credential redaction and sanitized output projection.
//!
//! Owns the sensitive-variant matcher, JSON and percent encoded matching,
//! the streaming chunk-boundary redactor, and the endpoint projections
//! that keep resolved credentials out of provider output.

use super::credential::HostCredential;
use siralos_core::provider::{ModelEvent, ProviderEvent, ToolCallInput};

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

pub(super) fn replace_variant_matches(
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
