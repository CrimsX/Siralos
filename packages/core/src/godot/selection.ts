import type { GodotEngineProfile } from "./engine-profile.js";
import type { GodotInstallation } from "./installations.js";

/**
 * Deterministic installation-selection policy.
 *
 * Ranks (highest first): explicit CLI path, explicit CLI installation id,
 * explicit environment path, explicit environment installation id,
 * configured active installation, verified-baseline stable standard editor,
 * compatible stable standard editor, compatible stable .NET editor,
 * prerelease editor, no selection.
 *
 * Invalid installations, runtime-only binaries, and Godot 3.x are never
 * selected. Explicit selections (CLI or environment) are validated by the
 * caller before ranking: an explicit selection that fails does not silently
 * fall back. Automatic-selection rationale is always recorded.
 */
export type GodotSelectionPreference =
  | { readonly kind: "path"; readonly path: string }
  | { readonly kind: "installation-id"; readonly installationId: string }
  | { readonly kind: "config-active" }
  | { readonly kind: "auto" }
  | { readonly kind: "none" };

export interface GodotRankedCandidate {
  readonly installation: GodotInstallation;
  readonly profile: GodotEngineProfile;
  /** Lower rank wins; null means not selectable. */
  readonly rank: number | null;
}

export interface GodotSelectionOutcome {
  readonly selected: GodotRankedCandidate | null;
  /** Bounded rationale for the selection (or for no selection). */
  readonly rationale: readonly string[];
}

export const GODOT_SELECTION_RANKS = {
  explicitPath: 1,
  explicitInstallationId: 2,
  environmentPath: 3,
  environmentInstallationId: 4,
  configActive: 5,
  verifiedBaseline: 6,
  compatibleStableStandard: 7,
  compatibleStableDotnet: 8,
  prereleaseEditor: 9,
  none: 10,
} as const;

/**
 * Rank candidates deterministically using their profiles. The caller passes
 * already-deduped valid candidates with profiles; explicit selection is
 * resolved beforehand (its rank is asserted by the caller through
 * `preferenceRank`).
 */
export function rankGodotCandidates(
  candidates: readonly {
    readonly installation: GodotInstallation;
    readonly profile: GodotEngineProfile;
  }[],
): readonly GodotRankedCandidate[] {
  return candidates
    .map((candidate) => ({ ...candidate, rank: rankCandidate(candidate) }))
    .sort((left, right) => {
      const leftRank = left.rank ?? GODOT_SELECTION_RANKS.none;
      const rightRank = right.rank ?? GODOT_SELECTION_RANKS.none;
      if (leftRank !== rightRank) {
        return leftRank - rightRank;
      }
      const leftPatch = left.profile.version.patch ?? 0;
      const rightPatch = right.profile.version.patch ?? 0;
      if (leftPatch !== rightPatch) {
        return rightPatch - leftPatch;
      }
      const leftStable = left.profile.releaseChannel === "stable" ? 0 : 1;
      const rightStable = right.profile.releaseChannel === "stable" ? 0 : 1;
      if (leftStable !== rightStable) {
        return leftStable - rightStable;
      }
      const leftEdition = left.profile.edition === "standard" ? 0 : 1;
      const rightEdition = right.profile.edition === "standard" ? 0 : 1;
      if (leftEdition !== rightEdition) {
        return leftEdition - rightEdition;
      }
      return left.installation.canonicalPath.localeCompare(right.installation.canonicalPath);
    });
}

export function rankCandidate(candidate: {
  readonly installation: GodotInstallation;
  readonly profile: GodotEngineProfile;
}): number | null {
  const { profile } = candidate;
  if (profile.support === "runtime-only" || profile.support === "invalid") {
    return null;
  }
  if (profile.support === "unsupported-major") {
    return null;
  }
  if (profile.support === "verified") {
    return GODOT_SELECTION_RANKS.verifiedBaseline;
  }
  if (profile.support === "compatible-untested") {
    if (profile.releaseChannel === "stable") {
      return profile.edition === "dotnet"
        ? GODOT_SELECTION_RANKS.compatibleStableDotnet
        : GODOT_SELECTION_RANKS.compatibleStableStandard;
    }
    return GODOT_SELECTION_RANKS.prereleaseEditor;
  }
  return GODOT_SELECTION_RANKS.prereleaseEditor;
}
