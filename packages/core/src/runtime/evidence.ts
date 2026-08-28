import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { sha256Hex } from "../godot/digest.js";

export const MAX_RUNTIME_EVIDENCE_STDOUT_BYTES = 1024 * 1024;
export const MAX_RUNTIME_EVIDENCE_STDERR_BYTES = 1024 * 1024;
export const MAX_EVIDENCE_RUN_ID_BYTES = 128;
export const MAX_EVIDENCE_OPERATION_ID_BYTES = 128;

export interface RuntimeEvidenceInput {
  readonly runId: string;
  readonly operationId: string;
  readonly exitCode: number | null;
  readonly durationMs: number;
  readonly stdout: string;
  readonly stderr: string;
}

export interface RuntimeEvidence {
  readonly runId: string;
  readonly operationId: string;
  readonly exitCode: number | null;
  readonly durationMs: number;
  readonly stdout: string;
  readonly stderr: string;
  readonly truncated: boolean;
  readonly artifactDigest: string;
  readonly digest: string;
}

function boundText(text: string, maxBytes: number): { bounded: string; truncated: boolean } {
  const bytes = Buffer.byteLength(text, "utf8");
  if (bytes <= maxBytes) return { bounded: text, truncated: false };
  let end = 0;
  let byteLen = 0;
  for (const ch of text) {
    const chBytes = Buffer.byteLength(ch, "utf8");
    if (byteLen + chBytes > maxBytes) break;
    byteLen += chBytes;
    end += ch.length;
  }
  return { bounded: text.slice(0, end), truncated: true };
}

function validateInput(input: RuntimeEvidenceInput): void {
  if (input.runId.length === 0) throw new Error("A runtime evidence requires a run id.");
  if (Buffer.byteLength(input.runId, "utf8") > MAX_EVIDENCE_RUN_ID_BYTES)
    throw new Error(
      `The runtime evidence run id exceeds the ${MAX_EVIDENCE_RUN_ID_BYTES}-byte bound.`,
    );
  if (input.operationId.length === 0)
    throw new Error("A runtime evidence requires an operation id.");
  if (Buffer.byteLength(input.operationId, "utf8") > MAX_EVIDENCE_OPERATION_ID_BYTES)
    throw new Error(
      `The runtime evidence operation id exceeds the ${MAX_EVIDENCE_OPERATION_ID_BYTES}-byte bound.`,
    );
}

export function createRuntimeEvidence(input: RuntimeEvidenceInput): RuntimeEvidence {
  validateInput(input);
  const stdoutBound = boundText(input.stdout, MAX_RUNTIME_EVIDENCE_STDOUT_BYTES);
  const stderrBound = boundText(input.stderr, MAX_RUNTIME_EVIDENCE_STDERR_BYTES);
  const truncated = stdoutBound.truncated || stderrBound.truncated;
  const stdout = stdoutBound.bounded;
  const stderr = stderrBound.bounded;
  const artifactBytes = Buffer.concat([
    Buffer.from(stdout, "utf8"),
    Buffer.from("\n", "utf8"),
    Buffer.from(stderr, "utf8"),
  ]);
  const artifactDigest = sha256Hex(artifactBytes.toString("utf8"));
  const exitCodeValue: string | null =
    input.exitCode === null || input.exitCode === undefined ? null : String(input.exitCode);
  const payload: Record<string, unknown> = {
    runId: input.runId,
    operationId: input.operationId,
    exitCode: exitCodeValue,
    durationMs: input.durationMs,
    stdout,
    stderr,
    truncated,
    artifactDigest,
  };
  const digest = computeArtifactDigest({
    artifactType: "RuntimeEvidence",
    schemaVersion: 1,
    payload,
  }).value;
  return {
    runId: input.runId,
    operationId: input.operationId,
    exitCode: input.exitCode,
    durationMs: input.durationMs,
    stdout,
    stderr,
    truncated,
    artifactDigest,
    digest,
  };
}

export function renderRuntimeEvidence(evidence: RuntimeEvidence): string {
  const exit =
    evidence.exitCode === null || evidence.exitCode === undefined
      ? "n/a"
      : String(evidence.exitCode);
  return `run ${evidence.runId} op ${evidence.operationId} exit=${exit} duration=${evidence.durationMs}ms truncated=${evidence.truncated} digest=${evidence.artifactDigest.slice(0, 12)}`;
}
