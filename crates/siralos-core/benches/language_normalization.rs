//! Benchmark: generic language-intelligence normalization (R5).
//!
//! Measures deterministic normalization throughput (diagnostic set
//! aggregation and advisory summary formatting). No hard performance
//! target is established; the benchmarks guard against accidental
//! regressions in the bounded hot paths.

// The criterion macros generate public harness functions; this is an
// internal benchmark harness, not a public API surface.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};

use siralos_core::language::diagnostic::{
    Diagnostic, DiagnosticSeverity, normalize_diagnostic_set,
};
use siralos_core::language::limits::LANGUAGE_LIMITS;
use siralos_core::language::structure::{
    StructuralDeclaration, StructuralDocument, StructuralKind,
    StructureOptions, SummaryOptions, build_structural_summary,
    normalize_structural_document,
};

fn sample_diagnostics(count: usize) -> Vec<Diagnostic> {
    (0..count)
        .map(|index| Diagnostic {
            source: "bench".to_owned(),
            severity: match index % 4 {
                0 => DiagnosticSeverity::Error,
                1 => DiagnosticSeverity::Warning,
                2 => DiagnosticSeverity::Info,
                _ => DiagnosticSeverity::Unknown,
            },
            path: Some(if index % 2 == 0 {
                "scripts/player.gd".to_owned()
            } else {
                "scripts/enemy.gd".to_owned()
            }),
            line: Some((index % 1000) as u64 + 1),
            column: Some((index % 40) as u64 + 1),
            code: Some(format!("code-{}", index % 7)),
            message: format!("message {index} with some text"),
            raw_category: None,
        })
        .collect()
}

fn sample_document() -> StructuralDocument {
    let declarations = (0..64)
        .map(|index| {
            StructuralDeclaration::leaf(
                StructuralKind::Function,
                Some(format!("function_{index:02}")),
                Some("(int) -> void".to_owned()),
                Some(index as u64 + 1),
                if index % 2 == 0 {
                    vec!["static".to_owned()]
                } else {
                    Vec::new()
                },
            )
        })
        .collect();
    normalize_structural_document(
        "src/bench.lang",
        declarations,
        vec!["lib/common".to_owned()],
        Vec::new(),
        &StructureOptions::default(),
    )
}

fn bench_normalize_diagnostics(criterion: &mut Criterion) {
    let diagnostics = sample_diagnostics(10_000);
    criterion.bench_function("normalize_diagnostic_set/10000", |bencher| {
        bencher.iter(|| {
            let (result, truncated) = normalize_diagnostic_set(
                std::hint::black_box(diagnostics.clone()),
                LANGUAGE_LIMITS.max_diagnostics_per_run,
            );
            std::hint::black_box((result.len(), truncated));
        })
    });
}

fn bench_structural_summary(criterion: &mut Criterion) {
    let document = sample_document();
    criterion.bench_function(
        "build_structural_summary/64-functions",
        |bencher| {
            bencher.iter(|| {
                let summary = build_structural_summary(
                    std::hint::black_box(&document),
                    &SummaryOptions::default(),
                );
                std::hint::black_box(summary.bytes);
            })
        },
    );
}

criterion_group!(
    language_normalization,
    bench_normalize_diagnostics,
    bench_structural_summary,
);
criterion_main!(language_normalization);
