import { computeArtifactDigest, type ArtifactDigest } from "../identity/artifact-digest.js";

/**
 * Formal context classes and PhaseContract (Stage 3 — Interpretable
 * Context Architecture, ADR 0030).
 *
 * Context classes formalize the categories Siralos already approximates:
 * global (execution identity/rules), routing (planning/scope/routing),
 * phase_contract (current operation requirements), stable_reference
 * (guidance/architecture/instructions), working (plan/source/evidence).
 * No phase defaults to repository-wide context.
 *
 * A PhaseContract DECLARES one bounded phase's inputs, authority ceiling,
 * operations, outputs, and verification. It is not a state machine —
 * TaskState remains authoritative for workflow progress — and it can
 * only NARROW authority: the authority profile is a fixed vocabulary,
 * so a malicious contract can never broaden runtime capability.
 * ToolProjector/security enforcement remain authoritative.
 */

export type ContextClass = "global" | "routing" | "phase_contract" | "stable_reference" | "working";

export const CONTEXT_CLASSES: readonly ContextClass[] = [
  "global",
  "routing",
  "phase_contract",
  "stable_reference",
  "working",
] as const;

export interface ContextClassArtifactKinds {
  readonly global: readonly string[];
  readonly routing: readonly string[];
  readonly phase_contract: readonly string[];
  readonly stable_reference: readonly string[];
  readonly working: readonly string[];
}

/** Bounded artifact-kind vocabulary per context class (by reference). */
export const CONTEXT_CLASS_ARTIFACT_KINDS: ContextClassArtifactKinds = {
  global: ["ExecutionContract", "RuntimeIdentity"],
  routing: [
    "PlanningPolicy",
    "WorkspaceScope",
    "ActiveWorkingSet",
    "DocumentationSelection",
    "ToolProjection",
  ],
  phase_contract: ["PhaseContract"],
  stable_reference: [
    "ScopedAgents",
    "ArchitectureDocs",
    "ApplicableAdrs",
    "ProjectInstructions",
    "ProjectKnowledge",
    "References",
  ],
  working: [
    "TaskPlan",
    "ActiveWorkingSetFiles",
    "CurrentSourceRevisions",
    "PreparedMutations",
    "ValidationEvidence",
    "ReviewFindings",
  ],
};

export function classArtifactKinds(contextClass: ContextClass): readonly string[] {
  return CONTEXT_CLASS_ARTIFACT_KINDS[contextClass];
}

// ---------------------------------------------------------------------------
// PhaseContract
// ---------------------------------------------------------------------------

export type PhaseContractId =
  | "planning"
  | "inspection"
  | "preparation"
  | "approval"
  | "mutation"
  | "verification"
  | "validation"
  | "impact"
  | "review"
  | "repair"
  | "acceptance";

export interface PhaseInputRequirement {
  readonly artifactType: string;
  readonly optional: boolean;
  readonly reason: string;
}

/**
 * Fixed authority vocabulary — the only authority shapes a PhaseContract
 * may declare. A contract can narrow (e.g. read-only) but never broaden.
 */
export interface PhaseAuthorityProfile {
  readonly readOnly: boolean;
  readonly mutation: "none" | "prepared_only";
  readonly approvalGrant: boolean;
  readonly acceptanceAuthority: boolean;
  readonly capabilityNarrowing: readonly string[];
}

export interface PhaseOperation {
  readonly id: string;
  readonly description: string;
}

export interface PhaseOutputRequirement {
  readonly artifactType: string;
  readonly verificationKind: "deterministic" | "host_verified" | "review";
}

export interface PhaseVerificationRequirement {
  readonly id: string;
  readonly description: string;
  readonly evidenceClass: string;
}

export interface PhaseContract {
  readonly id: PhaseContractId;
  readonly version: number;
  readonly phase: string;
  readonly inputs: readonly PhaseInputRequirement[];
  readonly authority: PhaseAuthorityProfile;
  readonly process: readonly PhaseOperation[];
  readonly outputs: readonly PhaseOutputRequirement[];
  readonly verification: readonly PhaseVerificationRequirement[];
  readonly contextClasses: readonly ContextClass[];
  readonly digest: ArtifactDigest;
}

