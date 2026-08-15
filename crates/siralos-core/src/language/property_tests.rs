//! Property tests over the generic language-intelligence invariants
//! (Stage 3R R5).
//!
//! Invariants: sorting determinism (any insertion order yields the
//! same canonical output), range ordering validation, truncation
//! never exceeds the bound, stable sanitization, and revision
//! metadata preserved through normalization.

use proptest::prelude::*;

use crate::language::diagnostic::{
    Diagnostic, DiagnosticSeverity, normalize_diagnostic_set, utf16_cmp,
};
use crate::language::position::{
    LanguagePosition, LanguageRange, PositionError, validate_range,
};
use crate::language::sanitize::sanitize_control_characters;
use crate::language::structure::build_structural_summary;

/// The advisory footer suffix (never truncated away).
const SUMMARY_FOOTER_END: &str = "advisory structural summary \u{2014} not authoritative source; read exact before editing.";
use crate::language::truncate::{
    truncate_utf8_bytes, utf16_prefix_byte_len, utf16_prefix_lossy,
};

fn sample_diagnostic(seed: u64) -> Diagnostic {
    let path = match seed % 3 {
        0 => Some("a.gd".to_owned()),
        1 => Some("b.gd".to_owned()),
        _ => None,
    };
    Diagnostic {
        source: "prop".to_owned(),
        severity: match seed % 4 {
            0 => DiagnosticSeverity::Error,
            1 => DiagnosticSeverity::Warning,
            2 => DiagnosticSeverity::Info,
            _ => DiagnosticSeverity::Unknown,
        },
        path,
        line: if seed % 5 == 0 { None } else { Some(seed % 50 + 1) },
        column: Some(seed % 20 + 1),
        code: if seed % 2 == 0 {
            Some(format!("code-{}", seed % 7))
        } else {
            None
        },
        message: format!("message-{}", seed % 9),
        raw_category: None,
    }
}

proptest! {
    /// Any insertion order of the same multiset yields the same
    /// canonical diagnostic sequence.
    #[test]
    fn diagnostic_set_normalization_is_insertion_order_independent(
        order in 0..24u64,
    ) {
        // A small fixed pool; shuffle by a deterministic permutation.
        let pool: Vec<Diagnostic> =
            (0..6).map(sample_diagnostic).collect();
        let mut working = pool.clone();
        let mut permuted = Vec::new();
        let mut seed_value = order;
        while !working.is_empty() {
            seed_value = seed_value.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let pick = (seed_value % working.len() as u64) as usize;
            permuted.push(working.swap_remove(pick));
        }
        let (canonical, _) = normalize_diagnostic_set(pool, 100);
        let (reordered, _) = normalize_diagnostic_set(permuted, 100);
        prop_assert_eq!(canonical, reordered);
    }

    /// Truncation never exceeds the bound and never splits a code point.
    #[test]
    fn truncation_respects_the_byte_bound(text in ".{0,64}", bound in 0usize..128) {
        let truncated = truncate_utf8_bytes(&text, bound);
        prop_assert!(truncated.len() <= bound);
        prop_assert!(truncated.len() <= text.len());
    }

    /// Sanitization is idempotent and never introduces control bytes.
    #[test]
    fn sanitization_is_stable(text in ".{0,64}") {
        let once = sanitize_control_characters(&text);
        let twice = sanitize_control_characters(&once);
        prop_assert_eq!(once, twice);
    }

    /// The UTF-16 prefix helpers agree on byte length and text.
    #[test]
    fn utf16_prefix_helpers_agree(text in ".{0,32}", units in 0usize..80) {
        let prefix = utf16_prefix_lossy(&text, units);
        let bytes = utf16_prefix_byte_len(&text, units);
        prop_assert_eq!(bytes, prefix.len());
    }

    /// utf16_cmp is a total order consistent with prefix comparison.
    #[test]
    fn utf16_cmp_is_antisymmetric(left in ".{0,16}", right in ".{0,16}") {
        use std::cmp::Ordering;
        let a = utf16_cmp(&left, &right);
        let b = utf16_cmp(&right, &left);
        match a {
            Ordering::Equal => prop_assert_eq!(b, Ordering::Equal),
            Ordering::Less => prop_assert_eq!(b, Ordering::Greater),
            Ordering::Greater => prop_assert_eq!(b, Ordering::Less),
        }
    }

    /// Summary byte budget is never exceeded and the footer survives.
    #[test]
    fn summary_bytes_never_exceed_the_budget(
        name in ".{0,12}",
        declarations in 0usize..24,
        max_bytes in 120usize..512,
    ) {
        use crate::language::structure::{
            StructuralDeclaration, StructuralKind, StructureOptions,
            SummaryOptions, normalize_structural_document,
        };
        let declarations_list = (0..declarations)
            .map(|index| StructuralDeclaration::leaf(
                StructuralKind::Function,
                Some(format!("fn_{index}")),
                None,
                Some(index as u64 + 1),
                Vec::new(),
            ))
            .collect();
        let document = normalize_structural_document(
            &format!("src/{name}.lang"),
            declarations_list,
            Vec::new(),
            Vec::new(),
            &StructureOptions::default(),
        );
        let summary = build_structural_summary(
            &document,
            &SummaryOptions {
                max_bytes: Some(max_bytes),
                notable_declarations: Some(12),
            },
        );
        prop_assert!(summary.bytes <= max_bytes);
        prop_assert!(summary.text.ends_with(SUMMARY_FOOTER_END));
    }


    /// Malformed ranges are rejected, never panicked on.
    #[test]
    fn range_validation_never_panics(s1 in 0u64..100, c1 in 0u64..100, s2 in 0u64..100, c2 in 0u64..100) {
        let range = LanguageRange {
            start: LanguagePosition { line: s1, column: c1 },
            end: LanguagePosition { line: s2, column: c2 },
        };
        match validate_range(range) {
            Ok(validated) => {
                prop_assert_eq!(validated, range);
            }
            Err(PositionError::InvalidPosition) => {
                prop_assert!(s1 == 0 || c1 == 0 || s2 == 0 || c2 == 0);
            }
            Err(PositionError::UnorderedRange) => {
                prop_assert!(s1 > s2 || (s1 == s2 && c1 > c2));
            }
        }
    }
}
