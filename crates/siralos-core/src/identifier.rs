//! The shared element-identifier rule (Stage 3R R13.4).
//!
//! One rule is enforced by four validators in two subsystems: the executor's
//! contract ids, reporting ids and milestone-entry ids, and planning's
//! step/touchpoint/constraint/risk ids. Where the rule genuinely differs the
//! author wrote a different body beside these — `is_rule_id` requires an
//! uppercase first byte, `is_milestone_id` caps the length at eight and
//! restricts it to uppercase and digits, and `is_acceptance_id` requires two
//! uppercase bytes — so these four are one rule, and the domain difference
//! lives in the message each validator renders.
//!
//! This module is a leaf: it imports nothing, so it adds no edge between
//! `executor` and `planning`, which the architecture keeps separate.

/// Whether `id` matches `^[A-Za-z][A-Za-z0-9._-]{0,63}$`.
///
/// The first byte must be ASCII alphabetic, and the remaining zero to
/// sixty-three bytes must be ASCII alphanumeric or one of `.`, `_`, `-`.
#[must_use]
pub(crate) fn is_element_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}
