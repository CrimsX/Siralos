import { canonicalizeJson, sha256Hex } from "../digest.js";
import { deepFreeze } from "../../domain/deep-freeze.js";
import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";
import type { MutationOperation, SemanticExpectation } from "./operations.js";

/**
 * Prepared mutation artifact (Stage 3 milestone 10, ADR 0026).
 *
 * Every native mutation becomes an immutable prepared artifact binding:
 * the exact target revision, the exact operation set, the complete
 * preview, the expected semantic effect, and a deterministic fingerprint.
 * Approval binds the fingerprint; changing the target, revision,
 * operations, or prepared output produces a new identity and invalidates
 * any old approval. A prepared mutation is NOT approval and NOT an apply.
 */

export interface GodotMutationPreview {
  /** Bounded structural summary of the operations (reviewable text). */
  readonly structuralSummary: string;
  /** Complete unified diff of the serialized before/after document. */
  readonly diff: string;
}

export interface PreparedGodotMutation {
  readonly targetPath: string;
  readonly sourceRevision: WorkspaceRevisionHandle;
  /** SHA-256 of the exact rev_A source text (apply precondition). */
  readonly sourceSha256: string;
  readonly kind: "scene" | "resource";
  readonly operations: readonly MutationOperation[];
  readonly expectedSemanticEffect: readonly SemanticExpectation[];
  readonly preview: GodotMutationPreview;
  /** Deterministic identity; approval binds this exact value. */
  readonly fingerprint: string;
  /** The deterministic serialized after-text (apply content). */
  readonly serializedAfter: string;
  /** Preview line counts (checkpoint metadata; complete diff already verified). */
  readonly addedLines: number;
  readonly removedLines: number;
}

export interface CreatePreparedGodotMutationInput {
  readonly targetPath: string;
  readonly sourceRevision: WorkspaceRevisionHandle;
  readonly sourceSha256: string;
  readonly kind: "scene" | "resource";
  readonly operations: readonly MutationOperation[];
  readonly expectedSemanticEffect: readonly SemanticExpectation[];
  readonly preview: GodotMutationPreview;
  readonly serializedAfter: string;
  /** Preview line counts (checkpoint metadata; complete diff already verified). */
  readonly addedLines: number;
  readonly removedLines: number;
}

const REVISION_HANDLE_PATTERN = /^rev_[0-9a-f]{32}$/;
const textEncoder = new TextEncoder();

function requireBounded(text: string, maxBytes: number, field: string): string {
  const value = text.trim();
  if (value.length === 0) {
    throw new Error(`${field} must not be empty.`);
  }
  if (textEncoder.encode(value).length > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} UTF-8 bytes.`);
  }
  return value;
}

/** Deterministic identity over the exact prepared mutation content. */
export function computeMutationFingerprint(input: {
  readonly targetPath: string;
  readonly sourceRevision: WorkspaceRevisionHandle;
  readonly sourceSha256: string;
  readonly kind: "scene" | "resource";
  readonly operations: readonly MutationOperation[];
  readonly serializedAfter: string;
}): string {
  return sha256Hex(
    canonicalizeJson({
      targetPath: input.targetPath,
      sourceRevision: input.sourceRevision,
      sourceSha256: input.sourceSha256,
      kind: input.kind,
      operations: input.operations,
      serializedAfter: input.serializedAfter,
    }),
  );
}

/**
 * Create the immutable prepared mutation. Host-owned identity: the
 * fingerprint is computed over the exact content so any material change
 * (revision, operations, target, serialized output) yields a new
 * identity that old approvals cannot satisfy.
 */
export function createPreparedGodotMutation(
  input: CreatePreparedGodotMutationInput,
): PreparedGodotMutation {
  const targetPath = requireBounded(input.targetPath, 1024, "A target path");
  if (!REVISION_HANDLE_PATTERN.test(input.sourceRevision)) {
    throw new Error(`A prepared mutation requires an exact source revision handle.`);
  }
  if (!/^[0-9a-f]{64}$/.test(input.sourceSha256)) {
    throw new Error(`A prepared mutation requires a 64-hex source SHA-256.`);
  }
  if (input.kind !== "scene" && input.kind !== "resource") {
    throw new Error(`A prepared mutation requires kind scene or resource.`);
  }
  const operations = input.operations.map((operation) => operation);
  if (operations.length === 0) {
    throw new Error("A prepared mutation requires at least one operation.");
  }
  const fingerprint = computeMutationFingerprint({
    targetPath,
    sourceRevision: input.sourceRevision,
    sourceSha256: input.sourceSha256,
    kind: input.kind,
    operations,
    serializedAfter: input.serializedAfter,
  });
  return deepFreeze({
    targetPath,
    sourceRevision: input.sourceRevision,
    sourceSha256: input.sourceSha256,
    kind: input.kind,
    operations,
    expectedSemanticEffect: input.expectedSemanticEffect.map((expectation) => expectation),
    preview: {
      structuralSummary: requireBounded(
        input.preview.structuralSummary,
        8 * 1024,
        "A preview summary",
      ),
      diff: requireBounded(input.preview.diff, 64 * 1024, "A preview diff"),
    },
    fingerprint,
    serializedAfter: input.serializedAfter,
    addedLines: input.addedLines,
    removedLines: input.removedLines,
  });
}
