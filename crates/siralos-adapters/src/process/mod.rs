//! Process-boundary adapters: truthful availability reporting, plus
//! exactly one enforcing execution path. Stage 4.1 adds the generic
//! controlled runtime execution adapter, which still reports
//! unavailable when the identity-bound primitive is absent. ADR 0038
//! adds the sandbox backend port implementation, whose live probe
//! reports the honest enforcement state, and Phase C adds the
//! enforcing backend over the pinned Sandbox Runtime CLI: the only
//! adapter that spawns, and only after its live canary proves
//! enforcement on this machine right now.

pub mod command_runners;
pub mod enforcing_backend;
pub mod runtime_execution;
pub mod sandbox_backend;

pub use runtime_execution::{
    RUNTIME_EXECUTION_UNAVAILABLE_REASON, decide_adapter_runtime_execution,
    is_runtime_execution_available,
};
