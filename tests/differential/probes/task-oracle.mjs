/**
 * Task-contract oracle probe (differential harness, ADR 0033, Stage 3R R3).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes the scenario against the REAL TypeScript reference task
 * runtime (packages/core/src/tasks) and prints the canonical R3
 * observation object as JSON on stdout. This is a thin scenario adapter:
 * it does not reimplement runtime behavior.
 *
 * Deterministic: all timestamps come from the scenario input; no ambient
 * clock, no randomness, no environment access.
 */
import { readFileSync } from "node:fs";
import { createTaskContract } from "../../../packages/core/src/tasks/task-contract.js";
import { createTaskRuntime } from "../../../packages/core/src/tasks/task-runtime.js";
import { createTaskRuntimeSnapshot } from "../../../packages/core/src/tasks/task-snapshot.js";

const MAX_INPUT_BYTES = 8 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

/** Map a reference validation message to the stable R3 code vocabulary. */
function contractCodeFromMessage(message) {
  if (message.includes("Invalid task contract id")) return "invalid_task_contract_id";
  if (message.includes("non-empty request")) return "empty_request";
  if (message.includes("request exceeds")) return "request_too_large";
  if (message.includes("revision must be at least 1")) return "invalid_revision";
  if (message.includes("accepts at most 64 acceptance criteria")) {
    return "too_many_acceptance_criteria";
  }
  if (message.includes("accepts at most 32 constraints")) return "too_many_constraints";
  if (message.includes("Invalid acceptance criterion id")) return "invalid_criterion_id";
  if (message.includes("Duplicate acceptance criterion id")) return "duplicate_criterion_id";
  if (message.includes("requires at least one acceptance criterion")) {
    return "no_acceptance_criteria";
  }
  if (message.includes("acceptance criterion") && message.includes("description exceeds")) {
    return "criterion_description_too_large";
  }
  if (message.includes("acceptance criterion") && message.includes("non-empty description")) {
    return "empty_criterion_description";
  }
  if (message.includes("invalid verification kind")) return "invalid_verification_kind";
  if (message.includes("Invalid task constraint id")) return "invalid_constraint_id";
  if (message.includes("Duplicate task constraint id")) return "duplicate_constraint_id";
  if (message.includes("task constraint") && message.includes("description exceeds")) {
    return "constraint_description_too_large";
  }
  if (message.includes("task constraint") && message.includes("non-empty description")) {
    return "empty_constraint_description";
  }
  if (message.includes("invalid kind")) return "invalid_constraint_kind";
  if (message.includes("Invalid task pause policy")) return "invalid_pause_policy";
  if (message.includes("context exceeds")) return "context_too_large";
  if (message.includes("must preserve id")) return "revision_id_mismatch";
  if (message.includes("accepts at most 128 steps")) return "too_many_steps";
  if (message.includes("Invalid task step id")) return "invalid_step_id";
  if (message.includes("Duplicate task step id")) return "duplicate_step_id";
  if (message.includes("step") && message.includes("description exceeds")) {
    return "step_description_too_large";
  }
  if (message.includes("step") && message.includes("non-empty description")) {
    return "empty_step_description";
  }
  if (message.includes("step") && message.includes("invalid kind")) return "invalid_step_kind";
  if (message.includes("accepts no evidence kinds")) return "empty_step_accepts";
  if (message.includes("invalid evidence kind")) return "invalid_step_evidence_kind";
  if (message.includes("duplicate evidence kinds")) return "duplicate_step_evidence_kind";
  return "invalid_contract";
}

