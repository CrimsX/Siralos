//! Fuzz target: `siralos_core::version::Version::parse`.
//!
//! Invariants (contract Part 4):
//! - parsing arbitrary bytes never panics;
//! - a successfully parsed version always round-trips through Display
//!   and re-parses to the identical value (decode → encode → decode
//!   preserves canonical semantics);
//! - component values are bounded by the type fields (no silent
//!   truncation, no overflow).

#![no_main]

use libfuzzer_sys::fuzz_target;

use siralos_core::Version;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(version) = Version::parse(text) {
        let canonical = version.to_string();
        let reparsed = Version::parse(&canonical).expect("canonical display always reparses");
        assert_eq!(reparsed, version, "decode → encode → decode must preserve the version");
        assert_eq!(canonical.parse::<Version>().expect("canonical"), version);
    }
});
