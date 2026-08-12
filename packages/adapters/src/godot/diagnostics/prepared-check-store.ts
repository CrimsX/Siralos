import {
  GODOT_LIMITS,
  createPreparedGDScriptCheck,
  type GodotScriptCheckTarget,
  type PreparedGDScriptCheck,
} from "@siralos/core";
import {
  createPreparedPlanStore,
  type PreparedPlanStore,
  type PreparedPlanStoreConfig,
} from "../probe/prepared-plan-store.js";

export interface PreparedCheckPlan {
  readonly preview: import("@siralos/core").GodotDiagnosticPreview;
  /** Full prepared-check digest; approval binds to exactly this. */
  readonly digest: string;
  readonly manifestDigest: string;
  readonly scriptTargets: readonly GodotScriptCheckTarget[];
  readonly selection: {
    readonly installation: import("@siralos/core").GodotInstallation;
    readonly profile: import("@siralos/core").GodotEngineProfile;
  };
}

export type StoredCheckPlan = PreparedCheckPlan & {
  readonly createdAt: number;
  readonly stateBytes: number;
};

export type PreparedCheckStoreConfig = Omit<PreparedPlanStoreConfig, "label">;

export type PreparedCheckPutResult =
  | {
      readonly ok: true;
      readonly check: PreparedGDScriptCheck;
      readonly stateBytes: number;
    }
  | {
      readonly ok: false;
      readonly reason: "count-limit" | "byte-limit";
      readonly message: string;
    };

/**
 * Prepared GDScript-check registry over the shared bounded, expiring,
 * single-use plan store. The serialized byte estimate counts the digest,
 * manifest digest, script targets, and engine selection.
 */
export function createPreparedCheckStore(config: PreparedCheckStoreConfig = {}): {
  put(plan: PreparedCheckPlan): PreparedCheckPutResult;
  consume(check: PreparedGDScriptCheck): StoredCheckPlan | null;
  dispose(check: PreparedGDScriptCheck): boolean;
  expireNow(): number;
  size(): number;
  stateBytes(): number;
  disposeAll(): void;
} {
  const store: PreparedPlanStore<PreparedGDScriptCheck, PreparedCheckPlan> =
    createPreparedPlanStore<PreparedGDScriptCheck, PreparedCheckPlan>(
      {
        ...config,
        label: "prepared-check",
        maxPlans: config.maxPlans ?? GODOT_LIMITS.maxPreparedChecks,
        maxStateBytes: config.maxStateBytes ?? GODOT_LIMITS.maxPreparedCheckStateBytes,
        ttlMs: config.ttlMs ?? GODOT_LIMITS.preparedCheckTtlMs,
      },
      () => createPreparedGDScriptCheck(),
      (plan) => planJson(plan),
    );
  return {
    put: (plan) => {
      const stored = store.put(plan);
      return stored.ok
        ? { ok: true, check: stored.handle, stateBytes: stored.stateBytes }
        : { ok: false, reason: stored.reason, message: stored.message };
    },
    consume: (check) => store.consume(check),
    dispose: (check) => store.dispose(check),
    expireNow: () => store.expireNow(),
    size: () => store.size(),
    stateBytes: () => store.stateBytes(),
    disposeAll: () => store.disposeAll(),
  };
}

function planJson(plan: PreparedCheckPlan): string {
  return JSON.stringify({
    digest: plan.digest,
    manifestDigest: plan.manifestDigest,
    preview: plan.preview,
    scriptTargets: plan.scriptTargets,
    selection: {
      installationId: plan.selection.installation.id,
      executableSha256: plan.selection.installation.sha256,
      version: plan.selection.profile.version.raw,
    },
  });
}
