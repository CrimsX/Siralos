import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import type { AcceptanceCriterionId } from "../tasks/task-contract.js";
import type { EvidenceKind } from "../tasks/task-model.js";
import type { StandardAcceptanceId } from "./standard-acceptance.js";
import { STANDARD_ACCEPTANCE_DEFINITIONS } from "./standard-acceptance.js";
import type { ValidationProfileRef } from "./validation-profile.js";

/**
 * Structured MilestoneManifest (executor briefing foundation).
 *
 * A manifest contains ONLY the requirements unique to one milestone:
 * what is being built, what must remain true, what must not be built,
 * and what observable results prove completion. It never restates Git
 * policy, standard validation commands, generic security rules, or
 * architecture principles already supplied by the Execution Contract.
 *
 * The manifest is immutable and versioned like every other Siralos
 * artifact; revision N is never mutated in place. It grants nothing:
 * there is no capability/policy surface in the model, and acceptance is
 * satisfied only by host-observed evidence through the
 * AcceptanceEvaluator — never by executor claims.
 */

export type MilestoneId = string;

export interface MilestoneRequirement {
  readonly id: string;
  readonly description: string;
}

export interface MilestoneDeliverable {
  readonly id: string;
  readonly description: string;
}

export interface MilestoneInvariant {
  readonly id: string;
  readonly description: string;
}

export interface TestRequirement {
  readonly id: string;
  readonly description: string;
}

/** Architecture concerns this milestone exercises (deterministic index tags). */
export type ArchitectureConcern = string;

/**
 * One acceptance requirement. Satisfaction is host-evidence-backed:
 * `evidenceKinds` lists the task EvidenceKinds that count, and
 * `criterionId` optionally links to a TaskContract acceptance criterion
 * whose host-verified state also satisfies it. `standardIds` reference
 * reusable definitions from the standard acceptance library instead of
 * restating them.
 */
export interface AcceptanceRequirement {
  readonly id: string;
  readonly description: string;
  /** Evidence kinds whose host-attached records satisfy this requirement. */
  readonly evidenceKinds?: readonly EvidenceKind[];
  /** Optional link to a TaskContract acceptance criterion. */
  readonly criterionId?: AcceptanceCriterionId;
  /** Optional reusable standard acceptance references. */
  readonly standardIds?: readonly StandardAcceptanceId[];
  /**
   * Optional requirements stay `not_applicable` (never a failure) when no
   * linked criterion exists for the current task.
   */
  readonly optional?: boolean;
}

export interface MilestoneRef {
  readonly id: MilestoneId;
  readonly version: number;
}

export interface MilestoneManifest {
  readonly id: MilestoneId;
  /** Immutable version identity; starts at 1 and only ever increases. */
  readonly version: number;
  readonly title: string;
  readonly goal: string;
  readonly prerequisites: readonly MilestoneRequirement[];
  readonly deliverables: readonly MilestoneDeliverable[];
  readonly nonGoals: readonly string[];
  readonly invariants: readonly MilestoneInvariant[];
  readonly acceptance: readonly AcceptanceRequirement[];
  readonly requiredTests: readonly TestRequirement[];
  /** Deterministic architecture-concern tags for context selection. */
  readonly architectureConcerns: readonly ArchitectureConcern[];
  /** Only when this milestone adds/specializes validation beyond the profile. */
  readonly validationProfile?: ValidationProfileRef;
  readonly nextMilestone?: MilestoneRef;
}

/** Stable reference to one immutable manifest version. */
export interface MilestoneManifestRef {
  readonly id: MilestoneId;
  readonly version: number;
}

/** Host-owned hard bounds for milestone manifests. */
export const MILESTONE_MANIFEST_LIMITS = Object.freeze({
  maxIdBytes: 32,
  maxTitleBytes: 256,
  maxGoalBytes: 2048,
  maxPrerequisites: 16,
  maxDeliverables: 16,
  maxNonGoals: 16,
  maxInvariants: 16,
  maxAcceptance: 32,
  maxRequiredTests: 16,
  maxEntryBytes: 512,
  maxConcerns: 12,
  maxConcernBytes: 64,
});

const MILESTONE_ID_PATTERN = /^[A-Z][A-Z0-9]{0,7}$/;
const ENTRY_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const ACCEPTANCE_ID_PATTERN = /^[A-Z][A-Z0-9][A-Z0-9._-]{0,63}$/;
const textEncoder = new TextEncoder();

export function milestoneManifestRef(manifest: MilestoneManifest): MilestoneManifestRef {
  return { id: manifest.id, version: manifest.version };
}