/** Map a reference rejection reason to the stable R3 code vocabulary. */
function opCodeFromReason(reason) {
  if (reason.includes("can no longer be changed")) return "terminal";
  if (reason.startsWith("The task is already ")) return "already_phase";
  if (reason.startsWith("Phase transition ")) return "invalid_transition";
  if (reason.startsWith("Unknown step: ")) return "unknown_step";
  if (reason.includes("is already active")) return "step_already_active";
  if (reason.includes("is already completed")) return "step_already_completed";
  if (reason.includes("is not active")) return "step_not_active";
  if (reason.includes("requires at least one evidence reference")) {
    return "step_requires_evidence";
  }
  if (reason.startsWith("Duplicate evidence reference")) return "duplicate_evidence_ref";
  if (reason.includes("does not accept evidence kind")) return "step_rejects_evidence_kind";
  if (reason.startsWith("Unknown evidence reference for this task")) {
    return "unknown_evidence_ref";
  }
  if (reason.includes("is kind ") && reason.includes(" not ")) return "evidence_kind_mismatch";
  if (reason.startsWith("Evidence requires a non-empty id")) return "empty_evidence_id";
  if (reason.startsWith("Evidence id exceeds") || reason.includes("evidence id exceeds")) {
    return "evidence_id_too_large";
  }
  if (reason.startsWith("Evidence source exceeds") || reason.includes("byte bound")) {
    return "evidence_source_too_large";
  }
  if (reason.startsWith("Unknown evidence kind")) return "unknown_evidence_kind";
  if (reason.includes("requires source type")) return "evidence_source_kind_mismatch";
  if (reason.includes("Invalid evidence verification")) return "invalid_evidence_verification";
  if (reason.includes("must bind a task criterion or milestone")) {
    return "invalid_evidence_verification";
  }
  if (reason.includes("successful source outcome")) {
    return "passed_verification_without_successful_source";
  }
  if (reason.includes("finite JSON-serializable")) return "non_finite_evidence_source";
  if (reason.startsWith("Evidence id already attached")) return "duplicate_evidence_id";
  if (reason.includes("maximum of")) return "evidence_limit";
  if (reason.startsWith("Unknown acceptance criterion")) return "unknown_criterion";
  if (reason.includes("requires exact successful verification evidence")) {
    return "criterion_requires_verification_evidence";
  }
  if (reason.startsWith("Unknown evidence reference")) return "unknown_evidence";
  if (reason.includes("not bound to the current task contract revision")) {
    return "evidence_not_bound_to_contract";
  }
  if (reason.includes("not bound to acceptance criterion")) {
    return "evidence_not_bound_to_criterion";
  }
  if (reason.includes("successful verification outcome")) return "evidence_not_successful";
  if (reason.includes("cannot verify a")) return "evidence_kind_cannot_verify_criterion";
  if (reason.includes("at most 128 current findings")) return "too_many_findings";
  if (reason.includes("non-empty ids and sources")) return "empty_finding_field";
  if (reason.includes("cannot exceed 4096")) return "finding_field_too_large";
  if (reason.startsWith("Duplicate task finding id")) return "duplicate_finding_id";
  if (reason.startsWith("A task with id ")) return "duplicate_task";
  return "rejected";
}

function stepOpObservation(op, result) {
  if (result.status === "ok") {
    return { op, ok: true };
  }
  return { op, ok: false, code: opCodeFromReason(result.reason) };
}

function criterionObservation(op, result) {
  if (result.status === "verified") return { op, status: "verified" };
  if (result.status === "failed") return { op, status: "failed" };
  return { op, status: "rejected", code: opCodeFromReason(result.reason ?? "") };
}

function dispositionJson(disposition) {
  if (disposition.type === "continue") {
    return disposition.nextAction === undefined
      ? { type: "continue" }
      : { type: "continue", nextAction: disposition.nextAction };
  }
  if (disposition.type === "complete") return { type: "complete" };
  return { type: "blocked", reason: disposition.reason };
}

/** Build the canonical per-event observation for an activity record. */
function activityJson(event) {
  const base = { type: event.type, sequence: event.sequence };
  switch (event.type) {
    case "task_started":
      return Object.assign({}, base, { contractRevision: event.contractRevision });
    case "task_phase_changed":
      return Object.assign({}, base, { phase: event.phase });
    case "step_started":
      return Object.assign({}, base, { stepId: event.stepId });
    case "step_completed":
      return Object.assign({}, base, {
        stepId: event.stepId,
        evidenceRefs: event.evidenceRefs.map((reference) => ({
          evidenceId: reference.evidenceId,
          kind: reference.kind,
        })),
      });
    case "step_failed":
      return Object.assign({}, base, { stepId: event.stepId, reason: event.reason });
    case "evidence_attached":
      return Object.assign({}, base, {
        evidenceId: event.evidenceId,
        kind: event.kind,
      });
    case "criterion_verified":
      return Object.assign({}, base, {
        criterionId: event.criterionId,
        verifiedBy: event.verifiedBy,
      });
    case "task_blocked":
    case "task_cancelled":
    case "task_failed":
      return Object.assign({}, base, { reason: event.reason });
    case "task_contract_revised":
      return Object.assign({}, base, { revision: event.revision });
    case "disposition_submitted":
      return Object.assign({}, base, {
        disposition: dispositionJson(event.disposition),
        source: event.source,
        accepted: event.accepted,
        note: event.note ?? null,
      });
    default:
      return base;
  }
}

