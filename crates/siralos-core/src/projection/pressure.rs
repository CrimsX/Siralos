//! Host-owned context pressure classification.
//!
//! Inclusive thresholds: normal < 0.70 ≤ warn < 0.85 ≤ auto < 1.00 ≤ hard.
//! For `working_maximum <= 0`, ratio is 1 and state is `hard` for any
//! non-negative estimate — the frozen fail-closed semantics.

/// One pressure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureState {
    /// Below the warn threshold.
    Normal,
    /// At or above 0.70 and below 0.85.
    Warn,
    /// At or above 0.85 and below 1.00.
    Auto,
    /// At or above the working maximum.
    Hard,
}

impl PressureState {
    /// Stable wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Auto => "auto",
            Self::Hard => "hard",
        }
    }
}

/// Pressure limits (informational for now — values are the frozen constants).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureLimits {
    /// Warn threshold ratio.
    pub warn_ratio: f64,
    /// Auto threshold ratio.
    pub auto_ratio: f64,
    /// Hard threshold ratio.
    pub hard_ratio: f64,
}

impl Default for PressureLimits {
    fn default() -> Self {
        Self { warn_ratio: 0.70, auto_ratio: 0.85, hard_ratio: 1.0 }
    }
}

/// Classification result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextPressure {
    /// Classified state.
    pub state: PressureState,
    /// Input estimate.
    pub estimated_tokens: usize,
    /// Working maximum used for classification.
    pub working_maximum: i64,
    /// Ratio (1.0 when `working_maximum <= 0`).
    pub ratio: f64,
}

/// Classify pressure with integer-safe threshold comparisons.
///
/// Thresholds are inclusive: exactly at a limit enters that state.
/// Uses integer arithmetic `estimated * 100 >= working * threshold*100`
/// to avoid floating-point approximation for the decision; the reported
/// `ratio` remains the floating-point estimate for observability.
pub fn classify_pressure(
    estimated_tokens: usize,
    working_maximum: i64,
    limits: PressureLimits,
) -> ContextPressure {
    if working_maximum <= 0 {
        return ContextPressure {
            state: PressureState::Hard,
            estimated_tokens,
            working_maximum,
            ratio: 1.0,
        };
    }
    let working = working_maximum as f64;
    let ratio = estimated_tokens as f64 / working;
    // Integer-safe inclusive comparisons: estimated >= working * ratio.
    // Using multiplication avoids division rounding for the decision.
    let state =
        if ge_ratio(estimated_tokens, working_maximum, limits.hard_ratio) {
            PressureState::Hard
        } else if ge_ratio(
            estimated_tokens,
            working_maximum,
            limits.auto_ratio,
        ) {
            PressureState::Auto
        } else if ge_ratio(
            estimated_tokens,
            working_maximum,
            limits.warn_ratio,
        ) {
            PressureState::Warn
        } else {
            PressureState::Normal
        };
    ContextPressure { state, estimated_tokens, working_maximum, ratio }
}

fn ge_ratio(estimated: usize, working: i64, ratio: f64) -> bool {
    // Compare estimated >= working * ratio without floating approximation
    // for the decision: use `estimated * 100 >= working * ratio*100` with
    // integer math for the known ratios 0.70/0.85/1.00, or fallback to the
    // floating comparison which is exact for these decimal values at the
    // tested magnitudes (working up to 32768, estimated up to a few MB).
    // Keep the floating path for arbitrary limits but make the default
    // path integer-exact.
    let scaled = (ratio * 100.0).round() as i64;
    if scaled == 70 || scaled == 85 || scaled == 100 {
        // estimated >= working * scaled / 100  →  estimated*100 >= working*scaled
        let left = estimated as i64 * 100;
        let right = working * scaled;
        left >= right
    } else {
        (estimated as f64) >= (working as f64 * ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::{PressureLimits, PressureState, classify_pressure};

    #[test]
    fn thresholds() {
        assert_eq!(
            classify_pressure(1000, 10_000, PressureLimits::default()).state,
            PressureState::Normal
        );
        assert_eq!(
            classify_pressure(7000, 10_000, PressureLimits::default()).state,
            PressureState::Warn
        );
        assert_eq!(
            classify_pressure(8500, 10_000, PressureLimits::default()).state,
            PressureState::Auto
        );
        assert_eq!(
            classify_pressure(10_000, 10_000, PressureLimits::default()).state,
            PressureState::Hard
        );
        assert_eq!(
            classify_pressure(11_000, 10_000, PressureLimits::default()).state,
            PressureState::Hard
        );
    }

    #[test]
    fn non_positive_working_maximum_is_hard() {
        let p = classify_pressure(0, 0, PressureLimits::default());
        assert_eq!(p.state, PressureState::Hard);
        assert_eq!(p.ratio, 1.0);
        let p2 = classify_pressure(5, -10, PressureLimits::default());
        assert_eq!(p2.state, PressureState::Hard);
    }

    #[test]
    fn inclusive_boundaries() {
        // Just below warn stays normal; exactly at warn enters warn.
        assert_eq!(
            classify_pressure(6999, 10_000, PressureLimits::default()).state,
            PressureState::Normal
        );
        // Exactly at auto, exactly at hard
        assert_eq!(
            classify_pressure(8499, 10_000, PressureLimits::default()).state,
            PressureState::Warn
        );
        assert_eq!(
            classify_pressure(9999, 10_000, PressureLimits::default()).state,
            PressureState::Auto
        );
    }
}
