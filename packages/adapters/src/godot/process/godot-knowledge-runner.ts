import type { GodotEngineProfile, GodotInstallation, SandboxedProcessResult } from "@siralos/core";

/**
 * Fixed Siralos-owned API documentation generation invocation. The exact
 * selected engine writes `extension_api.json` into the Siralos-private
 * probe working directory; the executable is the only variable and it is
 * always the verified selected installation. The architecture check enforces
 * that this module is the only runtime module that may carry the
 * `--dump-extension-api-with-docs` option and that it never carries
 * project-affecting options.
 */
export const GODOT_KNOWLEDGE_BASE_ARGUMENTS: readonly string[] = ["--dump-extension-api-with-docs"];

/** The only argument tuple the with-docs probe may pass. */
export function godotKnowledgeArguments(): readonly string[] {
  return [...GODOT_KNOWLEDGE_BASE_ARGUMENTS];
}

export interface GodotKnowledgeCommandDigestParts {
  readonly executableSha256: string;
  readonly argumentTemplate: readonly string[];
  readonly workingDirectoryPolicy: "siralos-private-probe-directory";
  readonly profileId: string;
  readonly environmentPolicy: "minimal";
  readonly stdinPolicy: "closed";
  readonly networkPolicy: "denied";
  readonly timeoutMs: number;
  readonly stdoutLimitBytes: number;
  readonly stderrLimitBytes: number;
}

export interface GodotKnowledgeRunRequest {
  readonly installation: GodotInstallation;
  readonly engineProfile: GodotEngineProfile;
  /** Siralos-private probe directory; never the workspace. */
  readonly probeDirectory: string;
  readonly signal?: AbortSignal;
}

export type GodotKnowledgeRunOutcome =
  | {
      readonly status: "completed";
      readonly result: SandboxedProcessResult;
    }
  | {
      readonly status: "unsupported";
      readonly message: string;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    }
  | {
      readonly status:
        | "timed-out"
        | "cancelled"
        | "sandbox-denied"
        | "sandbox-unavailable"
        | "output-limit"
        | "failed";
      readonly message: string;
      readonly result: SandboxedProcessResult;
    };

export interface GodotKnowledgeRunnerDependencies {
  // The dependencies are retained for signature compatibility; the
  // fail-closed runner never uses them.
  readonly backend: unknown;
  /** Sanitized host parent environment (never raw `process.env`). */
  readonly parentEnvironment?: Readonly<Record<string, string>>;
}

export interface GodotKnowledgeRunner {
  isAvailable(): Promise<boolean>;
  generateDocumentation(request: GodotKnowledgeRunRequest): Promise<GodotKnowledgeRunOutcome>;
}

export const GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE =
  "Exact-engine API documentation generation is unavailable: Node and the pinned sandbox runtime offer no exec-by-handle or directory-handle-relative primitive, so the staged executable's pathname is re-opened at spawn time and a same-user process could substitute different bytes between final verification and launch, and the Siralos-private probe directory cannot be created or cleaned up identity-bound. The verified fingerprint could then be attached to bytes that never execute. Generation fails closed and the executable is never spawned; no probe directory is created. It will become available only when a mechanically identity-bound launch and directory-lifecycle primitive exists.";

/**
 * API documentation generation fails closed and never spawns the
 * executable. The fixed probe would run `--dump-extension-api-with-docs`
 * in a Siralos-private probe directory with network denied and the
 * workspace excluded from readable roots; until launch can be mechanically
 * bound to the verified executable identity and the probe directory to a
 * verified parent, every generation reports a typed `unavailable` outcome
 * and nothing is created or deleted.
 */
export function createGodotKnowledgeRunner(
  _dependencies: GodotKnowledgeRunnerDependencies,
): GodotKnowledgeRunner {
  return {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(false);
    },
    async generateDocumentation(
      request: GodotKnowledgeRunRequest,
    ): Promise<GodotKnowledgeRunOutcome> {
      if (request.signal?.aborted) {
        throw createAbortError();
      }
      const capability = requireKnowledgeCapabilities(request);
      if (!capability.ok) {
        return { status: "unsupported", message: capability.message };
      }
      await Promise.resolve();
      return {
        status: "unavailable",
        message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
      };
    },
  };
}

function requireKnowledgeCapabilities(request: GodotKnowledgeRunRequest):
  | {
      readonly ok: true;
    }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  if (request.installation.status !== "valid") {
    return { ok: false, message: "The installation is invalid; rediscovery is required." };
  }
  if (request.engineProfile.edition === "runtime-only") {
    return {
      ok: false,
      message:
        "The selected executable is runtime-only; it cannot generate the extension API documentation.",
    };
  }
  if (!request.engineProfile.capabilities.extensionApiWithDocsDump) {
    return {
      ok: false,
      message:
        "The selected Godot version does not advertise --dump-extension-api-with-docs; exact-engine API documentation is unsupported and an ordinary --dump-extension-api result is never substituted.",
    };
  }
  return { ok: true };
}

function createAbortError(): Error {
  return new DOMException("The Godot API documentation generation was aborted.", "AbortError");
}
