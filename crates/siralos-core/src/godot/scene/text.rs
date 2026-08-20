//! Lexical helpers for Godot text resources (R8).
//!
//! Mirrors `packages/core/src/godot/scene/text.ts`. Pure scanning with
//! bounded nesting, honoring quotes and escapes. Nothing evaluates
//! expressions or executes project code.

/// Max nesting depth tracked by the balance scanner (defense in depth).
const MAX_TRACKED_NESTING: usize = 256;

/// Result of scanning for balanced `()`, `[]`, `{}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BalancedScan {
    /// Index one past the last character of the balanced region.
    pub end_index: usize,
    /// True when the region ended at true balance (not EOF/limit).
    pub balanced: bool,
    /// True when an unterminated string was encountered.
    pub unterminated_string: bool,
    /// True when nesting-depth bound was hit.
    pub depth_exceeded: bool,
}

fn closing_for(character: char) -> Option<char> {
    match character {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// Scan `text` from `start` and return the end of the region that keeps
/// every `(`, `[`, `{` balanced, honoring quoted strings and escapes.
#[must_use]
pub fn scan_balanced(text: &str, start: usize) -> BalancedScan {
    let chars: Vec<char> = text.chars().collect();
    let mut index = start;
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut string_quote = '\0';
    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            if ch == '\\' {
                index = index.saturating_add(2);
                continue;
            }
            if ch == string_quote {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            string_quote = ch;
            index += 1;
            continue;
        }
        if ch == '(' || ch == '[' || ch == '{' {
            if stack.len() >= MAX_TRACKED_NESTING {
                return BalancedScan {
                    end_index: byte_index_for_char(text, index),
                    balanced: false,
                    unterminated_string: false,
                    depth_exceeded: true,
                };
            }
            stack.push(ch);
            index += 1;
            continue;
        }
        if let Some(open) = closing_for(ch) {
            let popped = stack.pop();
            if popped != Some(open) {
                return BalancedScan {
                    end_index: byte_index_for_char(text, index),
                    balanced: false,
                    unterminated_string: false,
                    depth_exceeded: false,
                };
            }
            index += 1;
            continue;
        }
        if stack.is_empty() && (ch == ' ' || ch == '\t') {
            return BalancedScan {
                end_index: byte_index_for_char(text, index),
                balanced: true,
                unterminated_string: false,
                depth_exceeded: false,
            };
        }
        index += 1;
    }
    BalancedScan {
        end_index: text.len(),
        balanced: stack.is_empty() && !in_string,
        unterminated_string: in_string,
        depth_exceeded: false,
    }
}

fn byte_index_for_char(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).map(char::len_utf8).sum()
}

/// One header attribute `name=value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderAttribute {
    /// Attribute name.
    pub name: String,
    /// Exact raw value text.
    pub value_text: String,
    /// True when value is a double-quoted string literal.
    pub quoted: bool,
    /// Byte offset hint within the header.
    pub start_index: usize,
}

/// Parse `name=value` attribute pairs from a section header.
#[must_use]
pub fn parse_header_attributes(
    header_text: &str,
    max_attributes: usize,
) -> (Vec<HeaderAttribute>, bool) {
    let mut attributes = Vec::new();
    let mut index = 0usize;
    let chars: Vec<char> = header_text.chars().collect();
    let mut truncated = false;
    while index < chars.len() {
        while index < chars.len()
            && (chars[index] == ' ' || chars[index] == '\t')
        {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        if attributes.len() >= max_attributes {
            truncated = true;
            break;
        }
        let name_start = byte_index_for_char(header_text, index);
        while index < chars.len()
            && chars[index] != '='
            && chars[index] != ' '
            && chars[index] != '\t'
        {
            index += 1;
        }
        if index >= chars.len() || chars[index] != '=' {
            let name = header_text
                [name_start..byte_index_for_char(header_text, index)]
                .trim()
                .to_owned();
            if !name.is_empty() {
                attributes.push(HeaderAttribute {
                    name,
                    value_text: String::new(),
                    quoted: false,
                    start_index: name_start,
                });
            }
            continue;
        }
        let name = header_text
            [name_start..byte_index_for_char(header_text, index)]
            .trim()
            .to_owned();
        index += 1; // consume '='
        while index < chars.len()
            && (chars[index] == ' ' || chars[index] == '\t')
        {
            index += 1;
        }
        if index >= chars.len() {
            attributes.push(HeaderAttribute {
                name,
                value_text: String::new(),
                quoted: false,
                start_index: name_start,
            });
            break;
        }
        let byte_start = byte_index_for_char(header_text, index);
        let quoted = chars[index] == '"';
        let scan = scan_balanced(header_text, byte_start);
        let value_text =
            header_text[byte_start..scan.end_index].trim().to_owned();
        attributes.push(HeaderAttribute {
            name,
            value_text,
            quoted,
            start_index: name_start,
        });
        // advance char index to scan end
        let scanned_chars = header_text[..scan.end_index].chars().count();
        index = scanned_chars;
    }
    (attributes, truncated)
}

/// Split one `key=value` record line at the first `=` outside quotes.
#[must_use]
pub fn split_key_value(line: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = line.chars().collect();
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
        if ch == '=' {
            let key =
                line[..byte_index_for_char(line, index)].trim().to_owned();
            let value_start = byte_index_for_char(line, index + 1);
            return Some((key, value_start));
        }
        index += 1;
    }
    None
}

/// Whether a trimmed line is a whole-line comment (`;` or `#`).
#[must_use]
pub fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with(';') || trimmed.starts_with('#')
}

/// True when `text` has zero net nesting depth outside strings.
#[must_use]
pub fn is_balanced_text(text: &str) -> bool {
    let scan = scan_balanced(text, 0);
    scan.balanced
        && scan.end_index >= text.trim_end().len()
        && !scan.unterminated_string
}

#[cfg(test)]
mod tests {
    use super::{
        is_balanced_text, parse_header_attributes, scan_balanced,
        split_key_value,
    };

    #[test]
    fn scan_balanced_respects_strings() {
        let s = "ExtResource(\"1\") groups=[\"a\",\"b\"]";
        let scan = scan_balanced(s, 0);
        // ExtResource("1") ends before space
        assert!(scan.balanced);
    }

    #[test]
    fn balanced_text_detection() {
        assert!(is_balanced_text("[1, 2, 3]"));
        assert!(!is_balanced_text("[1, 2"));
    }

    #[test]
    fn header_attributes_parse() {
        let (attrs, truncated) =
            parse_header_attributes("name=\"Player\" type=\"Node\"", 10);
        assert!(!truncated);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].name, "name");
    }

    #[test]
    fn split_key_value_basic() {
        let kv = split_key_value("name = \"hello\"").expect("split");
        assert_eq!(kv.0, "name");
    }
}