function validateEntry<T extends { readonly id: string; readonly description: string }>(
  kind: string,
  entry: T,
): T {
  if (!ENTRY_ID_PATTERN.test(entry.id)) {
    throw new Error(`Invalid ${kind} id: ${entry.id}`);
  }
  const description = entry.description.trim();
  if (description.length === 0) {
    throw new Error(`${kind} ${entry.id} requires a description.`);
  }
  if (textEncoder.encode(description).length > MILESTONE_MANIFEST_LIMITS.maxEntryBytes) {
    throw new Error(
      `${kind} ${entry.id} exceeds ${MILESTONE_MANIFEST_LIMITS.maxEntryBytes} UTF-8 bytes.`,
    );
  }
  return { ...entry, description };
}

function copyEntries<T extends { readonly id: string; readonly description: string }>(
  kind: string,
  values: readonly T[],
  max: number,
): T[] {
  if (values.length > max) {
    throw new Error(`${kind} accepts at most ${max} entries.`);
  }
  const ids = new Set<string>();
  for (const value of values) {
    if (ids.has(value.id)) {
      throw new Error(`Duplicate ${kind} id: ${value.id}`);
    }
    ids.add(value.id);
  }
  return values.map((value) => validateEntry(kind, value));
}

function copyStatements(kind: string, values: readonly string[], max: number): string[] {
  if (values.length > max) {
    throw new Error(`${kind} accepts at most ${max} entries.`);
  }
  return values.map((value) => {
    const text = value.trim();
    if (text.length === 0) {
      throw new Error(`${kind} entries must be non-empty.`);
    }
    if (textEncoder.encode(text).length > MILESTONE_MANIFEST_LIMITS.maxEntryBytes) {
      throw new Error(
        `${kind} entry exceeds ${MILESTONE_MANIFEST_LIMITS.maxEntryBytes} UTF-8 bytes.`,
      );
    }
    return text;
  });
}

function validateAcceptance(
  requirements: readonly AcceptanceRequirement[],
): AcceptanceRequirement[] {
  if (requirements.length > MILESTONE_MANIFEST_LIMITS.maxAcceptance) {
    throw new Error(
      `A milestone manifest accepts at most ${MILESTONE_MANIFEST_LIMITS.maxAcceptance} acceptance requirements.`,
    );
  }
  const ids = new Set<string>();
  const result: AcceptanceRequirement[] = [];
  for (const requirement of requirements) {
    if (!ACCEPTANCE_ID_PATTERN.test(requirement.id)) {
      throw new Error(`Invalid acceptance requirement id: ${requirement.id}`);
    }
    if (ids.has(requirement.id)) {
      throw new Error(`Duplicate acceptance requirement id: ${requirement.id}`);
    }
    ids.add(requirement.id);
    const description = requirement.description.trim();
    if (description.length === 0) {
      throw new Error(`Acceptance requirement ${requirement.id} requires a description.`);
    }
    if (textEncoder.encode(description).length > MILESTONE_MANIFEST_LIMITS.maxEntryBytes) {
      throw new Error(
        `Acceptance requirement ${requirement.id} exceeds ${MILESTONE_MANIFEST_LIMITS.maxEntryBytes} UTF-8 bytes.`,
      );
    }
    const evidenceKinds =
      requirement.evidenceKinds === undefined ? [] : [...requirement.evidenceKinds];
    const standardIds = requirement.standardIds === undefined ? [] : [...requirement.standardIds];
    if (
      evidenceKinds.length === 0 &&
      requirement.criterionId === undefined &&
      standardIds.length === 0
    ) {
      throw new Error(
        `Acceptance requirement ${requirement.id} must declare evidenceKinds, a criterionId, or standardIds.`,
      );
    }
    for (const standardId of standardIds) {
      if (!(standardId in STANDARD_ACCEPTANCE_DEFINITIONS)) {
        throw new Error(
          `Acceptance requirement ${requirement.id} references unknown standard acceptance ${standardId}.`,
        );
      }
    }
    result.push({
      id: requirement.id,
      description,
      ...(evidenceKinds.length > 0 ? { evidenceKinds } : {}),
      ...(requirement.criterionId === undefined ? {} : { criterionId: requirement.criterionId }),
      ...(standardIds.length > 0 ? { standardIds } : {}),
      ...(requirement.optional === true ? { optional: true } : {}),
    });
  }
  return result;
}

