import {
  GODOT_LIMITS,
  createPreparedGDScriptSession,
  type GDScriptLSPSessionPreview,
  type PreparedGDScriptSession,
} from "@solaris/core";
import { createPreparedPlanStore, type PreparedPlanStore } from "../probe/prepared-plan-store.js";

export interface PreparedLSPSessionPlan {
  readonly preview: GDScriptLSPSessionPreview;
  /** Full prepared-session digest; approval binds to exactly this. */
  readonly digest: string;
  readonly manifestDigest: string;
  readonly selection: {
    readonly installation: import("@solaris/core").GodotInstallation;
    readonly profile: import("@solaris/core").GodotEngineProfile;
  };
}

export type StoredLSPSessionPlan = PreparedLSPSessionPlan & {
  readonly createdAt: number;
  readonly stateBytes: number;
};

export interface PreparedLSPSessionStoreConfig {
  readonly maxSessions?: number;
  readonly maxStateBytes?: number;
  readonly ttlMs?: number;
  readonly now?: () => number;
}

export type PreparedLSPSessionPutResult =
  | {
      readonly ok: true;
      readonly session: PreparedGDScriptSession;
      readonly stateBytes: number;
    }
  | {
      readonly ok: false;
      readonly reason: "count-limit" | "byte-limit";
      readonly message: string;
    };

/** Prepared LSP-session registry over the shared bounded, expiring, single-use plan store. */
export function createPreparedLSPSessionStore(config: PreparedLSPSessionStoreConfig = {}): {
  put(plan: PreparedLSPSessionPlan): PreparedLSPSessionPutResult;
  consume(session: PreparedGDScriptSession): StoredLSPSessionPlan | null;
  dispose(session: PreparedGDScriptSession): boolean;
  expireNow(): number;
  size(): number;
  stateBytes(): number;
  disposeAll(): void;
} {
  const store: PreparedPlanStore<PreparedGDScriptSession, PreparedLSPSessionPlan> =
    createPreparedPlanStore<PreparedGDScriptSession, PreparedLSPSessionPlan>(
      {
        ...config,
        label: "prepared-lsp-session",
        maxPlans: config.maxSessions ?? GODOT_LIMITS.maxPreparedLSPSessions,
        maxStateBytes: config.maxStateBytes ?? GODOT_LIMITS.maxPreparedLSPSessionStateBytes,
        ttlMs: config.ttlMs ?? GODOT_LIMITS.preparedLSPSessionTtlMs,
      },
      () => createPreparedGDScriptSession(),
      (plan) =>
        JSON.stringify({
          digest: plan.digest,
          manifestDigest: plan.manifestDigest,
          preview: plan.preview,
          selection: {
            installationId: plan.selection.installation.id,
            executableSha256: plan.selection.installation.sha256,
            version: plan.selection.profile.version.raw,
          },
        }),
    );
  return {
    put: (plan) => {
      const stored = store.put(plan);
      return stored.ok
        ? { ok: true, session: stored.handle, stateBytes: stored.stateBytes }
        : { ok: false, reason: stored.reason, message: stored.message };
    },
    consume: (session) => store.consume(session),
    dispose: (session) => store.dispose(session),
    expireNow: () => store.expireNow(),
    size: () => store.size(),
    stateBytes: () => store.stateBytes(),
    disposeAll: () => store.disposeAll(),
  };
}