function run(input) {
  const now = () => input.now;
  const results = [];

  let contract;
  try {
    contract = createTaskContract(input.contract);
  } catch (error) {
    return {
      rejected: true,
      code: contractCodeFromMessage(error instanceof Error ? error.message : String(error)),
    };
  }
  const steps = (input.steps ?? []).map((spec) => ({ ...spec }));

  const runtime = createTaskRuntime({ now });
  let handle;
  try {
    // createTask returns the task handle directly (the reference API).
    handle = runtime.createTask({
      contract,
      snapshot: createTaskRuntimeSnapshot({
        runtimeVersion: "task-runtime-1",
        provider: null,
        sandboxProfileId: null,
        capabilityPolicyRevision: null,
        workspaceIdentity: null,
        godotEngineFingerprint: null,
        workflow: null,
      }),
      steps,
      ...(input.iteration === undefined ? {} : { iteration: input.iteration }),
    });
  } catch (error) {
    return {
      rejected: true,
      code: contractCodeFromMessage(error instanceof Error ? error.message : String(error)),
    };
  }

  for (const op of input.ops ?? []) {
    switch (op.op) {
      case "transitionPhase":
        results.push(stepOpObservation("transitionPhase", handle.transitionPhase(op.phase)));
        break;
      case "beginStep":
        results.push(stepOpObservation("beginStep", handle.beginStep(op.stepId)));
        break;
      case "completeStep":
        results.push(
          stepOpObservation(
            "completeStep",
            handle.completeStep(
              op.stepId,
              op.refs.map((reference) => ({ ...reference })),
            ),
          ),
        );
        break;
      case "failStep":
        results.push(stepOpObservation("failStep", handle.failStep(op.stepId, op.reason)));
        break;
      case "attachEvidence": {
        const attach = handle.attachEvidence({
          id: op.id,
          kind: op.kind,
          source: op.source,
          // The reference verification type requires an explicit
          // milestone field; the R3 scenarios always bind a criterion.
          ...(op.verification === null || op.verification === undefined
            ? {}
            : { verification: { ...op.verification, milestone: null } }),
        });
        if (attach.status === "attached") {
          results.push({ op: "attachEvidence", ok: true });
        } else {
          results.push({
            op: "attachEvidence",
            ok: false,
            code: opCodeFromReason(attach.reason ?? ""),
          });
        }
        break;
      }
      case "verifyCriterion":
        results.push(
          criterionObservation(
            "verifyCriterion",
            handle.verifyCriterion(op.criterionId, op.verifiedBy ?? null, op.note ?? undefined),
          ),
        );
        break;
      case "markCriterionFailed":
        results.push(
          criterionObservation(
            "markCriterionFailed",
            handle.markCriterionFailed(op.criterionId, op.note ?? undefined),
          ),
        );
        break;
      case "setFindings":
        try {
          handle.setFindings(op.findings.map((finding) => ({ ...finding })));
          results.push({ op: "setFindings", ok: true });
        } catch (error) {
          results.push({
            op: "setFindings",
            ok: false,
            code: opCodeFromReason(error instanceof Error ? error.message : String(error)),
          });
        }
        break;
      case "setValidationStatus":
        handle.setValidationStatus(op.status);
        results.push({ op: "setValidationStatus", ok: true });
        break;
      case "setReviewStatus":
        handle.setReviewStatus(op.status);
        results.push({ op: "setReviewStatus", ok: true });
        break;
      case "setIteration":
        handle.setIteration(op.iteration);
        results.push({ op: "setIteration", ok: true });
        break;
      case "reviseContract":
        try {
          const changes = { id: op.changes.id };
          if (op.changes.request !== undefined) changes.request = op.changes.request;
          if (op.changes.context !== undefined) changes.context = op.changes.context;
          if (op.changes.constraints !== undefined) {
            changes.constraints = op.changes.constraints.map((entry) => ({ ...entry }));
          }
          if (op.changes.acceptanceCriteria !== undefined) {
            changes.acceptanceCriteria = op.changes.acceptanceCriteria.map((entry) => ({
              ...entry,
            }));
          }
          if (op.changes.pausePolicy !== undefined) {
            changes.pausePolicy = op.changes.pausePolicy;
          }
          const revision = handle.reviseContract(changes);
          results.push({ op: "reviseContract", ok: true, revision: revision.revision });
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          const code = message.includes("can no longer be changed")
            ? "terminal"
            : contractCodeFromMessage(message);
          results.push({ op: "reviseContract", ok: false, code });
        }
        break;
      case "submitDisposition": {
        const result = handle.submitDisposition(op.disposition, op.source ?? "host");
        if (result.accepted) {
          results.push({ op: "submitDisposition", accepted: true });
        } else {
          const code =
            op.disposition.type === "complete"
              ? "completion_gate"
              : opCodeFromReason(result.reason ?? "");
          results.push({
            op: "submitDisposition",
            accepted: false,
            code,
          });
        }
        break;
      }
      case "completeTask": {
        const result = handle.completeTask();
        if (result.status === "completed") {
          results.push({ op: "completeTask", status: "completed" });
        } else {
          results.push({ op: "completeTask", status: "rejected", missing: result.reasons });
        }
        break;
      }
      case "cancel":
        handle.cancel(op.reason);
        results.push({ op: "cancel", ok: true });
        break;
      case "fail":
        handle.fail(op.reason);
        results.push({ op: "fail", ok: true });
        break;
      case "markBlocked":
        handle.markBlocked(op.reason);
        results.push({ op: "markBlocked", ok: true });
        break;
      case "observe":
        handle.observe({
          action: op.action,
          fingerprint: op.fingerprint,
          ...(op.progress === undefined ? {} : { progress: op.progress }),
        });
        results.push({ op: "observe", ok: true });
        break;
      default:
        throw new Error("unknown op: " + String(op.op));
    }
  }

  const state = handle.snapshot();
  const completion = handle.evaluateCompletion();
  const progress = handle.progress();
  return {
    rejected: false,
    finalPhase: state.phase,
    contractRevision: state.contractRevision,
    contractDigest: state.contractDigest,
    stepStates: state.steps.map((step) => ({
      id: step.id,
      status: step.status,
      evidenceRefs: step.evidenceRefs.map((reference) => ({
        evidenceId: reference.evidenceId,
        kind: reference.kind,
      })),
    })),
    acceptance: state.acceptance.map((criterion) => ({
      criterionId: criterion.criterionId,
      status: criterion.status,
      verifiedBy: criterion.verifiedBy,
    })),
    evidenceIds: state.evidence.map((entry) => entry.id),
    validationStatus: state.validationStatus,
    reviewStatus: state.reviewStatus,
    iteration: state.iteration,
    currentFindings: state.currentFindings.map((finding) => ({
      findingId: finding.findingId,
      severity: finding.severity,
      source: finding.source,
    })),
    terminalReason: state.terminalReason,
    startedAtMs: state.startedAtMs,
    completedAtMs: state.completedAtMs,
    ops: results,
    activity: handle.activityLog().map(activityJson),
    completion: { allowed: completion.allowed, missing: completion.missing },
    progress: {
      state: progress.state,
      usefulObservations: progress.usefulObservations,
      repeatedActions: progress.repeatedActions,
    },
  };
}

try {
  const input = readStdinBounded();
  process.stdout.write(JSON.stringify(run(input)));
} catch (error) {
  const diagnostic =
    "SIRALOS_HARNESS_ERROR " +
    JSON.stringify({
      category: "HARNESS_INTERNAL_FAILURE",
      code: "PROBE_EXECUTION_FAILURE",
      message: String(error instanceof Error ? error.message : error),
    });
  process.stderr.write(diagnostic + "\n");
  process.exit(2);
}
