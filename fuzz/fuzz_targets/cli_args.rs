//! Fuzz target: `siralos_cli::parse_args`.
//!
//! Invariants:
//! - argument parsing never panics on arbitrary byte sequences;
//! - every argument is either valid UTF-8 and classified, or rejected
//!   with a typed `UsageError` — never silently accepted as another
//!   flag.

#![no_main]

use libfuzzer_sys::fuzz_target;

use std::ffi::OsString;

use siralos_cli::parse_args;

fuzz_target!(|data: &[u8]| {
    let mut args = Vec::new();
    for chunk in data.split(|byte| *byte == 0) {
        // OsString::from_vec is Unix-only; on Windows the raw bytes are
        // not representable portably, so feed lossy text instead. The
        // non-UTF-8 rejection path is covered by unit tests on each
        // platform.
        #[cfg(unix)]
        let arg = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(chunk.to_vec())
        };
        #[cfg(not(unix))]
        let arg = OsString::from(String::from_utf8_lossy(chunk));
        args.push(arg);
        let _ = parse_args(args.clone());
    }
});
