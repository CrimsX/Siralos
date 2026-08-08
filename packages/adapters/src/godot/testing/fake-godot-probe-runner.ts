import type {
  GodotApiDumpProbe,
  GodotHelpProbe,
  GodotInstallation,
  GodotProbeRunner,
  GodotVersionProbe,
} from "@solaris/core";

/**
 * Deterministic probe runner for tests. Standard tests never require a real
 * Godot binary; fixtures emulate stable, prerelease, custom, .NET-like,
 * runtime-only, and broken executables.
 */
export type ScriptedProbe =
  | {
      readonly status: "success";
      readonly stdout: string;
    }
  | {
      readonly status: "degraded";
      readonly stdout: string;
      readonly message: string;
    }
  | {
      readonly status: "failed";
      readonly message: string;
    };

export interface FakeGodotProbeRunnerOptions {
  readonly version?: ScriptedProbe;
  readonly help?: ScriptedProbe;
  readonly apiDump?: ScriptedProbe;
  /** Version result used when the version probe is called (fallback). */
  readonly versionText?: string;
  /** Help text used when the help probe is called (fallback). */
  readonly helpText?: string;
  /** `--dump-extension-api` advertised; when true the api dump scripted probe runs. */
  readonly advertiseApiDump?: boolean;
  /** When false, `isAvailable()` reports unavailable and no probe runs. */
  readonly available?: boolean;
}

export function createFakeGodotProbeRunner(options: FakeGodotProbeRunnerOptions = {}): {
  readonly runner: GodotProbeRunner;
  readonly calls: () => { version: number; help: number; api: number };
} {
  const calls = { version: 0, help: 0, api: 0 };
  const runner: GodotProbeRunner = {
    isAvailable(): Promise<boolean> {
      return Promise.resolve(options.available ?? true);
    },
    probeVersion(_installation: GodotInstallation): Promise<GodotVersionProbe> {
      calls.version += 1;
      return Promise.resolve(resolveVersionProbe(options));
    },
    probeHelp(_installation: GodotInstallation): Promise<GodotHelpProbe> {
      calls.help += 1;
      return Promise.resolve(resolveHelpProbe(options));
    },
    dumpExtensionApi(_installation: GodotInstallation): Promise<GodotApiDumpProbe> {
      calls.api += 1;
      return Promise.resolve(resolveApiDumpProbe(options));
    },
  };
  return { runner, calls: () => ({ ...calls }) };
}

function resolveVersionProbe(options: FakeGodotProbeRunnerOptions): GodotVersionProbe {
  const scripted = options.version;
  if (scripted === undefined) {
    const parsed = parseVersion(options.versionText ?? "4.7.1.stable.official");
    if (parsed === null) {
      return { status: "failed", message: "The Godot version output is not recognizable." };
    }
    return { status: "success", version: parsed };
  }
  if (scripted.status === "failed") {
    return { status: "failed", message: scripted.message };
  }
  const parsed = parseVersion(scripted.stdout);
  if (parsed === null) {
    return { status: "failed", message: "The Godot version output is not recognizable." };
  }
  return { status: "success", version: parsed };
}

function resolveHelpProbe(options: FakeGodotProbeRunnerOptions): GodotHelpProbe {
  const scripted = options.help;
  const text =
    scripted !== undefined && scripted.status !== "failed"
      ? scripted.stdout
      : (options.helpText ?? defaultHelpText(options.advertiseApiDump ?? false));
  const capabilities = parseCapabilities(text);
  if (scripted !== undefined && scripted.status === "failed") {
    return { status: "failed", message: scripted.message };
  }
  if (scripted !== undefined && scripted.status === "degraded") {
    return {
      status: "degraded",
      message: scripted.message,
      capabilities,
      unknownOptionCount: 0,
    };
  }
  return { status: "success", capabilities, unknownOptionCount: 0 };
}

