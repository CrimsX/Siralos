//! Fuzz target: differential corpus scenario decoding.
//!
//! Invariants (contract Part 4):
//! - arbitrary bytes never panic, hang, or over-allocate: the harness
//!   entry bounds input at its scenario byte limit, parses strict JSON,
//!   and answers every rejection with a typed error;
//! - malformed or unknown-shaped scenarios are rejected with a typed
//!   error — they never silently become a valid scenario
//!   (invalid data never silently becomes valid);
//! - a decoded scenario with an unknown subject or parity value is
//!   explicitly rejected by the harness's own strict validation, never
//!   silently executed.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The strict per-scenario gate lives in the harness crate itself
    // (typed parse + validation + canonical digest; no filesystem,
    // no process). This target only proves arbitrary bytes cannot
    // break it: every rejection is a typed error.
    let _ = siralos_harness::harness::fuzz_scenario_bytes(data);
});
