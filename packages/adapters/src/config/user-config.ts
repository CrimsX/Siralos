import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

export type UserSandboxProfileId = "inspect" | "develop-offline";

export type UserSandboxBackendId = "auto" | "anthropic-runtime";

export interface UserSandboxConfig {
  readonly profile: UserSandboxProfileId;
  readonly backend: UserSandboxBackendId;
}

export interface UserConfig {
  readonly sandbox: UserSandboxConfig;
}

export const DEFAULT_USER_CONFIG: UserConfig = {
  sandbox: {
    profile: "inspect",
    backend: "auto",
  },
};

const SUPPORTED_PROFILES: readonly string[] = ["inspect", "develop-offline"];
const SUPPORTED_BACKENDS: readonly string[] = ["auto", "anthropic-runtime"];

export function getDefaultUserConfigPath(): string {
  return join(homedir(), ".solaris", "config.json");
}

export async function loadUserConfig(configPath: string): Promise<UserConfig> {
  let content: string;
  try {
    content = await readFile(configPath, "utf8");
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return DEFAULT_USER_CONFIG;
    }
    throw new Error(`Cannot read Solaris configuration at ${configPath}: ${describeError(error)}`);
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch (error: unknown) {
    throw new Error(
      `Solaris configuration at ${configPath} is not valid JSON: ${describeError(error)}`,
    );
  }
  return parseUserConfig(parsed);
}

export function parseUserConfig(data: unknown): UserConfig {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw new Error("Solaris configuration must be a JSON object.");
  }
  const record = data as Record<string, unknown>;
  const unknownKeys = Object.keys(record).filter((key) => key !== "sandbox");
  if (unknownKeys.length > 0) {
    throw new Error(`Unknown Solaris configuration section: ${unknownKeys[0]}.`);
  }
  const sandboxValue = record["sandbox"];
  if (sandboxValue === undefined) {
    return DEFAULT_USER_CONFIG;
  }
  if (typeof sandboxValue !== "object" || sandboxValue === null || Array.isArray(sandboxValue)) {
    throw new Error('Solaris configuration section "sandbox" must be a JSON object.');
  }
  const sandbox = sandboxValue as Record<string, unknown>;
  const sandboxKeys = Object.keys(sandbox).filter((key) => key !== "profile" && key !== "backend");
  if (sandboxKeys.length > 0) {
    throw new Error(`Unknown Solaris sandbox configuration key: ${sandboxKeys[0]}.`);
  }
  const profile = sandbox["profile"] ?? "inspect";
  if (typeof profile !== "string" || !SUPPORTED_PROFILES.includes(profile)) {
    const profileLabel = typeof profile === "string" ? profile : JSON.stringify(profile);
    throw new Error(
      `Unknown sandbox profile: ${profileLabel}. Expected one of: inspect, develop-offline.`,
    );
  }
  const backend = sandbox["backend"] ?? "auto";
  if (typeof backend !== "string" || !SUPPORTED_BACKENDS.includes(backend)) {
    const backendLabel = typeof backend === "string" ? backend : JSON.stringify(backend);
    throw new Error(
      `Unknown sandbox backend: ${backendLabel}. Expected one of: auto, anthropic-runtime.`,
    );
  }
  return {
    sandbox: {
      profile: profile as UserSandboxProfileId,
      backend: backend as UserSandboxBackendId,
    },
  };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "unknown error";
}