export interface CreatePhaseContractInput {
  readonly id: PhaseContractId;
  readonly version: number;
  readonly phase: string;
  readonly inputs: readonly PhaseInputRequirement[];
  readonly authority: PhaseAuthorityProfile;
  readonly process: readonly PhaseOperation[];
  readonly outputs: readonly PhaseOutputRequirement[];
  readonly verification: readonly PhaseVerificationRequirement[];
  readonly contextClasses: readonly ContextClass[];
}

export function createPhaseContract(input: CreatePhaseContractInput): PhaseContract {
  if (input.id.length === 0) {
    throw new Error("A PhaseContract requires an id.");
  }
  if (!Number.isSafeInteger(input.version) || input.version < 1) {
    throw new Error("A PhaseContract version must be a positive safe integer.");
  }
  if (input.inputs.length === 0) {
    throw new Error(`PhaseContract ${input.id} requires at least one input.`);
  }
  if (input.outputs.length === 0) {
    throw new Error(`PhaseContract ${input.id} requires at least one output.`);
  }
  if (input.verification.length === 0) {
    throw new Error(`PhaseContract ${input.id} requires at least one verification requirement.`);
  }
  if (input.contextClasses.length === 0) {
    throw new Error(`PhaseContract ${input.id} requires at least one context class.`);
  }
  for (const contextClass of input.contextClasses) {
    if (!CONTEXT_CLASSES.includes(contextClass)) {
      throw new Error(`PhaseContract ${input.id} declares unknown context class ${contextClass}.`);
    }
  }
  validateAuthorityProfile(input.id, input.authority);
  const digest = computeArtifactDigest({
    artifactType: "PhaseContract",
    schemaVersion: 1,
    payload: {
      id: input.id,
      version: input.version,
      phase: input.phase,
      inputs: input.inputs,
      authority: input.authority,
      process: input.process,
      outputs: input.outputs,
      verification: input.verification,
      contextClasses: input.contextClasses,
    },
  });
  return {
    id: input.id,
    version: input.version,
    phase: input.phase,
    inputs: input.inputs.map((entry) => ({ ...entry })),
    authority: {
      ...input.authority,
      capabilityNarrowing: [...input.authority.capabilityNarrowing],
    },
    process: input.process.map((entry) => ({ ...entry })),
    outputs: input.outputs.map((entry) => ({ ...entry })),
    verification: input.verification.map((entry) => ({ ...entry })),
    contextClasses: [...input.contextClasses],
    digest,
  };
}

/**
 * Authority validation: the fixed vocabulary means a malformed contract
 * (e.g. a review contract demanding unrestricted mutation) is rejected
 * structurally before it can influence anything.
 */
export function validateAuthorityProfile(
  contractId: PhaseContractId,
  authority: PhaseAuthorityProfile,
): void {
  if (typeof authority.readOnly !== "boolean") {
    throw new Error(`PhaseContract ${contractId}: readOnly must be boolean.`);
  }
  if (authority.mutation !== "none" && authority.mutation !== "prepared_only") {
    throw new Error(`PhaseContract ${contractId}: mutation must be none or prepared_only.`);
  }
  if (typeof authority.approvalGrant !== "boolean") {
    throw new Error(`PhaseContract ${contractId}: approvalGrant must be boolean.`);
  }
  if (typeof authority.acceptanceAuthority !== "boolean") {
    throw new Error(`PhaseContract ${contractId}: acceptanceAuthority must be boolean.`);
  }
  if (authority.readOnly && authority.mutation !== "none") {
    throw new Error(
      `PhaseContract ${contractId}: a read-only contract cannot declare mutation authority.`,
    );
  }
}

/** Deterministic phase → context-class mapping (host-owned table). */
export function contextClassesForPhase(phaseId: PhaseContractId): readonly ContextClass[] {
  const contract = PHASE_CONTRACTS[phaseId];
  return contract === undefined ? [] : contract.contextClasses;
}

// ---------------------------------------------------------------------------
// Phase contract registry
// ---------------------------------------------------------------------------

const readOnlyAuthority: PhaseAuthorityProfile = {
  readOnly: true,
  mutation: "none",
  approvalGrant: false,
  acceptanceAuthority: false,
  capabilityNarrowing: [],
};

