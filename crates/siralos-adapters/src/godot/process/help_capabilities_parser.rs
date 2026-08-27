//! Extracts command capabilities from bounded `--help` output.
//!
//! Complete option tokens are matched exactly against the fixed known
//! set; substrings never match, malformed output never creates false
//! capabilities, and unrecognized options are preserved only as a
//! bounded diagnostic count. Advertised support is not operational
//! support.

use std::collections::BTreeSet;

use siralos_godot::godot::{
    GODOT_KNOWN_OPTIONS, GodotCommandCapabilities,
    empty_godot_command_capabilities,
};

/// Recognized options that are not capabilities (e.g. `--help` itself).
const KNOWN_NON_CAPABILITY_OPTIONS: [&str; 1] = ["--help"];

/// Parsed advertised capabilities with the unknown-option count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotHelpParseResult {
    /// Capabilities advertised by the recognized options.
    pub capabilities: GodotCommandCapabilities,
    /// Count of unrecognized option tokens (bounded diagnostic only).
    pub unknown_option_count: u64,
}

/// Parse every complete `--[a-z][a-z0-9-]*` token left-to-right and set
/// the matching capability flags.
pub fn parse_help_capabilities(help_text: &str) -> GodotHelpParseResult {
    let mut capabilities = empty_godot_command_capabilities();
    let mut seen = BTreeSet::new();
    let mut unknown_option_count: u64 = 0;
    for token in option_tokens(help_text) {
        if !seen.insert(token) {
            continue;
        }
        let known = GODOT_KNOWN_OPTIONS
            .iter()
            .find(|entry| entry.option == token)
            .map(|entry| entry.capability);
        match known {
            Some(capability) => capability.apply(&mut capabilities, true),
            None => {
                if !KNOWN_NON_CAPABILITY_OPTIONS.contains(&token) {
                    unknown_option_count += 1;
                }
            }
        }
    }
    GodotHelpParseResult { capabilities, unknown_option_count }
}

fn option_tokens(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index + 2 <= bytes.len() {
        let starts_option = bytes[index] == b'-'
            && bytes[index + 1] == b'-'
            && bytes[index + 2].is_ascii_lowercase();
        if !starts_option {
            index += 1;
            continue;
        }
        let mut end = index + 2;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase()
                || bytes[end].is_ascii_digit()
                || bytes[end] == b'-')
        {
            end += 1;
        }
        tokens.push(&text[index..end]);
        index = end;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::parse_help_capabilities;

    #[test]
    fn recognizes_complete_known_options_only() {
        let parsed = parse_help_capabilities(
            "Usage: godot [options]\n  --editor       Edit\n  \
             --headless     Headless\n  --path <dir>   Path\n",
        );
        assert!(parsed.capabilities.editor);
        assert!(parsed.capabilities.headless);
        assert!(parsed.capabilities.project_path);
        assert!(!parsed.capabilities.script);
        assert_eq!(parsed.unknown_option_count, 0);
    }

    #[test]
    fn substrings_never_match() {
        let parsed = parse_help_capabilities("--editorial --headlessish");
        assert!(!parsed.capabilities.editor);
        assert!(!parsed.capabilities.headless);
        assert_eq!(parsed.unknown_option_count, 2);
    }

    #[test]
    fn duplicates_are_counted_once_and_help_is_not_unknown() {
        let parsed = parse_help_capabilities(
            "--help --editor --editor --mystery --mystery",
        );
        assert!(parsed.capabilities.editor);
        assert_eq!(parsed.unknown_option_count, 1);
    }

    #[test]
    fn uppercase_and_malformed_tokens_never_match() {
        let parsed = parse_help_capabilities("--EDITOR --edi tor -x");
        assert!(!parsed.capabilities.editor);
        assert_eq!(parsed.unknown_option_count, 1);
    }

    #[test]
    fn digits_and_dashes_continue_tokens() {
        let parsed = parse_help_capabilities("--lsp-port 6006");
        assert!(parsed.capabilities.lsp);
        assert_eq!(parsed.unknown_option_count, 0);
    }
}
