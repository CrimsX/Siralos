//! Focused unit tests for the generic language-intelligence module
//! (Stage 3R R5). These verify generic invariants; the differential
//! scenarios verify oracle parity separately, and the property tests
//! strengthen determinism and bound guarantees.

use crate::language::definition::{
    DefinitionLimits, RawDefinitionEntry, normalize_definition_locations,
};
use crate::language::diagnostic::{
    Diagnostic, DiagnosticSeverity, RawDiagnostic, RawDiagnosticCode,
    bind_diagnostic_set, map_lsp_severity, normalize_diagnostic_payload,
    normalize_diagnostic_set, utf16_cmp,
};
use crate::language::limits::LANGUAGE_LIMITS;
use crate::language::position::{
    LanguagePosition, LanguageRange, Location, PositionError, RawPosition,
    RawRange, to_one_based_position, validate_range,
};
use crate::language::reference::{ReferenceLimits, normalize_references};
use crate::language::sanitize::sanitize_control_characters;
use crate::language::structure::{
    FunctionInfo, PropertyInfo, StructuralDocument, StructuralIssue,
    StructureStatus, SummaryOptions, build_structural_summary,
};
use crate::language::symbol::{Symbol, SymbolKind, normalize_symbols};
use crate::language::truncate::truncate_utf8_bytes;
use crate::language::validation::{ValidationResult, ValidationStatus};
use crate::workspace::revision::compute_workspace_revision_handle;

use std::cmp::Ordering;

fn diagnostic(message: &str) -> Diagnostic {
    Diagnostic {
        source: "test-lang".to_owned(),
        severity: DiagnosticSeverity::Error,
        path: Some("src/player.gd".to_owned()),
        line: Some(34),
        column: Some(17),
        code: None,
        message: message.to_owned(),
        raw_category: None,
    }
}

#[test]
fn severity_mapping_covers_the_reference_vocabulary() {
    assert_eq!(map_lsp_severity(Some(1)), DiagnosticSeverity::Error);
    assert_eq!(map_lsp_severity(Some(2)), DiagnosticSeverity::Warning);
    assert_eq!(map_lsp_severity(Some(3)), DiagnosticSeverity::Info);
    assert_eq!(map_lsp_severity(Some(4)), DiagnosticSeverity::Info);
    assert_eq!(map_lsp_severity(Some(9)), DiagnosticSeverity::Unknown);
    assert_eq!(map_lsp_severity(None), DiagnosticSeverity::Unknown);
    assert_eq!(DiagnosticSeverity::Error.as_str(), "error");
    assert_eq!(
        DiagnosticSeverity::parse("info"),
        Some(DiagnosticSeverity::Info)
    );
    assert_eq!(DiagnosticSeverity::parse("fatal"), None);
}

#[test]
fn payload_normalization_never_fabricates_positions() {
    let raw = vec![RawDiagnostic {
        range: None,
        severity: Some(1),
        code: None,
        message: Some("orphan message".to_owned()),
        source: Some("scanner".to_owned()),
    }];
    let payload = normalize_diagnostic_payload(
        &raw,
        "langsvc",
        "scripts/player.gd",
        None,
        &LANGUAGE_LIMITS,
    );
    assert_eq!(payload.diagnostics.len(), 1);
    let entry = &payload.diagnostics[0];
    assert_eq!(entry.line, None);
    assert_eq!(entry.column, None);
    assert_eq!(entry.path.as_deref(), Some("scripts/player.gd"));
    assert_eq!(entry.raw_category.as_deref(), Some("scanner"));
    assert_eq!(entry.source, "langsvc");
    assert!(!payload.truncated);
}

