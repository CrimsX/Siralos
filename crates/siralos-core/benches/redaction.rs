//! Secret-redaction benchmark (assurance contract Parts 13–14).
//!
//! Measures `siralos_core::doctor::sanitize_secrets_only`, the single
//! redaction owner, over six input shapes at six sizes. The baseline and the
//! post-optimization result are recorded in
//! `docs/development/performance-baseline.md`.
//!
//! Only the public entry is callable here — a criterion bench is its own crate
//! and the individual finders are private — so per-rule signal comes from the
//! shape of the input, never from naming a pass.

// The criterion macros generate public harness functions; this is an internal
// benchmark harness, not a public API surface.
#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use siralos_core::doctor::sanitize_secrets_only;

/// The sizes every shape is measured at: empty, the shortest input that still
/// carries a trigger, a paragraph, a page, a large brief, and a brief far past
/// anything the product renders today.
///
/// `30b` sits below both fixed-width minimums in the reference patterns (32 for
/// the hex run, 40 for the base64 run) and above every shape's first complete
/// trigger, so it is the one size at which a length guard can be seen to fire.
const SIZES: [(&str, usize); 6] = [
    ("0b", 0),
    ("30b", 30),
    ("64b", 64),
    ("1kb", 1024),
    ("16kb", 16 * 1024),
    ("64kb", 64 * 1024),
];

/// Build `unit` repeated to `target` bytes, truncated on a char boundary.
fn fill(unit: &str, target: usize) -> String {
    let mut out = String::with_capacity(target + unit.len());
    while out.len() < target {
        out.push_str(unit);
    }
    let mut end = target.min(out.len());
    while end > 0 && !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out
}

/// The six shapes, each a unit repeated to the target size.
///
/// `prose` and `near-trigger` must stay **secret-free**: the second is the one
/// that separates "skip a pass" from "move the work", because it carries the
/// literals and near-threshold runs without a single match.
fn shapes() -> [(&'static str, String); 6] {
    [
        ("prose", "the quick brown fox jumps over the lazy dog. ".to_owned()),
        (
            "near-trigger",
            format!(
                "sk-abc AKI bearer = {hex31} {b64_39} ",
                hex31 = "a".repeat(31),
                b64_39 = "z".repeat(39),
            ),
        ),
        (
            "all-rules",
            format!(
                "sk-abcdefgh AKIAIOSFODNN7EXAMPLE ghp_{gh} \
                 Bearer abcdefghijkl {hex32} {b64_40} ",
                gh = "a".repeat(20),
                hex32 = "a".repeat(32),
                b64_40 = "z".repeat(40),
            ),
        ),
        (
            "case-variants",
            "bearer abcdefghijkl BEARER abcdefghijkl ".to_owned(),
        ),
        (
            "non-ascii",
            "漢字テスト🔒sk-abcdefgh🔒漢 Bearer abcdefghijkl é ".to_owned(),
        ),
        ("secret-marker", "<secret> ".to_owned()),
    ]
}

fn bench_redaction(c: &mut Criterion) {
    for (size_label, size) in SIZES {
        for (shape, unit) in shapes() {
            let input = fill(&unit, size);
            // The shape labels are claims, so check them once, outside the
            // timed loop: prose and near-trigger are secret-free, the marker is
            // inert to every pass, and every remaining shape must redact.
            if !input.is_empty() {
                let redacted = sanitize_secrets_only(&input);
                match shape {
                    "prose" | "near-trigger" | "secret-marker" => {
                        assert_eq!(
                            redacted, input,
                            "{shape} must be left alone"
                        );
                    }
                    _ => assert!(
                        redacted.contains("<secret>"),
                        "{shape} must redact"
                    ),
                }
            }
            c.bench_function(
                &format!("redaction/{shape}-{size_label}"),
                |b| {
                    b.iter(|| sanitize_secrets_only(black_box(&input)));
                },
            );
        }
    }
}

criterion_group!(benches, bench_redaction);
criterion_main!(benches);