function validateConcerns(concerns: readonly ArchitectureConcern[]): string[] {
  if (concerns.length > MILESTONE_MANIFEST_LIMITS.maxConcerns) {
    throw new Error(
      `A milestone manifest accepts at most ${MILESTONE_MANIFEST_LIMITS.maxConcerns} architecture concerns.`,
    );
  }
  const seen = new Set<string>();
  for (const concern of concerns) {
    const text = concern.trim();
    if (text.length === 0) {
      throw new Error("Architecture concerns must be non-empty.");
    }
    if (textEncoder.encode(text).length > MILESTONE_MANIFEST_LIMITS.maxConcernBytes) {
      throw new Error(
        `An architecture concern exceeds ${MILESTONE_MANIFEST_LIMITS.maxConcernBytes} UTF-8 bytes.`,
      );
    }
    if (seen.has(text)) {
      throw new Error(`Duplicate architecture concern: ${text}`);
    }
    seen.add(text);
  }
  return concerns.map((concern) => concern.trim());
}

interface ManifestShape {
  readonly id: MilestoneId;
  readonly version: number;
  readonly title: string;
  readonly goal: string;
  readonly prerequisites: readonly MilestoneRequirement[];
  readonly deliverables: readonly MilestoneDeliverable[];
  readonly nonGoals: readonly string[];
  readonly invariants: readonly MilestoneInvariant[];
  readonly acceptance: readonly AcceptanceRequirement[];
  readonly requiredTests: readonly TestRequirement[];
  readonly architectureConcerns: readonly ArchitectureConcern[];
  readonly validationProfile?: ValidationProfileRef;
  readonly nextMilestone?: MilestoneRef;
}

function validateManifestShape(input: ManifestShape): MilestoneManifest {
  if (!MILESTONE_ID_PATTERN.test(input.id)) {
    throw new Error(`Invalid milestone id: ${input.id}`);
  }
  if (!Number.isSafeInteger(input.version) || input.version < 1) {
    throw new Error("A milestone manifest version must be at least 1.");
  }
  const title = input.title.trim();
  if (title.length === 0) {
    throw new Error("A milestone manifest requires a title.");
  }
  if (textEncoder.encode(title).length > MILESTONE_MANIFEST_LIMITS.maxTitleBytes) {
    throw new Error(
      `A milestone title exceeds ${MILESTONE_MANIFEST_LIMITS.maxTitleBytes} UTF-8 bytes.`,
    );
  }
  const goal = input.goal.trim();
  if (goal.length === 0) {
    throw new Error("A milestone manifest requires a goal.");
  }
  if (textEncoder.encode(goal).length > MILESTONE_MANIFEST_LIMITS.maxGoalBytes) {
    throw new Error(
      `A milestone goal exceeds ${MILESTONE_MANIFEST_LIMITS.maxGoalBytes} UTF-8 bytes.`,
    );
  }
  if (input.acceptance.length === 0) {
    throw new Error("A milestone manifest requires at least one acceptance requirement.");
  }
  if (input.validationProfile !== undefined) {
    if (
      !Number.isSafeInteger(input.validationProfile.revision) ||
      input.validationProfile.revision < 1
    ) {
      throw new Error("A validation profile revision must be at least 1.");
    }
    if (input.validationProfile.profileId.trim().length === 0) {
      throw new Error("A validation profile requires a profile id.");
    }
  }
  return deepFreeze({
    id: input.id,
    version: input.version,
    title,
    goal,
    prerequisites: copyEntries(
      "prerequisite",
      input.prerequisites,
      MILESTONE_MANIFEST_LIMITS.maxPrerequisites,
    ),
    deliverables: copyEntries(
      "deliverable",
      input.deliverables,
      MILESTONE_MANIFEST_LIMITS.maxDeliverables,
    ),
    nonGoals: copyStatements("non-goal", input.nonGoals, MILESTONE_MANIFEST_LIMITS.maxNonGoals),
    invariants: copyEntries("invariant", input.invariants, MILESTONE_MANIFEST_LIMITS.maxInvariants),
    acceptance: validateAcceptance(input.acceptance),
    requiredTests: copyEntries(
      "test requirement",
      input.requiredTests,
      MILESTONE_MANIFEST_LIMITS.maxRequiredTests,
    ),
    architectureConcerns: validateConcerns(input.architectureConcerns),
    ...(input.validationProfile === undefined
      ? {}
      : {
          validationProfile: {
            profileId: input.validationProfile.profileId.trim(),
            revision: input.validationProfile.revision,
          },
        }),
    ...(input.nextMilestone === undefined ? {} : { nextMilestone: { ...input.nextMilestone } }),
  });
}

