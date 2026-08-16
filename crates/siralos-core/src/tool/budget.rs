//! Tool-round budget normalization (frozen R7.2 semantics).

/// Default maximum tool rounds when the caller supplies no value.
pub const DEFAULT_MAX_TOOL_ROUNDS: u32 = 8;

/// Hard maximum tool rounds after normalization.
pub const MAX_TOOL_ROUNDS: u32 = 32;

/// Normalized maximum tool-round count.
///
/// The TypeScript normalization is `clamp(floor(v), 0, 32)` with
/// `undefined` and any non-finite number mapping to 8. Rust models the
/// boundary as `Option<f64>` so non-finite values can be preserved at
/// harness boundaries instead of being silently lost in an integer
/// conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoundBudget(u32);

impl RoundBudget {
    /// Normalize an optional JavaScript-style number.
    ///
    /// `None` represents a missing value; `Some(non-finite)` represents
    /// `NaN` or an infinity and normalizes to the default, exactly like
    /// the reference.
    pub fn normalize(value: Option<f64>) -> Self {
        let normalized = match value {
            None => DEFAULT_MAX_TOOL_ROUNDS,
            Some(value) if !value.is_finite() => DEFAULT_MAX_TOOL_ROUNDS,
            Some(value) => {
                let floored = value.floor();
                if floored < 0.0 {
                    0
                } else if floored > f64::from(MAX_TOOL_ROUNDS) {
                    MAX_TOOL_ROUNDS
                } else {
                    floored as u32
                }
            }
        };
        Self(normalized)
    }

    /// The normalized inclusive maximum.
    pub fn get(&self) -> u32 {
        self.0
    }

    /// The exact over-budget failure message for this normalized bound.
    pub fn cap_message(&self) -> String {
        format!(
            "Siralos reached the maximum of {} tool rounds; the requested tool round was not executed.",
            self.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_TOOL_ROUNDS, MAX_TOOL_ROUNDS, RoundBudget};

    #[test]
    fn normalizes_missing_nonfinite_and_numeric_boundaries() {
        assert_eq!(
            RoundBudget::normalize(None).get(),
            DEFAULT_MAX_TOOL_ROUNDS
        );
        assert_eq!(
            RoundBudget::normalize(Some(f64::NAN)).get(),
            DEFAULT_MAX_TOOL_ROUNDS
        );
        assert_eq!(
            RoundBudget::normalize(Some(f64::INFINITY)).get(),
            DEFAULT_MAX_TOOL_ROUNDS
        );
        assert_eq!(
            RoundBudget::normalize(Some(f64::NEG_INFINITY)).get(),
            DEFAULT_MAX_TOOL_ROUNDS
        );
        assert_eq!(RoundBudget::normalize(Some(-5.0)).get(), 0);
        assert_eq!(RoundBudget::normalize(Some(-0.5)).get(), 0);
        assert_eq!(RoundBudget::normalize(Some(0.0)).get(), 0);
        assert_eq!(RoundBudget::normalize(Some(1.9)).get(), 1);
        assert_eq!(RoundBudget::normalize(Some(8.0)).get(), 8);
        assert_eq!(RoundBudget::normalize(Some(32.0)).get(), 32);
        assert_eq!(RoundBudget::normalize(Some(32.9)).get(), MAX_TOOL_ROUNDS);
        assert_eq!(RoundBudget::normalize(Some(100.0)).get(), MAX_TOOL_ROUNDS);
    }

    #[test]
    fn cap_message_uses_the_normalized_integer() {
        let budget = RoundBudget::normalize(Some(40.0));
        assert_eq!(
            budget.cap_message(),
            "Siralos reached the maximum of 32 tool rounds; the requested tool round was not executed."
        );
    }
}