export const PHASE_CONTRACTS: Readonly<Record<PhaseContractId, PhaseContract>> = {
  planning: createPhaseContract({
    id: "planning",
    version: 1,
    phase: "working",
    inputs: [
      {
        artifactType: "TaskContract",
        optional: false,
        reason: "the plan binds the exact contract",
      },
      { artifactType: "WorkspaceScope", optional: true, reason: "structural source evidence" },
      {
        artifactType: "DocumentationSelection",
        optional: true,
        reason: "applicable architecture/ADRs",
      },
      { artifactType: "References", optional: true, reason: "external reference material" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "route", description: "deterministic planning-depth routing" }],
    outputs: [{ artifactType: "TaskPlan", verificationKind: "deterministic" }],
    verification: [
      {
        id: "plan-validated",
        description: "plan candidate validated against contract and depth",
        evidenceClass: "plan_validation",
      },
    ],
    contextClasses: ["global", "routing", "stable_reference", "phase_contract"],
  }),
  inspection: createPhaseContract({
    id: "inspection",
    version: 1,
    phase: "working",
    inputs: [
      { artifactType: "WorkspaceScope", optional: false, reason: "verified/candidate files" },
      {
        artifactType: "CurrentSourceRevisions",
        optional: false,
        reason: "exact revisions of inspected files",
      },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "inspect", description: "bounded structural/semantic inspection" }],
    outputs: [{ artifactType: "InspectionEvidence", verificationKind: "host_verified" }],
    verification: [
      {
        id: "evidence-attached",
        description: "observations attached as typed evidence",
        evidenceClass: "workspace_read",
      },
    ],
    contextClasses: ["routing", "working"],
  }),
  preparation: createPhaseContract({
    id: "preparation",
    version: 1,
    phase: "working",
    inputs: [
      { artifactType: "TaskPlan", optional: false, reason: "prepared changes follow the plan" },
      { artifactType: "CurrentSourceRevisions", optional: false, reason: "exact pre-state" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "prepare", description: "read-only preparation of exact changes" }],
    outputs: [{ artifactType: "PreparedChangeset", verificationKind: "deterministic" }],
    verification: [
      {
        id: "prepared-fingerprint",
        description: "prepared identity digest bound",
        evidenceClass: "change_preview",
      },
    ],
    contextClasses: ["working", "phase_contract"],
  }),
  approval: createPhaseContract({
    id: "approval",
    version: 1,
    phase: "working",
    inputs: [
      {
        artifactType: "PreparedChangeset",
        optional: false,
        reason: "exact content being approved",
      },
      { artifactType: "TaskContract", optional: false, reason: "criteria and constraints" },
    ],
    authority: {
      readOnly: true,
      mutation: "none",
      approvalGrant: true,
      acceptanceAuthority: false,
      capabilityNarrowing: [],
    },
    process: [
      { id: "request-approval", description: "host approval request binding exact digest" },
    ],
    outputs: [{ artifactType: "ApprovalRecord", verificationKind: "host_verified" }],
    verification: [
      {
        id: "digest-bound",
        description: "approval binds the exact prepared digest",
        evidenceClass: "change_preview",
      },
    ],
    contextClasses: ["phase_contract", "working"],
  }),
  mutation: createPhaseContract({
    id: "mutation",
    version: 1,
    phase: "working",
    inputs: [
      {
        artifactType: "PreparedChangeset",
        optional: false,
        reason: "only prepared operations apply",
      },
      { artifactType: "ApprovalRecord", optional: false, reason: "exact approval required" },
    ],
    authority: {
      readOnly: false,
      mutation: "prepared_only",
      approvalGrant: false,
      acceptanceAuthority: false,
      capabilityNarrowing: [],
    },
    process: [
      { id: "checkpoint", description: "checkpoint before mutation" },
      { id: "apply", description: "hash-verified exact application" },
    ],
    outputs: [{ artifactType: "MutationResult", verificationKind: "deterministic" }],
    verification: [
      {
        id: "applied-verified",
        description: "per-surface verification after apply",
        evidenceClass: "mutation_receipt",
      },
    ],
    contextClasses: ["working"],
  }),
  verification: createPhaseContract({
    id: "verification",
    version: 1,
    phase: "validating",
    inputs: [
      { artifactType: "MutationResult", optional: false, reason: "what was applied" },
      { artifactType: "CurrentSourceRevisions", optional: false, reason: "post-apply revisions" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "verify", description: "per-surface verification (parser/LSP/semantic)" }],
    outputs: [{ artifactType: "VerificationEvidence", verificationKind: "deterministic" }],
    verification: [
      {
        id: "verified",
        description: "required verification passed",
        evidenceClass: "parser_result",
      },
    ],
    contextClasses: ["working"],
  }),
  validation: createPhaseContract({
    id: "validation",
    version: 1,
    phase: "validating",
    inputs: [
      { artifactType: "VerificationEvidence", optional: false, reason: "changed surfaces" },
      { artifactType: "ImpactRelationships", optional: true, reason: "verified impact" },
      { artifactType: "AcceptanceCriteria", optional: false, reason: "host-required minimum" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "derive-plan", description: "deterministic validation plan derivation" }],
    outputs: [{ artifactType: "ValidationPlan", verificationKind: "deterministic" }],
    verification: [
      {
        id: "required-completed",
        description: "required validation completed or honestly unavailable",
        evidenceClass: "validation_result",
      },
    ],
    contextClasses: ["working", "phase_contract"],
  }),
  impact: createPhaseContract({
    id: "impact",
    version: 1,
    phase: "working",
    inputs: [
      { artifactType: "ChangedSurfaces", optional: false, reason: "what changed" },
      { artifactType: "RelationshipIndex", optional: true, reason: "verified relationships" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "derive-impact", description: "bounded impact analysis" }],
    outputs: [{ artifactType: "ReviewContextManifest", verificationKind: "host_verified" }],
    verification: [
      {
        id: "impact-derived",
        description: "impact manifest derived from evidence",
        evidenceClass: "validation_result",
      },
    ],
    contextClasses: ["working"],
  }),
  review: createPhaseContract({
    id: "review",
    version: 1,
    phase: "reviewing",
    inputs: [
      { artifactType: "TaskContract", optional: false, reason: "criteria" },
      { artifactType: "Changeset", optional: false, reason: "exact change under review" },
      { artifactType: "ReviewContextManifest", optional: true, reason: "impact context" },
      { artifactType: "ValidationEvidence", optional: false, reason: "relevant evidence" },
      { artifactType: "CurrentSourceRevisions", optional: false, reason: "relevant source" },
    ],
    authority: readOnlyAuthority,
    process: [{ id: "review", description: "fresh read-only review" }],
    outputs: [{ artifactType: "ReviewVerdict", verificationKind: "review" }],
    verification: [
      {
        id: "verdict-bound",
        description: "verdict bound to review input digest",
        evidenceClass: "review_result",
      },
    ],
    contextClasses: ["working", "stable_reference", "phase_contract"],
  }),
  repair: createPhaseContract({
    id: "repair",
    version: 1,
    phase: "working",
    inputs: [
      { artifactType: "ReviewFindings", optional: false, reason: "blocking findings" },
      { artifactType: "CurrentSourceRevisions", optional: false, reason: "current revisions" },
      { artifactType: "AcceptanceCriteria", optional: false, reason: "affected criteria" },
    ],
    authority: {
      readOnly: false,
      mutation: "prepared_only",
      approvalGrant: false,
      acceptanceAuthority: false,
      capabilityNarrowing: [],
    },
    process: [{ id: "re-prepare", description: "fresh preparation from current revisions" }],
    outputs: [{ artifactType: "PreparedChangeset", verificationKind: "deterministic" }],
    verification: [
      {
        id: "fresh-artifacts",
        description: "repair uses fresh revisions/approvals only",
        evidenceClass: "change_preview",
      },
    ],
    contextClasses: ["working"],
  }),
  acceptance: createPhaseContract({
    id: "acceptance",
    version: 1,
    phase: "reviewing",
    inputs: [
      { artifactType: "AcceptanceCriteria", optional: false, reason: "requirements" },
      {
        artifactType: "ValidationEvidence",
        optional: false,
        reason: "current evidence identities",
      },
      { artifactType: "ReviewVerdict", optional: false, reason: "required review verdict" },
      {
        artifactType: "MutationVerificationEvidence",
        optional: false,
        reason: "mutation verification",
      },
    ],
    authority: {
      readOnly: true,
      mutation: "none",
      approvalGrant: false,
      acceptanceAuthority: true,
      capabilityNarrowing: [],
    },
    process: [{ id: "evaluate", description: "deterministic acceptance evaluation" }],
    outputs: [{ artifactType: "AcceptanceResult", verificationKind: "deterministic" }],
    verification: [
      {
        id: "evidence-bound",
        description: "acceptance bound to exact evidence set",
        evidenceClass: "validation_result",
      },
    ],
    contextClasses: ["working", "phase_contract"],
  }),
};