export interface CreateMilestoneManifestInput {
  readonly id: MilestoneId;
  readonly title: string;
  readonly goal: string;
  readonly prerequisites?: readonly MilestoneRequirement[];
  readonly deliverables?: readonly MilestoneDeliverable[];
  readonly nonGoals?: readonly string[];
  readonly invariants?: readonly MilestoneInvariant[];
  readonly acceptance: readonly AcceptanceRequirement[];
  readonly requiredTests?: readonly TestRequirement[];
  readonly architectureConcerns?: readonly ArchitectureConcern[];
  readonly validationProfile?: ValidationProfileRef;
  readonly nextMilestone?: MilestoneRef;
}

/** Create the first immutable manifest version. */
export function createMilestoneManifest(input: CreateMilestoneManifestInput): MilestoneManifest {
  return validateManifestShape({
    id: input.id,
    version: 1,
    title: input.title,
    goal: input.goal,
    prerequisites: input.prerequisites ?? [],
    deliverables: input.deliverables ?? [],
    nonGoals: input.nonGoals ?? [],
    invariants: input.invariants ?? [],
    acceptance: input.acceptance,
    requiredTests: input.requiredTests ?? [],
    architectureConcerns: input.architectureConcerns ?? [],
    ...(input.validationProfile === undefined
      ? {}
      : { validationProfile: input.validationProfile }),
    ...(input.nextMilestone === undefined ? {} : { nextMilestone: input.nextMilestone }),
  });
}

/** Validate and detach a manifest at a runtime boundary. */
export function validateMilestoneManifest(input: MilestoneManifest): MilestoneManifest {
  return validateManifestShape({
    id: input.id,
    version: input.version,
    title: input.title,
    goal: input.goal,
    prerequisites: input.prerequisites,
    deliverables: input.deliverables,
    nonGoals: input.nonGoals,
    invariants: input.invariants,
    acceptance: input.acceptance,
    requiredTests: input.requiredTests,
    architectureConcerns: input.architectureConcerns,
    ...(input.validationProfile === undefined
      ? {}
      : { validationProfile: input.validationProfile }),
    ...(input.nextMilestone === undefined ? {} : { nextMilestone: input.nextMilestone }),
  });
}

export interface ReviseMilestoneManifestInput {
  readonly title?: string;
  readonly goal?: string;
  readonly prerequisites?: readonly MilestoneRequirement[];
  readonly deliverables?: readonly MilestoneDeliverable[];
  readonly nonGoals?: readonly string[];
  readonly invariants?: readonly MilestoneInvariant[];
  readonly acceptance?: readonly AcceptanceRequirement[];
  readonly requiredTests?: readonly TestRequirement[];
  readonly architectureConcerns?: readonly ArchitectureConcern[];
  readonly validationProfile?: ValidationProfileRef;
  readonly nextMilestone?: MilestoneRef;
}

/** Produce the next immutable manifest version; the previous object is untouched. */
export function reviseMilestoneManifest(
  previous: MilestoneManifest,
  changes: ReviseMilestoneManifestInput,
): MilestoneManifest {
  if (
    !Number.isSafeInteger(previous.version) ||
    previous.version < 1 ||
    previous.version >= Number.MAX_SAFE_INTEGER
  ) {
    throw new Error("A previous manifest version must be an incrementable safe integer.");
  }
  return validateManifestShape({
    id: previous.id,
    version: previous.version + 1,
    title: changes.title ?? previous.title,
    goal: changes.goal ?? previous.goal,
    prerequisites: changes.prerequisites ?? previous.prerequisites,
    deliverables: changes.deliverables ?? previous.deliverables,
    nonGoals: changes.nonGoals ?? previous.nonGoals,
    invariants: changes.invariants ?? previous.invariants,
    acceptance: changes.acceptance ?? previous.acceptance,
    requiredTests: changes.requiredTests ?? previous.requiredTests,
    architectureConcerns: changes.architectureConcerns ?? previous.architectureConcerns,
    ...(changes.validationProfile === undefined
      ? previous.validationProfile === undefined
        ? {}
        : { validationProfile: previous.validationProfile }
      : { validationProfile: changes.validationProfile }),
    ...(changes.nextMilestone === undefined
      ? previous.nextMilestone === undefined
        ? {}
        : { nextMilestone: previous.nextMilestone }
      : { nextMilestone: changes.nextMilestone }),
  });
}

/** Deterministic digest over a manifest version (canonical JSON). */
export function computeMilestoneManifestDigest(manifest: MilestoneManifest): string {
  return sha256Hex(canonicalizeJson(manifest));
}