#[test]
fn payload_normalization_converts_positions_and_skips_malformed() {
    let raw = vec![
        RawDiagnostic {
            range: Some(RawRange {
                start: RawPosition { line: Some(32), column: Some(15) },
                end: RawPosition { line: Some(32), column: Some(26) },
            }),
            severity: Some(2),
            code: Some(RawDiagnosticCode::Number(7)),
            message: Some("warning text".to_owned()),
            source: None,
        },
        // Malformed entries are skipped conservatively.
        RawDiagnostic {
            range: Some(RawRange {
                start: RawPosition { line: Some(-1), column: Some(0) },
                end: RawPosition { line: Some(1), column: Some(1) },
            }),
            severity: Some(1),
            code: None,
            message: Some("bad position".to_owned()),
            source: None,
        },
        RawDiagnostic {
            range: None,
            severity: None,
            code: None,
            // Empty message after sanitization: skipped.
            message: Some("   \u{0} \u{7f} ".to_owned()),
            source: None,
        },
        RawDiagnostic {
            range: None,
            severity: None,
            code: None,
            message: None,
            source: None,
        },
    ];
    let payload = normalize_diagnostic_payload(
        &raw,
        "langsvc",
        "scripts/player.gd",
        None,
        &LANGUAGE_LIMITS,
    );
    // The malformed-position entry is preserved with null coordinates
    // (never fabricated), the sanitized-empty entry is kept (U+FFFD
    // replacements are real text), and only a missing message is
    // skipped: three diagnostics total.
    assert_eq!(payload.diagnostics.len(), 3);
    let entry = &payload.diagnostics[0];
    assert_eq!(entry.severity, DiagnosticSeverity::Warning);
    assert_eq!(entry.line, Some(33));
    assert_eq!(entry.column, Some(16));
    assert_eq!(entry.code.as_deref(), Some("7"));
    assert_eq!(payload.diagnostics[1].line, None);
    assert_eq!(payload.diagnostics[1].column, None);
    assert_eq!(payload.diagnostics[1].severity, DiagnosticSeverity::Error);
    assert_eq!(payload.diagnostics[2].message, "\u{fffd} \u{fffd}");
}

#[test]
fn message_sanitization_and_masking_are_bounded() {
    let raw = vec![RawDiagnostic {
        range: None,
        severity: None,
        code: None,
        message: Some(
            "bad \u{1b}[31mred\u{1b}[0m at /work/root deep and then a very long tail".to_owned(),
        ),
        source: None,
    }];
    let payload = normalize_diagnostic_payload(
        &raw,
        "langsvc",
        "scripts/player.gd",
        Some("/work/root"),
        &LANGUAGE_LIMITS,
    );
    let message = &payload.diagnostics[0].message;
    assert!(!message.contains("\u{1b}"), "CSI sequences must be stripped");
    assert!(message.contains("<mirror>"), "root path must be masked");
    assert!(
        payload.diagnostics[0].message.len()
            <= LANGUAGE_LIMITS.max_diagnostic_message_bytes
    );
}

#[test]
fn diagnostic_set_ordering_is_deterministic() {
    let result = normalize_diagnostic_set(
        vec![
            diagnostic("z"),
            diagnostic("a"),
            diagnostic("a"), // exact duplicate
            Diagnostic { path: None, ..diagnostic("orphan") },
            Diagnostic { line: Some(40), ..diagnostic("later") },
        ],
        100,
    );
    let (diagnostics, truncated) = result;
    assert!(!truncated);
    assert_eq!(diagnostics.len(), 4);
    // null path sorts first (reference -1 convention), then by line,
    // then by message (UTF-16 order).
    assert_eq!(diagnostics[0].path, None);
    assert_eq!(diagnostics[1].line, Some(34));
    assert_eq!(diagnostics[1].message, "a");
    assert_eq!(diagnostics[2].message, "z");
    assert_eq!(diagnostics[3].line, Some(40));
}

#[test]
fn utf16_ordering_matches_javascript_string_order() {
    // Astral characters sort BEFORE BMP characters in UTF-16 order
    // (high surrogate 0xD800 < 0xFFFF), but AFTER in byte order.
    let astral = "a\u{1f600}";
    let bmp_high = "a\u{ffff}";
    assert_eq!(utf16_cmp(astral, bmp_high), Ordering::Less);
    assert_eq!(utf16_cmp("abc", "abd"), Ordering::Less);
    assert_eq!(utf16_cmp("abc", "abc"), Ordering::Equal);
    assert_eq!(utf16_cmp("abc", "abcd"), Ordering::Less);
}

