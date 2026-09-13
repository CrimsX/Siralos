//! Provider-safe tool names.
//!
//! A live request failed with `invalid request: tool names can only contain
//! certain characters (A-Za-z0-9_) and can't begin with a digit` -- the
//! capability ids Siralos registers are dotted (`workspace.read`), and the
//! provider validator rejects the dot.
//!
//! The capability id stays the REAL name everywhere inside Siralos: it is what
//! the Tool Registry resolves, what the approval surface shows, and what the
//! differential corpus pins. Only the provider request boundary translates --
//! outbound definitions and replayed assistant calls carry the alias, and every
//! inbound call is mapped back to the real name before the Application sees it.

use siralos_core::provider::{ModelEvent, ProviderEvent};

/// The longest tool name the provider boundary emits.
pub const MAX_PROVIDER_TOOL_NAME_BYTES: usize = 64;

/// Hex digits appended when a name must be truncated.
const DIGEST_BYTES: usize = 8;

/// Sanitize one tool name into the provider's accepted shape: ASCII
/// alphanumerics and `_` only, never starting with a digit, at most
/// [@MAX_PROVIDER_TOOL_NAME_BYTES@] bytes.
///
/// Deterministic and pure: the same real name always yields the same alias, so
/// a replayed call and its definition cannot drift apart.
#[must_use]
pub fn provider_tool_name(real: &str) -> String {
    let mut out = String::with_capacity(real.len());
    for ch in real.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("tool");
    }
    if out.as_bytes()[0].is_ascii_digit() {
        out.insert_str(0, "t_");
    }
    if out.len() > MAX_PROVIDER_TOOL_NAME_BYTES {
        let digest = short_digest(real);
        out.truncate(MAX_PROVIDER_TOOL_NAME_BYTES - DIGEST_BYTES - 1);
        out.push('_');
        out.push_str(&digest);
    }
    out
}

/// FNV-1a over the real name, 8 hex digits: stable across runs and platforms.
fn short_digest(real: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in real.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash & 0xffff_ffff) as u32)
}

/// The alias set for one provider request: real name to provider-visible
/// alias, with a reverse lookup that stays total.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolNames {
    pairs: Vec<(String, String)>,
}

impl ToolNames {
    /// Build the map from the request's tool definitions, in order.
    ///
    /// Two real names that sanitize to the same alias (`workspace.read` and
    /// `workspace_read`) are disambiguated by a deterministic numeric suffix,
    /// so the reverse lookup never guesses.
    #[must_use]
    pub fn new<'a>(reals: impl IntoIterator<Item = &'a str>) -> Self {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for real in reals {
            let base = provider_tool_name(real);
            let mut alias = base.clone();
            let mut suffix = 2usize;
            while pairs
                .iter()
                .any(|(taken, owner)| taken == &alias && owner != real)
            {
                alias = format!("{base}_{suffix}");
                suffix += 1;
            }
            pairs.push((alias, real.to_owned()));
        }
        Self { pairs }
    }

    /// The alias the provider sees for `real`. An unlisted name is sanitized
    /// on the fly (never echoed raw).
    #[must_use]
    pub fn alias(&self, real: &str) -> String {
        self.pairs.iter().find(|(_, owner)| owner == real).map_or_else(
            || provider_tool_name(real),
            |(alias, _)| alias.clone(),
        )
    }

    /// The real name behind an inbound alias, when the map knows it.
    #[must_use]
    pub fn real(&self, alias: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(taken, _)| taken == alias)
            .map(|(_, owner)| owner.as_str())
    }

    /// Translate every inbound tool-call name back to the real one.
    ///
    /// An unknown name passes through unchanged: the Application reports it as
    /// an unknown tool, which is a truthful, recoverable outcome -- unlike the
    /// provider rejecting the whole request.
    #[must_use]
    pub fn restore_events(
        &self,
        events: Vec<ProviderEvent>,
    ) -> Vec<ProviderEvent> {
        events
            .into_iter()
            .map(|event| match event {
                ProviderEvent::Event(ModelEvent::ToolCall {
                    call_id,
                    tool_name,
                    input,
                }) => {
                    let tool_name =
                        self.real(&tool_name).unwrap_or(&tool_name).to_owned();
                    ProviderEvent::Event(ModelEvent::ToolCall {
                        call_id,
                        tool_name,
                        input,
                    })
                }
                other => other,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROVIDER_TOOL_NAME_BYTES, ToolNames, provider_tool_name};

    #[test]
    fn dotted_capability_ids_become_provider_safe_aliases() {
        // The exact live failure: `workspace.read` is rejected by the
        // provider validator, `workspace_read` is accepted.
        assert_eq!(provider_tool_name("workspace.read"), "workspace_read");
        assert_eq!(provider_tool_name("workspace.list"), "workspace_list");
        assert_eq!(provider_tool_name("workspace-search"), "workspace_search");
        assert_eq!(provider_tool_name("mcp:thing/x"), "mcp_thing_x");
        for name in ["workspace.read", "9lives", "", "aaaaaaaaaaaaaaaa"] {
            let alias = provider_tool_name(name);
            assert!(
                alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{alias} must be provider-safe"
            );
            assert!(
                !alias.as_bytes()[0].is_ascii_digit(),
                "{alias} must not begin with a digit"
            );
            assert!(alias.len() <= MAX_PROVIDER_TOOL_NAME_BYTES);
        }
        // Empty and digit-leading names get a safe, readable shell.
        assert_eq!(provider_tool_name(""), "tool");
        assert_eq!(provider_tool_name("9lives"), "t_9lives");
    }

    #[test]
    fn truncation_is_deterministic_and_distinguishes_long_names() {
        let a = format!("{}.read", "segment".repeat(20));
        let b = format!("{}.write", "segment".repeat(20));
        let first = provider_tool_name(&a);
        assert_eq!(first, provider_tool_name(&a), "stable across calls");
        assert_eq!(first.len(), MAX_PROVIDER_TOOL_NAME_BYTES);
        assert_ne!(first, provider_tool_name(&b), "long names stay distinct");
    }

    #[test]
    fn aliases_round_trip_and_collisions_are_disambiguated() {
        let names = ToolNames::new(["workspace.read", "workspace_read"]);
        let dotted = names.alias("workspace.read");
        let underscored = names.alias("workspace_read");
        assert_ne!(
            dotted, underscored,
            "a collision must not merge two tools"
        );
        assert_eq!(names.real(&dotted), Some("workspace.read"));
        assert_eq!(names.real(&underscored), Some("workspace_read"));
        assert_eq!(names.real("never_registered"), None);
        // An unlisted name is still sanitized rather than passed through raw.
        assert_eq!(names.alias("other.tool"), "other_tool");
    }
}
