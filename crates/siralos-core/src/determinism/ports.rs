//! Explicit nondeterminism boundaries (Stage 3 — Deterministic Execution
//! & Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/context.ts`. Authoritative host
//! decisions must not silently depend on ambient wall-clock time,
//! randomness, process environment, filesystem enumeration order,
//! locale/timezone, or concurrency completion order. Adapters own
//! external nondeterminism; core consumes it through these explicit
//! ports. "Controlled time" never means frozen production time —
//! production uses the explicit system clock; tests use a fixed clock.

use std::cmp::Ordering;

/// Explicit clock port. Production binds the system wall clock; tests
/// bind a fixed clock.
pub trait Clock {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> u64;
}

/// System wall clock (adapter boundary: real time enters only here).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Fixed clock for tests and deterministic policy evaluation.
#[derive(Debug)]
pub struct FixedClock {
    current_ms: std::cell::Cell<u64>,
}

impl FixedClock {
    /// Bind a fixed initial time.
    #[must_use]
    pub fn new(initial_ms: u64) -> Self {
        Self { current_ms: core::cell::Cell::new(initial_ms) }
    }

    /// Advance by a non-negative delta.
    pub fn advance(&self, delta_ms: u64) {
        self.current_ms.set(self.current_ms.get().saturating_add(delta_ms));
    }

    /// Set an absolute time.
    pub fn set(&self, ms: u64) {
        self.current_ms.set(ms);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.current_ms.get()
    }
}

/// Random source port. Used only where randomness is genuinely part of
/// the design; most host policy decisions need none. The 128-bit token
/// is identity generation, never a decision input.
pub trait RandomSource {
    /// Next uniform value in `[0, 1)`.
    fn next_f64(&mut self) -> f64;
    /// Next integer in `[0, bound)`; `None` when the bound is zero or
    /// oversized.
    fn next_int(&mut self, bound: u64) -> Option<u64>;
    /// 128-bit lowercase hex token.
    fn next_token(&mut self) -> String;
}

/// Deterministic seeded PRNG (mulberry32), bit-exact with the oracle:
/// `state = state + 0x6D2B79F5`, then `Math.imul(t ^ (t >>> 15), t | 1)`
/// followed by `t ^= t + Math.imul(t ^ (t >>> 7), t | 61)`, normalized
/// by `2^32`.
#[derive(Debug, Clone)]
pub struct SeededRandomSource {
    state: u32,
}

impl SeededRandomSource {
    /// Seed the generator; a zero seed is replaced with the golden-ratio
    /// constant, mirroring the oracle.
    #[must_use]
    pub fn new(seed: u32) -> Self {
        let state = if seed == 0 { 0x9e37_79b9 } else { seed };
        Self { state }
    }

    fn draw_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        // Math.imul(t ^ (t >>> 15), t | 1)
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        // t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    }
}

impl RandomSource for SeededRandomSource {
    fn next_f64(&mut self) -> f64 {
        f64::from(self.draw_u32()) / 4294967296.0
    }

    fn next_int(&mut self, bound: u64) -> Option<u64> {
        if bound == 0 || bound > i64::MAX as u64 {
            return None;
        }
        Some((self.next_f64() * bound as f64).floor() as u64)
    }

    fn next_token(&mut self) -> String {
        let mut token = String::with_capacity(32);
        for _ in 0..4 {
            let word = (self.next_f64() * 4294967295.0).floor() as u32;
            token.push_str(&format!("{word:08x}"));
        }
        token
    }
}

/// Canonical code-unit comparison (locale-independent; byte order on
/// UTF-8 equals code-unit order).
#[must_use]
pub fn compare_code_units(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}

/// Stable sort by a string key: Rust `sort_by` is stable, which
/// realizes the oracle's index tie-break exactly (equal keys keep
/// insertion order).
pub fn stable_sort_by_key<T: Clone>(
    items: &[T],
    key: impl Fn(&T) -> String,
) -> Vec<T> {
    let mut keyed: Vec<(String, T)> =
        items.iter().map(|item| (key(item), item.clone())).collect();
    keyed.sort_by(|left, right| compare_code_units(&left.0, &right.0));
    keyed.into_iter().map(|(_, item)| item).collect()
}

/// Canonical ordering of an unordered id-keyed result set
/// (order-insensitive).
pub trait IdKeyed {
    /// The ordering key.
    fn id_key(&self) -> String;
}

/// Normalize an id-keyed result list into canonical order.
#[must_use]
pub fn normalize_keyed_results<T: IdKeyed + Clone>(results: &[T]) -> Vec<T> {
    stable_sort_by_key(results, |entry| entry.id_key())
}

#[cfg(test)]
mod tests {
    use super::{
        Clock, FixedClock, RandomSource, SeededRandomSource,
        compare_code_units,
    };
    use std::cmp::Ordering;

    #[test]
    fn mulberry32_seed_42_first_draw_matches_the_oracle() {
        let mut rng = SeededRandomSource::new(42);
        let first = rng.next_f64();
        assert!(
            (first - 0.6011037519201636).abs() < 1e-15,
            "first draw {first} does not match oracle"
        );
    }

    #[test]
    fn mulberry32_second_draw_matches_the_oracle() {
        let mut rng = SeededRandomSource::new(42);
        rng.next_f64(); // consume first draw
        let second = rng.next_f64();
        assert!(
            (second - 0.44829055899754167).abs() < 1e-15,
            "second draw {second} does not match oracle"
        );
    }

    #[test]
    fn mulberry32_token_sequence_matches_the_oracle() {
        let mut rng = SeededRandomSource::new(42);
        let token = rng.next_token();
        assert_eq!(token, "99e1ef7b72c32b89da3b32bfab73b0ac");
    }

    #[test]
    fn zero_seed_is_replaced_with_the_golden_ratio_constant() {
        let mut zero_seeded = SeededRandomSource::new(0);
        let mut golden_seeded = SeededRandomSource::new(0x9e37_79b9);
        assert_eq!(zero_seeded.next_f64(), golden_seeded.next_f64());
    }

    #[test]
    fn next_int_rejects_zero_bound() {
        let mut rng = SeededRandomSource::new(42);
        assert!(rng.next_int(0).is_none());
        let value = rng.next_int(5).expect("valid bound");
        assert!(value < 5);
    }

    #[test]
    fn code_unit_comparison_is_locale_independent() {
        assert_eq!(compare_code_units("a", "b"), Ordering::Less);
        assert_eq!(compare_code_units("b", "a"), Ordering::Greater);
        assert_eq!(compare_code_units("a", "a"), Ordering::Equal);
    }

    #[test]
    fn fixed_clock_advances_and_sets_deterministically() {
        let clock = FixedClock::new(100);
        assert_eq!(clock.now_ms(), 100);
        clock.advance(50);
        assert_eq!(clock.now_ms(), 150);
        clock.set(200);
        assert_eq!(clock.now_ms(), 200);
    }
}
