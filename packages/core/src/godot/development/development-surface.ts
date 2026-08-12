/**
 * Unified development surface routing (Stage 3 milestone 11, ADR 0027).
 *
 * The host determines which Godot surfaces a development task requires
 * from host-observed state only — the request text, the verified and
 * candidate touchpoints produced by workspace scope / structural
 * inspection, and the project surface inventory — never from model
 * claims. The classification is deterministic, bounded, and recorded
 * with its evidence so the execution path stays host-owned.
 */

export type DevelopmentSurfaceKind = "script_only" | "native_only" | "mixed" | "none";

export interface DevelopmentSurfaceTouchpoint {
  /** Workspace-relative path of a candidate or verified touchpoint. */
  readonly path: string;
  /** Distinction preserved from workspace scope (never promoted). */
  readonly status: "verified" | "candidate";
}

export interface DevelopmentSurfaceInput {
  readonly request: string;
  /** Host-observed verified/candidate touchpoints (may be empty). */
  readonly touchpoints: readonly DevelopmentSurfaceTouchpoint[];
  /**
   * Bounded project surface inventory derived from static inspection
   * (project.godot references, scene/resource files observed under the
   * workspace scope). Never a full project load.
   */
  readonly projectSurfaces?: {
    readonly hasScenes: boolean;
    readonly hasResources: boolean;
    readonly hasScripts: boolean;
  };
}

export interface DevelopmentSurfaceDecision {
  readonly kind: DevelopmentSurfaceKind;
  /** Human-readable routing rationale (bounded, deterministic). */
  readonly rationale: string;
  /** Host-observed evidence the decision used. */
  readonly evidence: readonly string[];
}

/** Path-based surface detection: .tscn/.tres are native, .gd is script. */
export function classifyDevelopmentSurfacePath(path: string): "script" | "native" | "other" {
  if (path.endsWith(".tscn") || path.endsWith(".tres")) {
    return "native";
  }
  if (path.endsWith(".gd")) {
    return "script";
  }
  return "other";
}

const SCENE_OR_RESOURCE_SIGNAL =
  /\b(?:\.tscn|\.tres|scene|resource|node|property|signal|autoload|project\.godot)\b/i;

// Conservative: ordinary prose containing "script" alone does not match
// (mirrors the scene/resource signal discipline).
const SCRIPT_SIGNAL = /\b(?:[A-Za-z0-9_/-]+\.gd|gdscript|@export|export\s+var)\b/i;

/**
 * Deterministic host-owned surface classification.
 *
 * Native involvement is triggered by native touchpoints or an explicit
 * scene/resource request reference; script involvement by script
 * touchpoints. The decision never comes from a model assertion.
 */
export function classifyDevelopmentSurface(
  input: DevelopmentSurfaceInput,
): DevelopmentSurfaceDecision {
  const evidence: string[] = [];
  const touchesScript = input.touchpoints.some((touchpoint) => {
    const surface = classifyDevelopmentSurfacePath(touchpoint.path);
    if (surface === "script") {
      evidence.push(`touchpoint ${touchpoint.path} (${touchpoint.status}) is GDScript`);
      return true;
    }
    return false;
  });
  const touchesNative = input.touchpoints.some((touchpoint) => {
    const surface = classifyDevelopmentSurfacePath(touchpoint.path);
    if (surface === "native") {
      evidence.push(`touchpoint ${touchpoint.path} (${touchpoint.status}) is scene/resource`);
      return true;
    }
    return false;
  });
  const requestMentionsNative = SCENE_OR_RESOURCE_SIGNAL.test(input.request);
  if (requestMentionsNative) {
    evidence.push("request references scene/resource terminology");
  }
  const requestMentionsScript = SCRIPT_SIGNAL.test(input.request);
  if (requestMentionsScript) {
    evidence.push("request references GDScript terminology");
  }
  const projectHasNative =
    (input.projectSurfaces?.hasScenes ?? false) || (input.projectSurfaces?.hasResources ?? false);
  if (projectHasNative) {
    evidence.push("project surface inventory includes scenes/resources");
  }
  const projectHasScripts = input.projectSurfaces?.hasScripts ?? false;
  if (projectHasScripts) {
    evidence.push("project surface inventory includes scripts");
  }

  const native = touchesNative || requestMentionsNative || projectHasNative;
  const script = touchesScript || requestMentionsScript || projectHasScripts;

  if (native && script) {
    return {
      kind: "mixed",
      rationale:
        "Host-observed evidence shows both GDScript and scene/resource surfaces; the task routes to the unified mixed-workflow path.",
      evidence,
    };
  }
  if (native) {
    return {
      kind: "native_only",
      rationale:
        "Host-observed evidence shows only scene/resource surfaces; no script change is routed.",
      evidence,
    };
  }
  if (script) {
    return {
      kind: "script_only",
      rationale:
        "Host-observed evidence shows only GDScript surfaces; the task keeps the existing script path.",
      evidence,
    };
  }
  return {
    kind: "none",
    rationale:
      "No Godot surface evidence is host-observed; no mutation surface is routed for this request.",
    evidence,
  };
}
