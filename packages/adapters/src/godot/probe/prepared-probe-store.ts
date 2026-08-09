import { GODOT_LIMITS, createPreparedGodotProbe, type PreparedGodotProbe } from "@solaris/core";
import { createPreparedPlanStore, type PreparedPlanStore } from "./prepared-plan-store.js";

export interface PreparedProbePlan {
  readonly preview: import("@solaris/core").GodotProbePreview;
  /** Full prepared-probe digest; approval binds to exactly this. */
  readonly digest: string;
  readonly manifestDigest: string;
  readonly manifest: import("@solaris/core").GodotProjectRiskManifest;
  readonly selection: {
    readonly installation: import("@solaris/core").GodotInstallation;
    readonly profile: import("@solaris/core").GodotEngineProfile;
  };
}

export type StoredProbePlan = PreparedProbePlan & {
  readonly createdAt: number;
  readonly stateBytes: number;
};

export type PreparedProbeStoreConfig = {
  /** Maximum simultaneously prepared probes. */
  readonly maxProbes?: number;
  /** Maximum aggregate serialized plan bytes. */
  readonly maxStateBytes?: number;
  /** Maximum lifetime of a prepared probe before it expires. */
  readonly ttlMs?: number;
  /** Test seam; defaults to `Date.now`. */
  readonly now?: () => number;
};

export type PreparedProbePutResult =
  | {
      readonly ok: true;
      readonly probe: PreparedGodotProbe;
      readonly stateBytes: number;
    }
  | {
      readonly ok: false;
      readonly reason: "count-limit" | "byte-limit";
      readonly message: string;
    };

/**
 * Prepared-probe registry over the shared bounded, expiring, single-use
 * plan store. The serialized byte estimate counts the same canonical JSON
 * subset as before (digest, manifest digest, preview, engine selection).
 */
export function createPreparedProbeStore(config: PreparedProbeStoreConfig = {}): {
  put(plan: PreparedProbePlan): PreparedProbePutResult;
  consume(probe: PreparedGodotProbe): StoredProbePlan | null;
  dispose(probe: PreparedGodotProbe): boolean;
  expireNow(): number;
  size(): number;
  stateBytes(): number;
  disposeAll(): void;
} {
  const store: PreparedPlanStore<PreparedGodotProbe, PreparedProbePlan> = createPreparedPlanStore<
    PreparedGodotProbe,
    PreparedProbePlan
  >(
    {
      ...config,
      label: "prepared-probe",
      maxPlans: config.maxProbes ?? GODOT_LIMITS.maxPreparedProbes,
      maxStateBytes: config.maxStateBytes ?? GODOT_LIMITS.maxPreparedProbeStateBytes,
      ttlMs: config.ttlMs ?? GODOT_LIMITS.preparedProbeTtlMs,
    },
    () => createPreparedGodotProbe(),
    (plan) => planJson(plan),
  );
  return {
    put: (plan) => {
      const stored = store.put(plan);
      return stored.ok
        ? { ok: true, probe: stored.handle, stateBytes: stored.stateBytes }
        : { ok: false, reason: stored.reason, message: stored.message };
    },
    consume: (probe) => store.consume(probe),
    dispose: (probe) => store.dispose(probe),
    expireNow: () => store.expireNow(),
    size: () => store.size(),
    stateBytes: () => store.stateBytes(),
    disposeAll: () => store.disposeAll(),
  };
}

function planJson(plan: PreparedProbePlan): string {
  return JSON.stringify({
    digest: plan.digest,
    manifestDigest: plan.manifestDigest,
    preview: plan.preview,
    selection: {
      installationId: plan.selection.installation.id,
      executableSha256: plan.selection.installation.sha256,
      version: plan.selection.profile.version.raw,
    },
  });
}
