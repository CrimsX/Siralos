import { deepFreeze } from "../domain/deep-freeze.js";

/**
 * Standard validation profiles (executor briefing foundation).
 *
 * Standard repository validation is represented as a host-owned profile
 * reference instead of repeated prompt prose: the Execution Contract
 * points at a profile, the MilestoneManifest specifies only extra/special
 * validation, and the host executes the checks. The profile is
 * descriptive — the commands are executed by the host, never by core and
 * never by providers.
 */

export interface ValidationCheckRef {
  readonly id: string;
  /** Host-executed command for this check (descriptive only). */
  readonly command: string;
}

export interface ValidationProfile {
  readonly profileId: string;
  readonly revision: number;
  readonly checks: readonly ValidationCheckRef[];
}

/** Stable reference to one immutable validation profile revision. */
export interface ValidationProfileRef {
  readonly profileId: string;
  readonly revision: number;
}

/** Host-owned hard bounds for validation profiles. */
export const VALIDATION_PROFILE_LIMITS = Object.freeze({
  maxChecks: 32,
  maxCheckIdBytes: 64,
  maxCommandBytes: 512,
});

const CHECK_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const textEncoder = new TextEncoder();

export function validateValidationProfile(input: ValidationProfile): ValidationProfile {
  if (input.profileId.trim().length === 0) {
    throw new Error("A validation profile requires a profile id.");
  }
  if (!Number.isSafeInteger(input.revision) || input.revision < 1) {
    throw new Error("A validation profile revision must be at least 1.");
  }
  if (input.checks.length > VALIDATION_PROFILE_LIMITS.maxChecks) {
    throw new Error(
      `A validation profile accepts at most ${VALIDATION_PROFILE_LIMITS.maxChecks} checks.`,
    );
  }
  const ids = new Set<string>();
  for (const check of input.checks) {
    if (!CHECK_ID_PATTERN.test(check.id)) {
      throw new Error(`Invalid validation check id: ${check.id}`);
    }
    if (ids.has(check.id)) {
      throw new Error(`Duplicate validation check id: ${check.id}`);
    }
    ids.add(check.id);
    const command = check.command.trim();
    if (command.length === 0) {
      throw new Error(`Validation check ${check.id} requires a command.`);
    }
    if (textEncoder.encode(command).length > VALIDATION_PROFILE_LIMITS.maxCommandBytes) {
      throw new Error(
        `Validation check ${check.id} exceeds ${VALIDATION_PROFILE_LIMITS.maxCommandBytes} UTF-8 bytes.`,
      );
    }
  }
  return deepFreeze({
    profileId: input.profileId.trim(),
    revision: input.revision,
    checks: input.checks.map((check) => ({
      id: check.id,
      command: check.command.trim(),
    })),
  });
}

export function createValidationProfile(input: {
  readonly profileId: string;
  readonly checks: readonly ValidationCheckRef[];
}): ValidationProfile {
  return validateValidationProfile({
    profileId: input.profileId,
    revision: 1,
    checks: input.checks,
  });
}

/**
 * The standard repository validation profile for the Solaris repository:
 * formatting, lint, typecheck, build, unit/integration tests, behavior
 * tests, and architecture checks. Milestone manifests reference this
 * profile instead of restating the commands.
 */
export const STANDARD_REPO_VALIDATION: ValidationProfile = createValidationProfile({
  profileId: "standard-repo-validation",
  checks: [
    { id: "format", command: "npm run format:check" },
    { id: "lint", command: "npm run lint" },
    { id: "typecheck", command: "npm run typecheck" },
    { id: "test", command: "npm test" },
    { id: "behavior", command: "npm run test:behavior" },
    { id: "architecture", command: "npm run check:architecture" },
  ],
});

/** Deterministic compact description of a profile (for snapshots/reports). */
export function summarizeValidationProfile(profile: ValidationProfile): string {
  return `${profile.profileId} rev ${profile.revision} (${profile.checks.length} checks)`;
}
