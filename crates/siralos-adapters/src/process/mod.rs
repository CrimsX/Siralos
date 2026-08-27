//! Process-boundary adapters: truthful availability reporting only.
//! No process is ever launched. Stage 4.1 adds the generic controlled
//! runtime execution adapter, which still reports unavailable when the
//! identity-bound primitive is absent.

pub mod command_runners;
pub mod runtime_execution;

pub use runtime_execution::{
    RUNTIME_EXECUTION_UNAVAILABLE_REASON, decide_adapter_runtime_execution,
    is_runtime_execution_available,
};
