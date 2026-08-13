//! Version parsing/formatting benchmark (assurance contract Part 13).
//!
//! Representative workload: canonical `major.minor.patch` parse +
//! Display round-trip, plus rejection of malformed input. Baselines are
//! recorded in docs/development/performance-baseline.md.

// The criterion macros generate public harness functions; this is an
// internal benchmark harness, not a public API surface.
#![allow(missing_docs)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};

use siralos_core::Version;

fn bench_parse_canonical(c: &mut Criterion) {
    let input = "1.97.1";
    c.bench_function("version/parse-canonical", |b| {
        b.iter(|| {
            black_box(Version::parse(black_box(input)))
                .expect("canonical parses")
        });
    });
}

fn bench_parse_reject(c: &mut Criterion) {
    let input = "not-a-version";
    c.bench_function("version/parse-reject", |b| {
        b.iter(|| {
            assert!(black_box(Version::parse(black_box(input))).is_err())
        });
    });
}

fn bench_round_trip(c: &mut Criterion) {
    let version = Version::parse("1.97.1").expect("canonical parses");
    c.bench_function("version/display-round-trip", |b| {
        b.iter(|| {
            let text = black_box(version).to_string();
            black_box(text);
        });
    });
}

criterion_group!(
    benches,
    bench_parse_canonical,
    bench_parse_reject,
    bench_round_trip
);
criterion_main!(benches);
