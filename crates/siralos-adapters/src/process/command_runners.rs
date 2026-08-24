//! Command runner adapters (Stage 3R R13.1).
//!
//! Mirrors the TypeScript reference command-runner surface: the pinned
//! sandbox runtime cannot bind node-script or npm-script execution to the
//! approved bytes, so both runners report truthful unavailability and
//! never launch a process. Status display reads [`is_available`];
//! preparation refuses before any effect.

/// The node-script runner definition id.
pub const NODE_SCRIPT_RUNNER_ID: &str = "node-script";
/// The npm-script runner definition id.
pub const NPM_SCRIPT_RUNNER_ID: &str = "npm-script";

/// The exact reference unavailability notice for node scripts.
pub const NODE_SCRIPT_UNAVAILABLE_NOTICE: &str = "Unavailable: the pinned Node runtime cannot bind execution to the approved script bytes (internal process.binding surfaces and the verify-to-spawn window bypass the boundary).";

/// The exact reference unavailability notice for npm scripts.
pub const NPM_SCRIPT_UNAVAILABLE_NOTICE: &str = "Unavailable: npm execution cannot be bound to the approved package bytes under the pinned sandbox runtime.";

/// Report whether the node-script runner can currently resolve its
/// trusted executable. Always false at this stage; never launches.
pub fn node_script_is_available() -> bool {
    false
}

/// Report whether the npm-script runner can currently resolve its
/// trusted executable. Always false at this stage; never launches.
pub fn npm_script_is_available() -> bool {
    false
}
