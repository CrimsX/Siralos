import { GODOT_LIMITS, createPreparedGodotProbe, type PreparedGodotProbe } from "@solaris/core";

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

interface StoredPlan extends PreparedProbePlan {
  readonly createdAt: number;
  readonly stateBytes: number;
}

export interface PreparedProbeStoreConfig {
  /** Maximum simultaneously prepared probes. */
  readonly maxProbes?: number;
  /** Maximum aggregate serialized plan bytes. */
  readonly maxStateBytes?: number;
  /** Maximum lifetime of a prepared probe before it expires. */
  readonly ttlMs?: number;
  /** Test seam; defaults to `Date.now`. */
  readonly now?: () => number;
}

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
 * Bounded, expiring, single-use prepared-probe registry.
 *
 * Prepared probes are opaque handles into an in-memory registry with a
 * count limit, an aggregate serialized-byte limit, and a TTL. Every handle
 * can be consumed exactly once (the plan is removed on consume), denied or
 * abandoned plans are explicitly disposed, and `disposeAll()` clears the
 * registry on session shutdown so prepared probes can never leak
 * indefinitely. The serialized byte estimate counts UTF-8 bytes of the
 * canonical plan JSON, never JavaScript string length.
 */
export function createPreparedProbeStore(config: PreparedProbeStoreConfig = {}): {
  put(plan: PreparedProbePlan): PreparedProbePutResult;
  consume(probe: PreparedGodotProbe): StoredPlan | null;
  dispose(probe: PreparedGodotProbe): boolean;
  expireNow(): number;
  size(): number;
  stateBytes(): number;
  disposeAll(): void;
} {
  const maxProbes = config.maxProbes ?? GODOT_LIMITS.maxPreparedProbes;
  const maxStateBytes = config.maxStateBytes ?? GODOT_LIMITS.maxPreparedProbeStateBytes;
  const ttlMs = config.ttlMs ?? GODOT_LIMITS.preparedProbeTtlMs;
  const now = config.now ?? ((): number => Date.now());
  const plans = new Map<PreparedGodotProbe, StoredPlan>();

  function put(plan: PreparedProbePlan): PreparedProbePutResult {
    expireNow();
    const stateBytes = utf8ByteLength(planJson(plan));
    if (plans.size >= maxProbes) {
      return {
        ok: false,
        reason: "count-limit",
        message: `The prepared-probe limit of ${maxProbes} was reached; prepare the probe again later.`,
      };
    }
    const currentBytes = [...plans.values()].reduce((total, entry) => total + entry.stateBytes, 0);
    if (currentBytes + stateBytes > maxStateBytes) {
      return {
        ok: false,
        reason: "byte-limit",
        message:
          "The prepared-probe state byte limit was reached; the probe could not be prepared.",
      };
    }
    const probe = createPreparedGodotProbe();
    plans.set(probe, { ...plan, createdAt: now(), stateBytes });
    return { ok: true, probe, stateBytes };
  }

  function consume(probe: PreparedGodotProbe): StoredPlan | null {
    const plan = plans.get(probe);
    if (plan === undefined) {
      return null;
    }
    plans.delete(probe);
    if (now() - plan.createdAt > ttlMs) {
      return null;
    }
    return plan;
  }

  function dispose(probe: PreparedGodotProbe): boolean {
    return plans.delete(probe);
  }

  function expireNow(): number {
    let removed = 0;
    for (const [probe, plan] of plans) {
      if (now() - plan.createdAt > ttlMs) {
        plans.delete(probe);
        removed += 1;
      }
    }
    return removed;
  }

  function size(): number {
    return plans.size;
  }

  function stateBytes(): number {
    return [...plans.values()].reduce((total, entry) => total + entry.stateBytes, 0);
  }

  function disposeAll(): void {
    plans.clear();
  }

  return { put, consume, dispose, expireNow, size, stateBytes, disposeAll };
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

function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}
