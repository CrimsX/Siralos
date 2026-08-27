//! Conservative Godot Variant value parser (R8).
//!
//! Mirrors `packages/core/src/godot/scene/variant.ts`.
//! Bounded, never evaluates expressions.

use super::limits::GODOT_SCENE_LIMITS;
use super::models::{DictionaryEntry, GodotRawValue, GodotVariantValue};
use super::text::scan_balanced;

/// Result of parsing one Variant value.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantParseResult {
    /// Parsed value.
    pub value: GodotVariantValue,
    /// True when a bound stopped full interpretation.
    pub truncated: bool,
}

/// Parse one Godot Variant value from its exact raw text.
#[must_use]
pub fn parse_godot_variant(raw: &str) -> VariantParseResult {
    parse_variant(raw.trim(), 0)
}

fn parse_variant(text: &str, depth: usize) -> VariantParseResult {
    if text.is_empty() {
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "unknown".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if depth > GODOT_SCENE_LIMITS.max_variant_depth {
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "unknown".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: true,
        };
    }
    if text == "null" {
        return VariantParseResult {
            value: GodotVariantValue::Null,
            truncated: false,
        };
    }
    if text == "true" {
        return VariantParseResult {
            value: GodotVariantValue::Boolean(true),
            truncated: false,
        };
    }
    if text == "false" {
        return VariantParseResult {
            value: GodotVariantValue::Boolean(false),
            truncated: false,
        };
    }
    if is_integer_text(text) {
        if let Ok(n) = text.parse::<i64>() {
            return VariantParseResult {
                value: GodotVariantValue::Integer(n),
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "unknown".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if is_number_text(text) {
        if let Ok(n) = text.parse::<f64>() {
            if n.is_finite() {
                return VariantParseResult {
                    value: GodotVariantValue::Float(n),
                    truncated: false,
                };
            }
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "unknown".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if text.starts_with('"') {
        if let Some(v) = parse_quoted_string(text) {
            return VariantParseResult {
                value: GodotVariantValue::String(v),
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "string".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if let Some(stripped) = text.strip_prefix('&') {
        let rest = stripped.trim();
        if rest.starts_with('"') {
            if let Some(v) = parse_quoted_string(rest) {
                return VariantParseResult {
                    value: GodotVariantValue::StringName(v),
                    truncated: false,
                };
            }
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: "StringName".to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if text.starts_with('[') && text.ends_with(']') {
        return parse_array(&text[1..text.len() - 1], depth + 1);
    }
    if text.starts_with('{') && text.ends_with('}') {
        return parse_dictionary(&text[1..text.len() - 1], depth + 1);
    }
    if let Some(type_name) = constructor_type_name(text) {
        return parse_constructor(text, &type_name, depth);
    }
    VariantParseResult {
        value: GodotVariantValue::Opaque {
            type_name: "unknown".to_owned(),
            raw: bounded_raw(text),
        },
        truncated: false,
    }
}

fn is_integer_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes[0] == b'+' || bytes[0] == b'-' {
        if bytes.len() == 1 {
            return false;
        }
        i = 1;
    }
    bytes[i..].iter().all(|b| b.is_ascii_digit())
}

fn is_number_text(text: &str) -> bool {
    // Matches NUMBER_PATTERN: optional sign, then either dotted or exponent form.
    // Simplified: must contain '.' or 'e'/'E' and parse as finite f64, and not be pure integer.
    if !text.bytes().any(|b| b == b'.' || b == b'e' || b == b'E') {
        return false;
    }
    if let Ok(n) = text.parse::<f64>() {
        if n.is_finite() {
            // Must look like a number (sign, digits, dot, exponent)
            let body = if text.starts_with('+') || text.starts_with('-') {
                &text[1..]
            } else {
                text
            };
            if body.is_empty() {
                return false;
            }
            // at least one digit somewhere
            return body.bytes().any(|b| b.is_ascii_digit());
        }
    }
    false
}

fn constructor_type_name(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let prefix = text[..open].trim();
    if prefix.is_empty() {
        return None;
    }
    if prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(prefix.to_owned());
    }
    None
}

fn parse_constructor(
    text: &str,
    type_name: &str,
    depth: usize,
) -> VariantParseResult {
    let open_index = text.find('(').unwrap_or(0);
    let scan = scan_balanced(text, open_index);
    if !scan.balanced || scan.end_index != text.len() {
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: type_name.to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    let inner = text[open_index + 1..text.len() - 1].trim().to_owned();
    let args = split_top_level_arguments(&inner);
    if type_name == "ExtResource" || type_name == "SubResource" {
        let id = args.first().and_then(|a| unquote(a));
        if id.as_ref().is_none_or(|s| s.is_empty()) {
            return VariantParseResult {
                value: GodotVariantValue::Opaque {
                    type_name: type_name.to_owned(),
                    raw: bounded_raw(text),
                },
                truncated: false,
            };
        }
        let id = id.unwrap();
        return VariantParseResult {
            value: if type_name == "ExtResource" {
                GodotVariantValue::ExtResource(id)
            } else {
                GodotVariantValue::SubResource(id)
            },
            truncated: false,
        };
    }
    if type_name == "NodePath" {
        let path = args.first().and_then(|a| unquote(a));
        if let Some(p) = path {
            return VariantParseResult {
                value: GodotVariantValue::NodePath(p),
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: type_name.to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if type_name == "Resource" {
        let uid = args.first().and_then(|a| unquote(a));
        if uid.is_none() {
            return VariantParseResult {
                value: GodotVariantValue::Opaque {
                    type_name: type_name.to_owned(),
                    raw: bounded_raw(text),
                },
                truncated: false,
            };
        }
        let uid = uid.unwrap();
        let second = args.get(1).and_then(|a| unquote(a));
        let type_opt = second.filter(|s| !s.is_empty());
        if uid.starts_with("uid://") {
            return VariantParseResult {
                value: GodotVariantValue::Resource {
                    uid: Some(uid),
                    path: None,
                    type_name: type_opt,
                },
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Resource {
                uid: None,
                path: Some(uid),
                type_name: type_opt,
            },
            truncated: false,
        };
    }
    if type_name == "Color" {
        if let Some(components) = parse_number_components(&args, 8) {
            return VariantParseResult {
                value: GodotVariantValue::Color(components),
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: type_name.to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if is_vector_type(type_name) {
        if let Some(components) = parse_number_components(
            &args,
            GODOT_SCENE_LIMITS.max_vector_components,
        ) {
            return VariantParseResult {
                value: GodotVariantValue::Vector {
                    type_name: type_name.to_owned(),
                    components,
                },
                truncated: false,
            };
        }
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: type_name.to_owned(),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if type_name == "Object" {
        let inner_type = args.first().and_then(|a| unquote(a));
        return VariantParseResult {
            value: GodotVariantValue::Opaque {
                type_name: inner_type.unwrap_or_else(|| "Object".to_owned()),
                raw: bounded_raw(text),
            },
            truncated: false,
        };
    }
    if type_name.starts_with("Packed") {
        let mut items = Vec::new();
        let mut truncated = false;
        for arg in &args {
            if items.len() >= GODOT_SCENE_LIMITS.max_array_items {
                truncated = true;
                break;
            }
            items.push(parse_variant(arg, depth + 1).value);
        }
        return VariantParseResult {
            value: GodotVariantValue::PackedArray {
                type_name: type_name.to_owned(),
                items,
            },
            truncated,
        };
    }
    VariantParseResult {
        value: GodotVariantValue::Opaque {
            type_name: type_name.to_owned(),
            raw: bounded_raw(text),
        },
        truncated: false,
    }
}

fn parse_array(inner: &str, depth: usize) -> VariantParseResult {
    let mut items = Vec::new();
    let mut truncated = false;
    for item in split_top_level_arguments(inner) {
        if items.len() >= GODOT_SCENE_LIMITS.max_array_items {
            truncated = true;
            break;
        }
        let parsed = parse_variant(&item, depth);
        items.push(parsed.value);
        if parsed.truncated {
            truncated = true;
        }
    }
    VariantParseResult { value: GodotVariantValue::Array(items), truncated }
}

fn parse_dictionary(inner: &str, depth: usize) -> VariantParseResult {
    let mut entries = Vec::new();
    let mut truncated = false;
    for pair in split_top_level_pairs(inner) {
        if entries.len() >= GODOT_SCENE_LIMITS.max_dictionary_entries {
            truncated = true;
            break;
        }
        let pk = parse_variant(&pair.0, depth);
        let pv = parse_variant(&pair.1, depth);
        entries.push(DictionaryEntry {
            key: Box::new(pk.value),
            value: Box::new(pv.value),
        });
        if pk.truncated || pv.truncated {
            truncated = true;
        }
    }
    VariantParseResult {
        value: GodotVariantValue::Dictionary(entries),
        truncated,
    }
}

/// Split comma-separated top-level arguments (respecting strings and nested brackets).
#[must_use]
pub fn split_top_level_arguments(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let mut depth: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut quote = '\0';
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            if ch == '\\' {
                index = index.saturating_add(2);
                continue;
            }
            if ch == quote {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
            index += 1;
            continue;
        }
        if ch == '(' || ch == '[' || ch == '{' {
            depth.push(ch);
            index += 1;
            continue;
        }
        if ch == ')' || ch == ']' || ch == '}' {
            depth.pop();
            index += 1;
            continue;
        }
        if ch == ',' && depth.is_empty() {
            let part =
                text[byte_idx(text, start)..byte_idx(text, index)].trim();
            if !part.is_empty() {
                parts.push(part.to_owned());
            }
            start = index + 1;
        }
        index += 1;
    }
    let tail = text[byte_idx(text, start)..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_owned());
    }
    parts
}

fn split_top_level_pairs(inner: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        while index < chars.len()
            && (chars[index] == ' ' || chars[index] == ',')
        {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let key_start = index;
        let mut in_string = false;
        let mut quote = '\0';
        let mut key_end: Option<usize> = None;
        while index < chars.len() {
            let ch = chars[index];
            if in_string {
                if ch == '\\' {
                    index = index.saturating_add(2);
                    continue;
                }
                if ch == quote {
                    in_string = false;
                }
                index += 1;
                continue;
            }
            if ch == '"' || ch == '\'' {
                in_string = true;
                quote = ch;
                index += 1;
                continue;
            }
            if ch == ':' {
                key_end = Some(index);
                break;
            }
            index += 1;
        }
        let Some(kend) = key_end else { break };
        let key = inner[byte_idx(inner, key_start)..byte_idx(inner, kend)]
            .trim()
            .to_owned();
        index = kend + 1;
        let value_start = index;
        let mut depth: Vec<char> = Vec::new();
        in_string = false;
        let mut value_end = chars.len();
        while index < chars.len() {
            let ch = chars[index];
            if in_string {
                if ch == '\\' {
                    index = index.saturating_add(2);
                    continue;
                }
                if ch == quote {
                    in_string = false;
                }
                index += 1;
                continue;
            }
            if ch == '"' || ch == '\'' {
                in_string = true;
                quote = ch;
                index += 1;
                continue;
            }
            if ch == '(' || ch == '[' || ch == '{' {
                depth.push(ch);
                index += 1;
                continue;
            }
            if ch == ')' || ch == ']' || ch == '}' {
                depth.pop();
                index += 1;
                continue;
            }
            if ch == ',' && depth.is_empty() {
                value_end = index;
                break;
            }
            index += 1;
        }
        let value = inner
            [byte_idx(inner, value_start)..byte_idx(inner, value_end)]
            .trim()
            .to_owned();
        pairs.push((key, value));
        index = value_end + 1;
    }
    pairs
}

fn parse_number_components(args: &[String], max: usize) -> Option<Vec<f64>> {
    if args.is_empty() || args.len() > max {
        return None;
    }
    let mut comps = Vec::with_capacity(args.len());
    for a in args {
        let n: f64 = a.parse().ok()?;
        if !n.is_finite() {
            return None;
        }
        comps.push(n);
    }
    Some(comps)
}

fn is_vector_type(name: &str) -> bool {
    matches!(
        name,
        "Vector2"
            | "Vector2i"
            | "Vector3"
            | "Vector3i"
            | "Vector4"
            | "Vector4i"
            | "Rect2"
            | "Rect2i"
            | "Transform2D"
            | "Transform3D"
            | "Basis"
            | "Quaternion"
            | "Plane"
            | "AABB"
            | "Projection"
    )
}

fn unquote(text: &str) -> Option<String> {
    let t = text.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return parse_quoted_string(t);
    }
    if t.is_empty() { None } else { Some(t.to_owned()) }
}

/// Parse a double-quoted string with Godot escapes.
#[must_use]
pub fn parse_quoted_string(raw: &str) -> Option<String> {
    if !raw.starts_with('"') {
        return None;
    }
    let mut value = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut index = 1usize;
    let mut closed = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            closed = true;
            index += 1;
            break;
        }
        if ch == '\\' {
            let next = chars.get(index + 1).copied();
            match next {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('b') => value.push('\x08'),
                Some('f') => value.push('\x0c'),
                Some('u') => {
                    let digits: String =
                        chars[index + 2..].iter().take(4).collect();
                    if digits.len() == 4
                        && digits.chars().all(|c| c.is_ascii_hexdigit())
                    {
                        if let Ok(cp) = u32::from_str_radix(&digits, 16) {
                            if let Some(c) = char::from_u32(cp) {
                                value.push(c);
                                index += 6;
                                continue;
                            }
                        }
                    }
                    value.push('\\');
                    value.push('u');
                }
                Some('U') => {
                    value.push('\\');
                    value.push('U');
                }
                Some('"') => value.push('"'),
                Some('\\') => value.push('\\'),
                Some('\'') => value.push('\''),
                Some('/') => value.push('/'),
                Some(c) => {
                    value.push('\\');
                    value.push(c);
                }
                None => break,
            }
            index += 2;
            continue;
        }
        value.push(ch);
        index += 1;
    }
    if !closed {
        return None;
    }
    let rest_start = byte_idx(raw, index);
    if !raw[rest_start..].trim().is_empty() {
        return None;
    }
    Some(value)
}

fn byte_idx(text: &str, char_idx: usize) -> usize {
    text.chars().take(char_idx).map(char::len_utf8).sum()
}

fn bounded_raw(text: &str) -> GodotRawValue {
    if text.len() <= GODOT_SCENE_LIMITS.max_raw_value_length {
        GodotRawValue { text: text.to_owned(), truncated: false }
    } else {
        GodotRawValue {
            text: text[..GODOT_SCENE_LIMITS.max_raw_value_length].to_owned(),
            truncated: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_godot_variant, parse_quoted_string, split_top_level_arguments,
    };

    #[test]
    fn parses_null_and_bool() {
        assert!(matches!(
            parse_godot_variant("null").value,
            super::super::models::GodotVariantValue::Null
        ));
        assert!(matches!(
            parse_godot_variant("true").value,
            super::super::models::GodotVariantValue::Boolean(true)
        ));
    }

    #[test]
    fn parses_string() {
        let r = parse_godot_variant("\"hello\"");
        assert!(
            matches!(r.value, super::super::models::GodotVariantValue::String(ref s) if s == "hello")
        );
    }

    #[test]
    fn quoted_string_roundtrip() {
        assert_eq!(parse_quoted_string("\"hi\""), Some("hi".to_owned()));
        assert_eq!(
            parse_quoted_string("\"a\\n b\""),
            Some("a\n b".to_owned())
        );
    }

    #[test]
    fn splits_args() {
        let parts = split_top_level_arguments("a, b, [1, 2], c");
        assert_eq!(parts, vec!["a", "b", "[1, 2]", "c"]);
    }
}
