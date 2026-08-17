//! Projection capacity — the authoritative working budget.
//!
//! Only `working_maximum` drives projection arithmetic. `advertised_maximum`
//! and `verified_maximum` are informational and always `None` today.
//! `max_output_tokens` is carried but does not enter projection math.

/// Host-owned projection capacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCapacity {
    /// Model-advertised window (informational); always `None`.
    pub advertised_maximum: Option<u64>,
    /// Host-verified window (informational); always `None`.
    pub verified_maximum: Option<u64>,
    /// Authoritative working budget.
    pub working_maximum: i64,
    /// Reserved output budget (informational, 4096 today).
    pub max_output_tokens: Option<u64>,
}

/// Default working maximum: 32,768 estimated tokens.
pub const DEFAULT_WORKING_MAXIMUM: i64 = 32_768;

/// Default max output tokens: 4,096.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u64 = 4_096;

impl Default for ContextCapacity {
    fn default() -> Self {
        Self {
            advertised_maximum: None,
            verified_maximum: None,
            working_maximum: DEFAULT_WORKING_MAXIMUM,
            max_output_tokens: Some(DEFAULT_MAX_OUTPUT_TOKENS),
        }
    }
}

impl ContextCapacity {
    /// Create a capacity with the given working maximum.
    pub fn with_working_maximum(working_maximum: i64) -> Self {
        Self { working_maximum, ..Self::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContextCapacity, DEFAULT_MAX_OUTPUT_TOKENS, DEFAULT_WORKING_MAXIMUM,
    };

    #[test]
    fn default_capacity_matches_oracle() {
        let cap = ContextCapacity::default();
        assert_eq!(cap.working_maximum, DEFAULT_WORKING_MAXIMUM);
        assert_eq!(cap.max_output_tokens, Some(DEFAULT_MAX_OUTPUT_TOKENS));
        assert_eq!(cap.advertised_maximum, None);
        assert_eq!(cap.verified_maximum, None);
    }
}