function resolveApiDumpProbe(options: FakeGodotProbeRunnerOptions): GodotApiDumpProbe {
  const scripted = options.apiDump;
  const summary = {
    headerVersion: "4.7.1.stable.official",
    apiHash: "abc123",
    classCount: 3,
    builtinClassCount: 2,
    globalEnumCount: 1,
    utilityFunctionCount: 2,
    configurationVersion: 5,
    fileSizeBytes: 1024,
    sha256: "b".repeat(64),
  };
  if (scripted === undefined) {
    return { status: "success", summary };
  }
  if (scripted.status === "failed") {
    return { status: "failed", message: scripted.message };
  }
  if (scripted.status === "degraded") {
    return { status: "degraded", message: scripted.message };
  }
  return { status: "success", summary };
}

function parseVersion(text: string): import("@solaris/core").GodotVersion | null {
  const segments = text.trim().split(".");
  const major = Number(segments[0]);
  const minor = Number(segments[1]);
  if (!Number.isFinite(major) || !Number.isFinite(minor)) {
    return null;
  }
  const patchToken = segments[2];
  const patch =
    patchToken === undefined || patchToken === "" || !/^\d+$/.test(patchToken)
      ? null
      : Number(patchToken);
  const statusToken = segments[patch === null ? 2 : 3];
  const status: "stable" | "rc" | "beta" | "alpha" | "dev" | "custom" | "unknown" =
    statusToken === undefined || statusToken === "stable"
      ? "stable"
      : statusToken.startsWith("rc")
        ? "rc"
        : statusToken.startsWith("beta")
          ? "beta"
          : statusToken.startsWith("dev")
            ? "dev"
            : statusToken.startsWith("custom")
              ? "custom"
              : "unknown";
  const statusNumberMatch = /(\d+)$/.exec(statusToken ?? "");
  const buildSegment = segments[patch === null ? 3 : 4];
  return {
    raw: text.trim(),
    major,
    minor,
    patch,
    status,
    statusNumber: statusNumberMatch === null ? null : Number(statusNumberMatch[1]),
    build: buildSegment ?? null,
    commit: null,
  } satisfies import("@solaris/core").GodotVersion;
}

function defaultHelpText(advertiseApiDump: boolean): string {
  const lines = [
    "--editor  Starts the editor.",
    "--project-manager  Starts the project manager.",
    "--headless  Runs headless.",
    "--path <directory>  Sets the project path.",
    "--dump-extension-api  Generates extension_api.json.",
  ];
  if (advertiseApiDump) {
    return lines.join("\n");
  }
  return lines.join("\n");
}

function parseCapabilities(text: string) {
  const options: readonly {
    readonly option: string;
    readonly key: keyof import("@solaris/core").GodotCommandCapabilities;
  }[] = [
    { option: "--editor", key: "editor" },
    { option: "--project-manager", key: "projectManager" },
    { option: "--recovery-mode", key: "recoveryMode" },
    { option: "--headless", key: "headless" },
    { option: "--path", key: "projectPath" },
    { option: "--scene", key: "scene" },
    { option: "--script", key: "script" },
    { option: "--check-only", key: "checkOnly" },
    { option: "--import", key: "import" },
    { option: "--quit", key: "quit" },
    { option: "--quit-after", key: "quitAfter" },
    { option: "--lsp-port", key: "lsp" },
    { option: "--dap-port", key: "dap" },
    { option: "--debug-server", key: "debugServer" },
    { option: "--build-solutions", key: "buildSolutions" },
    { option: "--dump-extension-api", key: "extensionApiDump" },
    { option: "--dump-extension-api-with-docs", key: "extensionApiWithDocsDump" },
    { option: "--validate-extension-api", key: "extensionApiValidation" },
    { option: "--doctool", key: "docTool" },
    { option: "--write-movie", key: "movieWriting" },
  ];
  const result: Record<string, boolean> = {};
  for (const entry of options) {
    result[entry.key] = text.includes(entry.option);
  }
  return result as unknown as import("@solaris/core").GodotCommandCapabilities;
}
