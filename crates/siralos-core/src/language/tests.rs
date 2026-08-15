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
    StructuralDeclaration, StructuralDocument, StructuralIssue,
    StructuralKind, StructureOptions, StructureStatus, SummaryOptions,
    build_structural_summary, normalize_structural_document,
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

fn declaration(
    kind: StructuralKind,
    name: &str,
    line: u64,
) -> StructuralDeclaration {
    StructuralDeclaration::leaf(
        kind,
        Some(name.to_owned()),
        None,
        Some(line),
        Vec::new(),
    )
}

fn normalized(
    path: &str,
    declarations: Vec<StructuralDeclaration>,
    dependencies: Vec<String>,
    issues: Vec<StructuralIssue>,
) -> StructuralDocument {
    let mut document = normalize_structural_document(
        path,
        declarations,
        dependencies,
        issues,
        &StructureOptions::default(),
    );
    document.revision = None;
    document
}

#[test]
fn generic_model_contains_no_language_domain_semantics() {
    // The generic kind vocabulary is the whole semantic surface: no
    // language-domain kinds exist, and attributes are never
    // interpreted.
    let kinds = [
        StructuralKind::Type,
        StructuralKind::Function,
        StructuralKind::Method,
        StructuralKind::Field,
        StructuralKind::Variable,
        StructuralKind::Constant,
        StructuralKind::Enum,
        StructuralKind::Event,
        StructuralKind::Module,
        StructuralKind::Other,
    ];
    for kind in kinds {
        assert!(StructuralKind::parse(kind.as_str()) == Some(kind));
    }
    assert_eq!(StructuralKind::parse("signal"), None);
    assert_eq!(StructuralKind::parse("class_name"), None);
    // A language-provided attribute string is opaque data.
    let document = normalized(
        "src/example.lang",
        vec![StructuralDeclaration::leaf(
            StructuralKind::Field,
            Some("value".to_owned()),
            None,
            Some(2),
            vec!["export".to_owned(), "rpc".to_owned()],
        )],
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(document.declarations[0].attributes, ["export", "rpc"]);
    // The generic summary must not interpret the attribute.
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert!(!summary.text.contains("export"));
    assert!(!summary.text.contains("signal"));
}

#[test]
fn declarations_preserve_deterministic_document_order() {
    let document = normalized(
        "src/example.lang",
        vec![
            declaration(StructuralKind::Type, "Example", 1),
            declaration(StructuralKind::Function, "calculate", 5),
            declaration(StructuralKind::Constant, "LIMIT", 9),
        ],
        Vec::new(),
        Vec::new(),
    );
    let names = document
        .declarations
        .iter()
        .filter_map(|item| item.name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Example", "calculate", "LIMIT"]);
    assert_eq!(document.declaration_count(), 3);
}

#[test]
fn nested_declarations_are_bounded_and_ordered() {
    let nested = vec![StructuralDeclaration {
        kind: StructuralKind::Type,
        name: Some("Example".to_owned()),
        detail: None,
        line: Some(1),
        attributes: Vec::new(),
        children: vec![
            declaration(StructuralKind::Method, "calculate", 3),
            declaration(StructuralKind::Field, "value", 4),
        ],
    }];
    let document =
        normalized("src/example.lang", nested, Vec::new(), Vec::new());
    assert_eq!(document.declaration_count(), 3);
    assert_eq!(
        document.declarations[0].children[0].kind,
        StructuralKind::Method,
    );
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert!(summary.text.contains("method: 1"));
    assert!(summary.text.contains("field: 1"));
}

#[test]
fn partial_structure_is_derived_from_issues_and_explicit() {
    let document = normalized(
        "src/broken.lang",
        vec![declaration(StructuralKind::Function, "run", 3)],
        Vec::new(),
        vec![StructuralIssue {
            line: Some(2),
            message: "Unterminated string literal.".to_owned(),
        }],
    );
    assert_eq!(document.status, StructureStatus::Partial);
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert!(
        summary.text.contains("- structural status: partial (1 issue(s))")
    );
    assert!(summary.text.ends_with(
        "advisory structural summary \u{2014} not authoritative source; read exact before editing."
    ));
    assert!(!summary.truncated);
    assert_eq!(summary.bytes, summary.text.len());
    // A document without issues is complete.
    let clean =
        normalized("src/clean.lang", Vec::new(), Vec::new(), Vec::new());
    assert_eq!(clean.status, StructureStatus::Complete);
}

#[test]
fn truncation_bounds_declarations_dependencies_and_issues() {
    let declarations = (0..6)
        .map(|index| {
            declaration(
                StructuralKind::Function,
                &format!("f{index}"),
                index as u64 + 1,
            )
        })
        .collect::<Vec<_>>();
    let document = normalize_structural_document(
        "src/big.lang",
        declarations,
        (0..8).map(|index| format!("dep{index}")).collect(),
        (0..80)
            .map(|index| StructuralIssue {
                line: Some(index as u64 + 1),
                message: format!("issue {index}"),
            })
            .collect(),
        &StructureOptions {
            max_declarations: 4,
            max_depth: 16,
            max_dependencies: 3,
            max_issues: 5,
        },
    );
    assert!(document.truncated);
    assert_eq!(document.declarations.len(), 4);
    assert_eq!(document.dependencies, ["dep0", "dep1", "dep2"]);
    assert_eq!(document.issues.len(), 5);
    assert_eq!(document.status, StructureStatus::Partial);
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert!(
        summary
            .text
            .contains("- structural output truncated (output bound reached)",)
    );
}

#[test]
fn depth_bound_excludes_deeper_declarations_explicitly() {
    let deep = StructuralDeclaration {
        kind: StructuralKind::Type,
        name: Some("Outer".to_owned()),
        detail: None,
        line: Some(1),
        attributes: Vec::new(),
        children: vec![StructuralDeclaration {
            kind: StructuralKind::Type,
            name: Some("Inner".to_owned()),
            detail: None,
            line: Some(2),
            attributes: Vec::new(),
            children: vec![StructuralDeclaration {
                kind: StructuralKind::Method,
                name: Some("too_deep".to_owned()),
                detail: None,
                line: Some(3),
                attributes: Vec::new(),
                children: Vec::new(),
            }],
        }],
    };
    let document = normalize_structural_document(
        "src/deep.lang",
        vec![deep],
        Vec::new(),
        Vec::new(),
        &StructureOptions {
            max_declarations: 256,
            max_depth: 2,
            max_dependencies: 32,
            max_issues: 64,
        },
    );
    assert!(document.truncated);
    assert_eq!(document.declaration_count(), 2);
    assert!(document.declarations[0].children[0].children.is_empty());
}

#[test]
fn malformed_generic_input_never_panics() {
    // Deeply nested children with a depth-1 bound terminate safely.
    let mut node = StructuralDeclaration::leaf(
        StructuralKind::Other,
        None,
        None,
        None,
        Vec::new(),
    );
    for _ in 0..10_000 {
        node = StructuralDeclaration {
            kind: StructuralKind::Other,
            name: None,
            detail: None,
            line: None,
            attributes: Vec::new(),
            children: vec![node],
        };
    }
    let document = normalize_structural_document(
        "src/hostile.lang",
        vec![node],
        Vec::new(),
        Vec::new(),
        &StructureOptions::default(),
    );
    assert!(document.truncated);
    // Huge declaration lists are bounded, not panned over.
    let many = (0..100_000)
        .map(|index| declaration(StructuralKind::Variable, "", index as u64))
        .collect::<Vec<_>>();
    let bounded = normalize_structural_document(
        "src/huge.lang",
        many,
        Vec::new(),
        Vec::new(),
        &StructureOptions::default(),
    );
    assert!(bounded.truncated);
    assert_eq!(bounded.declarations.len(), 256);
}

#[test]
fn summary_renders_generic_counts_names_and_revision() {
    let mut document = normalized(
        "src/example.lang",
        vec![
            StructuralDeclaration {
                kind: StructuralKind::Type,
                name: Some("Example".to_owned()),
                detail: None,
                line: Some(1),
                attributes: Vec::new(),
                children: vec![declaration(
                    StructuralKind::Method,
                    "calculate",
                    3,
                )],
            },
            declaration(StructuralKind::Function, "run", 5),
            declaration(StructuralKind::Constant, "LIMIT", 9),
        ],
        vec!["lib/common".to_owned()],
        Vec::new(),
    );
    document.revision = Some("rev_abc".to_owned());
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert!(!summary.truncated);
    assert!(summary.text.contains("example.lang (summary @ rev_abc)"));
    assert!(summary.text.contains(
        "- declarations: 4 (type: 1, function: 1, method: 1, constant: 1)",
    ));
    assert!(summary.text.contains("- top-level: Example, run, LIMIT"));
    assert!(summary.text.contains("- dependencies: lib/common"));
    assert!(!summary.text.contains("- extends"));
    assert!(!summary.text.contains("class_name"));
    assert!(!summary.text.contains("signals"));
}

#[test]
fn summary_truncation_keeps_the_footer_and_marks_explicitly() {
    let mut document = normalized(
        "src/big.lang",
        (0..30)
            .map(|index| {
                StructuralDeclaration::leaf(
                    StructuralKind::Function,
                    Some(format!("function_{index:02}")),
                    None,
                    Some(index as u64 + 1),
                    Vec::new(),
                )
            })
            .collect::<Vec<_>>(),
        Vec::new(),
        Vec::new(),
    );
    document.revision = Some("rev_abc".to_owned());
    // A 220-byte budget cannot fit the full body plus the footer, so
    // the body is byte-truncated with the explicit marker.
    let summary = build_structural_summary(
        &document,
        &SummaryOptions {
            max_bytes: Some(220),
            notable_declarations: Some(5),
        },
    );
    assert!(summary.truncated);
    assert!(summary.text.contains("\u{2026} [summary truncated]"));
    assert!(summary.text.contains("- top-level: function_00"));
    assert!(summary.text.ends_with(SUMMARY_FOOTER_END));
    assert!(summary.bytes <= 220);
}

#[test]
fn summary_empty_document_renders_the_baseline() {
    let document =
        normalized("src/empty.lang", Vec::new(), Vec::new(), Vec::new());
    let summary =
        build_structural_summary(&document, &SummaryOptions::default());
    assert_eq!(
        summary.text,
        "empty.lang (summary no revision)\nadvisory structural summary \u{2014} not authoritative source; read exact before editing."
    );
    assert!(!summary.truncated);
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
