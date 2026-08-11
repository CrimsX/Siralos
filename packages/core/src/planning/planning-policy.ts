import type { PlanningDepth } from "./planning-model.js";

/**
 * Deterministic host-owned planning-depth selection (Stage 3 milestone 7,
 * ADR 0020).
 *
 * The HOST decides whether planning is needed and at what depth — never
 * the model. The policy is a pure deterministic function of host-visible
 * task facts: identical structured inputs produce identical decisions, no
 * model is invoked merely to classify task complexity, and ambiguous
 * signals resolve conservatively toward `light` over `none` (planning has
 * token/context cost, so unambiguous narrow work stays plan-free).
 */

export type PlanningDecisionReason =
  | "explicit-plan-request"
  | "inspection-or-no-mutation"
  | "protected-config"
  | "multi-subsystem"
  | "research-required"
  | "capability-uncertainty"
  | "scene-resource-relationships"
  | "narrow-repair-known-surface"
  | "unknown-surface-bounded"
  | "broad-surface-or-many-criteria"
  | "bounded-non-trivial";

export interface PlanningDecision {
  readonly depth: PlanningDepth;
  /** Deterministic machine-readable reason for the decision. */
  readonly reason: PlanningDecisionReason;
}

/**
 * Host-visible signals the policy may use. Every field must be derivable
 * from reliable host facts (request shape, contract contents, explicit
 * user intent, known workspace state) — never vague model intuition.
 */
export interface PlanningDecisionInput {
  /** The task request text (host-visible; used for protected-config detection). */
  readonly request: string;
  /** The user explicitly asked for a plan (/plan or an explicit /develop flag). */
  readonly explicitPlanRequest: boolean;
  /** Depth the user explicitly requested with the plan request, if any. */
  readonly requestedDepth?: "light" | "full";
  /** The task only inspects/reviews; no plan is warranted. */
  readonly inspectionOnly: boolean;
  /** Source mutation is expected during execution. */
  readonly expectedMutation: boolean;
  /** Number of TaskContract acceptance criteria. */
  readonly acceptanceCriterionCount: number;
  /** Protected behavioral configuration (AGENTS.md / .solaris/**) is involved. */
  readonly protectedConfigInvolved: boolean;
  /** The task demonstrably spans multiple subsystems/architecture domains. */
  readonly spansMultipleSubsystems: boolean;
  /** Project/reference research is required. */
  readonly researchRequired: boolean;
  /** Runtime capability uncertainty remains (availability is not established). */
  readonly capabilityUncertainty: boolean;
  /** The task is a repair of a narrowly identified issue. */
  readonly narrowRepair: boolean;
  /** How many likely touched files are already known (0 = unknown surface). */
  readonly knownTouchpoints: number;
  /**
   * The task explicitly involves Godot scene/resource relationships
   * (`.tscn`/`.tres` references, scene inheritance/instancing, signal
   * connections). Complexity evidence only: a simple one-property scene
   * request can still stay light.
   */
  readonly involvesGodotSceneOrResource?: boolean;
}

/** Deterministic marker check for protected behavioral-config references. */
export function containsProtectedConfigReference(text: string): boolean {
  const normalized = text.replace(/\\/g, "/");
  return (
    /(^|[\s/"])(AGENTS\.md)([\s/"]|$)/i.test(normalized) ||
    /(^|[\s/"])(\.solaris)(\/|[\s"]|$)/i.test(normalized) ||
    /behaviou?ral config/i.test(normalized)
  );
}

/**
 * Deterministic marker check for explicit Godot scene/resource references
 * in task text: `.tscn`/`.tres` paths, scene inheritance/instancing
 * phrases, or signal-connection phrasing. Ordinary prose containing the
 * words "scene" or "resource" alone does not match.
 */
export function containsGodotSceneOrResourceReference(text: string): boolean {
  const normalized = text.replace(/\\/g, "/");
  return (
    /\.(tscn|tres)\b/i.test(normalized) ||
    /\b(?:scene|resource)\s+(?:file|tree|inherits?|instance|instanced|signal|connection)s?\b/i.test(
      normalized,
    ) ||
    /\b(?:inherited|instanced)\s+scene\b/i.test(normalized)
  );
}

export interface PlanningPolicy {
  decide(input: PlanningDecisionInput): PlanningDecision;
}

export function createPlanningPolicy(): PlanningPolicy {
  return {
    decide(input: PlanningDecisionInput): PlanningDecision {
      // 1. Explicit user intent wins: a requested plan is always produced
      //    (full unless the user asked for light).
      if (input.explicitPlanRequest) {
        return { depth: input.requestedDepth ?? "full", reason: "explicit-plan-request" };
      }
      // 2. Read-only/inspection work never warrants a plan.
      if (input.inspectionOnly || !input.expectedMutation) {
        return { depth: "none", reason: "inspection-or-no-mutation" };
      }
      // 3. Concrete high-risk signals force full planning.
      if (input.protectedConfigInvolved) {
        return { depth: "full", reason: "protected-config" };
      }
      if (input.spansMultipleSubsystems) {
        return { depth: "full", reason: "multi-subsystem" };
      }
      if (input.researchRequired) {
        return { depth: "full", reason: "research-required" };
      }
      if (input.capabilityUncertainty) {
        return { depth: "full", reason: "capability-uncertainty" };
      }
      // 4. Explicit scene/resource relationship work beyond a trivial
      //    surface warrants full planning; a simple one-property scene
      //    request stays light (the signal is complexity evidence only).
      if (
        input.involvesGodotSceneOrResource === true &&
        (input.knownTouchpoints > 2 || input.acceptanceCriterionCount >= 3)
      ) {
        return { depth: "full", reason: "scene-resource-relationships" };
      }
      // 5. A narrow repair on a known surface stays plan-free.
      if (input.narrowRepair && input.knownTouchpoints > 0 && input.knownTouchpoints <= 2) {
        return { depth: "none", reason: "narrow-repair-known-surface" };
      }
      // 6. An unknown implementation surface is conservatively bounded:
      //    light, never none (ambiguity prefers light over none).
      if (input.knownTouchpoints === 0) {
        return { depth: "light", reason: "unknown-surface-bounded" };
      }
      // 7. Broad surface or many independent criteria warrant full planning.
      if (input.acceptanceCriterionCount >= 4 || input.knownTouchpoints > 4) {
        return { depth: "full", reason: "broad-surface-or-many-criteria" };
      }
      // 8. Everything else is bounded but non-trivial: light.
      return { depth: "light", reason: "bounded-non-trivial" };
    },
  };
}
