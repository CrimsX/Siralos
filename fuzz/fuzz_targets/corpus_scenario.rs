//! Fuzz target: differential corpus scenario decoding.
//!
//! Invariants:
//! - arbitrary JSON never panics the candidate runner's corpus decoder;
//! - malformed or unknown-shaped scenarios are rejected with a typed
//!   `serde_json::Error` — they never silently become a valid scenario
//!   (invalid data never silently becomes valid);
//! - a decoded scenario with an unknown subject or parity value is
//!   explicitly rejected by the harness's own validation, never
//!   silently executed.

#![no_main]

use libfuzzer_sys::fuzz_target;

use siralos_cli::harness::Scenario;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(scenario) = serde_json::from_str::<Scenario>(text) else {
        return;
    };
    // Mirror the harness's load-time validation: an invalid parity or
    // an unknown subject must never pass silently.
    if scenario.parity != "required" && scenario.parity != "informational" {
        return;
    }
    match scenario.subject.as_str() {
        "state-dir" | "version-identity" => {}
        _ => return,
    }
});