#[test]
fn diagnostic_set_run_bound_truncates_explicitly() {
    let many = (1..=12)
        .map(|line| Diagnostic { line: Some(line), ..diagnostic("x") })
        .collect::<Vec<_>>();
    let (diagnostics, truncated) = normalize_diagnostic_set(many, 10);
    assert_eq!(diagnostics.len(), 10);
    assert!(truncated);
}

#[test]
fn revision_binding_uses_the_r4_identity() {
    let handle = compute_workspace_revision_handle(
        "fixture-workspace",
        "scripts/player.gd",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert!(handle.starts_with("rev_"));
    assert_eq!(handle.len(), 36);
    let set = bind_diagnostic_set(
        "scripts/player.gd",
        Some(handle.clone()),
        vec![diagnostic("x")],
        &LANGUAGE_LIMITS,
    );
    assert_eq!(set.revision.as_deref(), Some(handle.as_str()));
    assert_eq!(set.path, "scripts/player.gd");
}

#[test]
fn definition_results_preserve_order_and_bounds() {
    let entries = (0..5)
        .map(|index| RawDefinitionEntry {
            uri: Some(format!("file:///work/project/f{index}.gd")),
            range: Some(RawRange {
                start: RawPosition { line: Some(index), column: Some(0) },
                end: RawPosition { line: Some(index), column: Some(1) },
            }),
        })
        .collect::<Vec<_>>();
    let map_uri = |uri: &str| -> Option<String> {
        uri.strip_prefix("file:///work/project/").map(str::to_owned)
    };
    let result = normalize_definition_locations(
        &entries,
        "scripts/player.gd",
        map_uri,
        DefinitionLimits { max_locations: 3 },
    );
    assert_eq!(result.path, "scripts/player.gd");
    assert_eq!(result.locations.len(), 3);
    assert!(result.truncated);
    assert_eq!(result.locations[0].path, "f0.gd");
    assert!(!result.locations[0].external);
}

#[test]
fn outside_workspace_definitions_are_conservative() {
    let entries = vec![
        RawDefinitionEntry {
            uri: Some("file:///elsewhere/engine/core.gd".to_owned()),
            range: Some(RawRange {
                start: RawPosition { line: Some(0), column: Some(0) },
                end: RawPosition { line: Some(0), column: Some(1) },
            }),
        },
        // No URI: skipped.
        RawDefinitionEntry { uri: None, range: None },
        // Malformed range: skipped.
        RawDefinitionEntry {
            uri: Some("file:///work/project/x.gd".to_owned()),
            range: None,
        },
    ];
    let result = normalize_definition_locations(
        &entries,
        "q.gd",
        |_| None,
        DefinitionLimits { max_locations: 100 },
    );
    assert_eq!(result.locations.len(), 1);
    assert!(result.locations[0].external);
    // Conservative basename, never an absolute path.
    assert_eq!(result.locations[0].path, "core.gd");
    assert!(!result.locations[0].path.starts_with('/'));
    assert!(!result.locations[0].path.contains("elsewhere"));
}

#[test]
fn symbols_order_deterministically_and_keep_duplicate_names() {
    let symbols = vec![
        Symbol {
            name: "bump".to_owned(),
            kind: SymbolKind::Function,
            line: Some(9),
            detail: None,
        },
        Symbol {
            name: "bump".to_owned(),
            kind: SymbolKind::Function,
            line: Some(4),
            detail: None,
        },
        Symbol {
            name: "alpha".to_owned(),
            kind: SymbolKind::Property,
            line: None,
            detail: None,
        },
    ];
    let result = normalize_symbols(
        "scripts/player.gd",
        None,
        symbols,
        &LANGUAGE_LIMITS,
    );
    // Unknown line first (-1), then line 4, then line 9; duplicate
    // names at different locations remain distinguishable.
    assert_eq!(result.symbols.len(), 3);
    assert_eq!(result.symbols[0].name, "alpha");
    assert_eq!(result.symbols[1].line, Some(4));
    assert_eq!(result.symbols[2].line, Some(9));
    assert!(!result.truncated);
}

#[test]
fn references_preserve_order_and_bind_revision() {
    let locations = vec![
        Location {
            path: "scripts/ui.gd".to_owned(),
            range: LanguageRange {
                start: LanguagePosition { line: 3, column: 1 },
                end: LanguagePosition { line: 3, column: 5 },
            },
        },
        Location {
            path: "scripts/ui.gd".to_owned(),
            range: LanguageRange {
                start: LanguagePosition { line: 9, column: 1 },
                end: LanguagePosition { line: 9, column: 5 },
            },
        },
    ];
    let result = normalize_references(
        "scripts/player.gd",
        Some("rev_123".to_owned()),
        locations,
        ReferenceLimits { max_locations: 1 },
    );
    assert_eq!(result.locations.len(), 1);
    assert!(result.truncated);
    assert_eq!(result.revision.as_deref(), Some("rev_123"));
}

#[test]
fn validation_distinguishes_source_invalid_from_infrastructure_failure() {
    let valid = ValidationResult::valid();
    assert_eq!(valid.status, ValidationStatus::Valid);
    assert!(valid.diagnostics.is_none());
    let invalid = ValidationResult::invalid(bind_diagnostic_set(
        "scripts/player.gd",
        None,
        vec![diagnostic("syntax")],
        &LANGUAGE_LIMITS,
    ));
    assert_eq!(invalid.status, ValidationStatus::Invalid);
    assert!(invalid.diagnostics.is_some());
    let unavailable = ValidationResult::infrastructure(
        ValidationStatus::Unavailable,
        "no executing language service".to_owned(),
    );
    assert_eq!(unavailable.status, ValidationStatus::Unavailable);
    assert!(unavailable.diagnostics.is_none());
    // Source invalid and infrastructure failure are distinct statuses.
    assert_ne!(invalid.status, unavailable.status);
    assert_eq!(ValidationStatus::Invalid.as_str(), "invalid");
    assert_eq!(ValidationStatus::Unavailable.as_str(), "unavailable");
}

#[test]
fn partial_structure_is_explicitly_marked() {
    let document = StructuralDocument {
        path: "scripts/player.gd".to_owned(),
        revision: None,
        base_type: Some("Node".to_owned()),
        declared_name: Some("Player".to_owned()),
        file_annotations: Vec::new(),
        signals: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        properties: Vec::new(),
        functions: Vec::new(),
        dependencies: Vec::new(),
        status: StructureStatus::Partial,
        issues: vec![StructuralIssue {
            line: 2,
            message: "Unterminated string literal.".to_owned(),
        }],
        truncated: false,
    };
    let summary =
        build_structural_summary(&document, None, &SummaryOptions::default());
    assert!(
        summary
            .text
            .contains("structural_status: partial (1 parser error(s))")
    );
    assert!(summary.text.ends_with(
        "advisory structural summary \u{2014} not authoritative source; read exact before editing."
    ));
    assert!(summary.text.contains("- extends Node"));
    assert!(summary.text.contains("- class_name Player"));
    assert!(!summary.truncated);
    assert_eq!(summary.bytes, summary.text.len());
}

#[test]
fn summary_truncation_keeps_the_footer_and_marks_explicitly() {
    let functions = (0..60)
        .map(|index| FunctionInfo {
            name: format!("function_{index:02}"),
            parameters: Vec::new(),
            return_type: None,
            is_static: false,
            annotations: Vec::new(),
            line: index as u64 + 1,
            multiline_signature: false,
        })
        .collect::<Vec<_>>();
    let document = StructuralDocument {
        path: "scripts/big.gd".to_owned(),
        revision: None,
        base_type: None,
        declared_name: None,
        file_annotations: Vec::new(),
        signals: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        properties: vec![PropertyInfo {
            name: "speed".to_owned(),
            type_name: Some("float".to_owned()),
            annotations: vec!["export".to_owned()],
            line: 1,
            multiline: false,
        }],
        functions,
        dependencies: Vec::new(),
        status: StructureStatus::Complete,
        issues: Vec::new(),
        truncated: true,
    };
    let summary = build_structural_summary(
        &document,
        Some("rev_abc"),
        &SummaryOptions { max_bytes: Some(300), notable_methods: Some(12) },
    );
    assert!(summary.truncated);
    assert!(summary.text.contains("\u{2026} [summary truncated]"));
    assert!(summary.text.contains("exported properties: speed"));
    assert!(summary.text.ends_with(SUMMARY_FOOTER_END));
    assert!(summary.bytes <= 300);
}

#[test]
fn summary_renders_the_notable_function_total() {
    let functions = (0..20)
        .map(|index| FunctionInfo {
            name: format!("fn_{index}"),
            parameters: Vec::new(),
            return_type: None,
            is_static: index % 2 == 0,
            annotations: Vec::new(),
            line: index as u64 + 1,
            multiline_signature: false,
        })
        .collect::<Vec<_>>();
    let document = StructuralDocument {
        path: "scripts/many.gd".to_owned(),
        revision: None,
        base_type: None,
        declared_name: None,
        file_annotations: Vec::new(),
        signals: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        properties: Vec::new(),
        functions,
        dependencies: vec!["res://assets/icon.svg".to_owned()],
        status: StructureStatus::Complete,
        issues: Vec::new(),
        truncated: false,
    };
    let summary = build_structural_summary(
        &document,
        None,
        &SummaryOptions { max_bytes: Some(4096), notable_methods: Some(12) },
    );
    assert!(!summary.truncated);
    assert!(summary.text.contains("fn_0 (static)"));
    assert!(summary.text.contains(", ... (20 total)"));
    assert!(summary.text.contains("- dependencies: res://assets/icon.svg"));
    assert!(summary.text.contains("- 20 functions"));
}

const SUMMARY_FOOTER_END: &str = "advisory structural summary \u{2014} not authoritative source; read exact before editing.";

#[test]
fn sanitization_handles_unicode_and_controls() {
    assert_eq!(
        sanitize_control_characters("caf\u{e9} \u{1f600}\n"),
        "caf\u{e9} \u{1f600}\n",
    );
    assert_eq!(
        sanitize_control_characters("\u{0}\u{8}\u{9}\u{a}\u{d}\u{7f}"),
        "\u{fffd}\u{fffd}\t\n\r\u{fffd}"
    );
    assert_eq!(truncate_utf8_bytes("h\u{e9}llo", 4), "h\u{e9}l");
}

#[test]
fn malformed_positions_never_panic() {
    for raw in [
        RawPosition { line: None, column: None },
        RawPosition { line: Some(-5), column: Some(0) },
        RawPosition { line: Some(0), column: Some(-1) },
    ] {
        assert_eq!(to_one_based_position(raw), None);
    }
    let reversed = LanguageRange {
        start: LanguagePosition { line: 2, column: 1 },
        end: LanguagePosition { line: 1, column: 1 },
    };
    assert_eq!(validate_range(reversed), Err(PositionError::UnorderedRange));
}

#[test]
fn summary_empty_document_renders_the_baseline() {
    let document = StructuralDocument {
        path: "scripts/empty.gd".to_owned(),
        revision: None,
        base_type: None,
        declared_name: None,
        file_annotations: Vec::new(),
        signals: Vec::new(),
        enums: Vec::new(),
        constants: Vec::new(),
        properties: Vec::new(),
        functions: Vec::new(),
        dependencies: Vec::new(),
        status: StructureStatus::Complete,
        issues: Vec::new(),
        truncated: false,
    };
    let summary =
        build_structural_summary(&document, None, &SummaryOptions::default());
    assert_eq!(
        summary.text,
        "empty.gd (summary no revision)\nadvisory structural summary \u{2014} not authoritative source; read exact before editing."
    );
    assert!(!summary.truncated);
}
