//! Causal runtime identity (Stage 3 — Runtime Readiness & Operational
//! Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/identity.ts`. Hierarchical causal
//! correlation, not distributed tracing:
//!
//! ```text
//! TaskId -> PhaseId -> RunId -> OperationId -> Evidence/Artifact
//! ```
//!
//! Ids are deterministic from host inputs over the single digest
//! primitive (`siralos:RunId:v1` / `siralos:OperationId:v1`), so
//! equivalent inputs produce equivalent identities.

use super::{RuntimeError, runtime_error};
use crate::identity::CanonicalValue;
use crate::identity::compute_artifact_digest;

fn object(entries: Vec<(&str, CanonicalValue)>) -> CanonicalValue {
    CanonicalValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string_value(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

/// Inputs for [`create_run_id`].
pub struct RunIdentityInput<'a> {
    /// Owning task id.
    pub task_id: &'a str,
    /// Owning phase id.
    pub phase_id: &'a str,
    /// Run sequence within the phase (1-based).
    pub sequence: u64,
    /// Run kind for the id domain; defaults to `runtime`.
    pub kind: Option<&'a str>,
}

/// Deterministic run id: `run_<kind>_<24 hex of task:phase:seq>`.
pub fn create_run_id(
    input: &RunIdentityInput<'_>,
) -> Result<String, RuntimeError> {
    if input.task_id.is_empty() || input.phase_id.is_empty() {
        return Err(runtime_error(
            "A run identity requires a task id and a phase id.",
        ));
    }
    if input.sequence < 1 {
        return Err(runtime_error(
            "A run sequence must be a positive safe integer.",
        ));
    }
    let kind = input.kind.unwrap_or("runtime");
    let payload = object(vec![
        ("taskId", string_value(input.task_id)),
        ("phaseId", string_value(input.phase_id)),
        ("sequence", CanonicalValue::U64(input.sequence)),
        ("kind", string_value(kind)),
    ]);
    let digest = compute_artifact_digest("RunId", 1, &payload)
        .map_err(|error| runtime_error(error.message))?;
    Ok(format!("run_{kind}_{}", &digest.value[..24]))
}

/// Deterministic operation id within a run: `op_<24 hex>`.
pub fn create_operation_id(run_id: &str, operation: &str) -> String {
    let payload = object(vec![
        ("runId", string_value(run_id)),
        ("operation", string_value(operation)),
    ]);
    let digest = compute_artifact_digest("OperationId", 1, &payload)
        .expect("operation identity inputs are structurally valid");
    format!("op_{}", &digest.value[..24])
}

/// Causal trace reference preserved on evidence and artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTraceRef {
    /// Owning task id.
    pub task_id: String,
    /// Owning phase id.
    pub phase_id: String,
    /// The run that produced the observation.
    pub run_id: String,
    /// Operation within the run, when attributed.
    pub operation_id: Option<String>,
    /// Producer identity, e.g. `process-supervisor`.
    pub producer: String,
}

/// Build a causal trace reference; an absent operation stays `None`
/// (serialized as JSON null by projections).
pub fn create_run_trace_ref(
    task_id: &str,
    phase_id: &str,
    run_id: &str,
    operation_id: Option<&str>,
    producer: &str,
) -> RunTraceRef {
    RunTraceRef {
        task_id: task_id.to_owned(),
        phase_id: phase_id.to_owned(),
        run_id: run_id.to_owned(),
        operation_id: operation_id.map(str::to_owned),
        producer: producer.to_owned(),
    }
}

/// Bounded human-readable trace line (projection, never authority).
#[must_use]
pub fn format_run_trace_ref(trace: &RunTraceRef) -> String {
    match &trace.operation_id {
        Some(operation) => format!(
            "task={} phase={} run={} op={operation} producer={}",
            trace.task_id, trace.phase_id, trace.run_id, trace.producer
        ),
        None => format!(
            "task={} phase={} run={} producer={}",
            trace.task_id, trace.phase_id, trace.run_id, trace.producer
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RunIdentityInput, create_operation_id, create_run_id,
        create_run_trace_ref, format_run_trace_ref,
    };
    use crate::runtime::RuntimeError;

    #[test]
    fn run_ids_are_deterministic_and_domain_separated() {
        // Pinned from the differential oracle probe
        // (tests/differential/probes/runtime-readiness-identity-oracle.mjs,
        // op "run-id") over packages/core/src/runtime/identity.ts.
        let pinned = create_run_id(&RunIdentityInput {
            task_id: "task-7",
            phase_id: "mutation",
            sequence: 3,
            kind: None,
        })
        .expect("valid identity");
        assert_eq!(pinned, "run_runtime_2fe2ef8aa58af47585603d7d");
        let first = create_run_id(&RunIdentityInput {
            task_id: "task-1",
            phase_id: "mutation",
            sequence: 3,
            kind: None,
        })
        .expect("valid identity");
        assert!(first.starts_with("run_runtime_"));
        assert_eq!(first.len(), "run_runtime_".len() + 24);
        let again = create_run_id(&RunIdentityInput {
            task_id: "task-1",
            phase_id: "mutation",
            sequence: 3,
            kind: Some("runtime"),
        })
        .expect("valid identity");
        assert_eq!(first, again);
        let other = create_run_id(&RunIdentityInput {
            task_id: "task-1",
            phase_id: "mutation",
            sequence: 4,
            kind: None,
        })
        .expect("valid identity");
        assert_ne!(first, other);
    }

    #[test]
    fn run_identity_rejects_empty_ids_and_zero_sequence() {
        assert_eq!(
            create_run_id(&RunIdentityInput {
                task_id: "",
                phase_id: "mutation",
                sequence: 1,
                kind: None,
            }),
            Err(RuntimeError {
                message: "A run identity requires a task id and a phase id."
                    .to_owned()
            })
        );
        assert_eq!(
            create_run_id(&RunIdentityInput {
                task_id: "task-1",
                phase_id: "",
                sequence: 1,
                kind: None,
            }),
            Err(RuntimeError {
                message: "A run identity requires a task id and a phase id."
                    .to_owned()
            })
        );
        assert_eq!(
            create_run_id(&RunIdentityInput {
                task_id: "task-1",
                phase_id: "mutation",
                sequence: 0,
                kind: None,
            }),
            Err(RuntimeError {
                message: "A run sequence must be a positive safe integer."
                    .to_owned()
            })
        );
    }

    #[test]
    fn operation_ids_are_stable_per_run_and_operation() {
        let run = create_run_id(&RunIdentityInput {
            task_id: "t",
            phase_id: "p",
            sequence: 1,
            kind: None,
        })
        .expect("valid identity");
        assert_eq!(
            create_operation_id(&run, "checkpoint"),
            create_operation_id(&run, "checkpoint")
        );
        assert_ne!(
            create_operation_id(&run, "checkpoint"),
            create_operation_id(&run, "apply")
        );
    }

    #[test]
    fn trace_refs_render_with_or_without_operations() {
        let with_op = create_run_trace_ref(
            "task-1",
            "mutation",
            "run_runtime_abc",
            Some("op_def"),
            "process-supervisor",
        );
        assert_eq!(
            format_run_trace_ref(&with_op),
            "task=task-1 phase=mutation run=run_runtime_abc op=op_def producer=process-supervisor"
        );
        let without_op = create_run_trace_ref(
            "task-1",
            "mutation",
            "run_runtime_abc",
            None,
            "artifact-capture",
        );
        assert_eq!(
            format_run_trace_ref(&without_op),
            "task=task-1 phase=mutation run=run_runtime_abc producer=artifact-capture"
        );
    }
}
