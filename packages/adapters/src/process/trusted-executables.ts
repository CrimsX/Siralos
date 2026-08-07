import { lstat, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { readParentEnvironment } from "../environment/child-environment.js";

export interface TrustedNodeIdentity {
  readonly executable: string;
  readonly version: string;
}

export function resolveTrustedNode(): TrustedNodeIdentity {
  return {
    executable: process.execPath,
    version: process.version,
  };
}

export type NpmCliResolution =
  | {
      readonly status: "resolved";
      readonly cliPath: string;
      readonly version: string;
    }
  | {
      readonly status: "unavailable";
      readonly message: string;
    };

/**
 * Resolve the npm CLI JavaScript file associated with the current trusted
 * Node installation. The resolved file is invoked through the trusted Node
 * executable with separate arguments; no `npm.cmd` wrapper and no shell is
 * ever used at the Solaris process layer.
 */
export async function resolveNpmCli(): Promise<NpmCliResolution> {
  for (const candidate of buildNpmCliCandidates()) {
    const outcome = await probeNpmCli(candidate);
    if (outcome !== null) {
      return outcome;
    }
  }
  return {
    status: "unavailable",
    message: "The trusted npm CLI could not be resolved next to the trusted Node executable.",
  };
}

async function probeNpmCli(cliPath: string): Promise<NpmCliResolution | null> {
  let stats;
  try {
    stats = await lstat(cliPath);
  } catch {
    return null;
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    return null;
  }
  const npmPackageJson = join(dirname(dirname(cliPath)), "package.json");
  try {
    const parsed = JSON.parse(await readFile(npmPackageJson, "utf8")) as {
      readonly version?: unknown;
    };
    const version = typeof parsed.version === "string" ? parsed.version : "unknown";
    return { status: "resolved", cliPath, version };
  } catch {
    return null;
  }
}

function buildNpmCliCandidates(): readonly string[] {
  const candidates: string[] = [];
  const nodeDirectory = dirname(process.execPath);
  candidates.push(join(nodeDirectory, "node_modules", "npm", "bin", "npm-cli.js"));
  candidates.push(join(nodeDirectory, "..", "lib", "node_modules", "npm", "bin", "npm-cli.js"));
  const npmExecPath = readParentEnvironment()["npm_execpath"];
  if (
    typeof npmExecPath === "string" &&
    npmExecPath.length > 0 &&
    npmExecPath.toLowerCase().endsWith("npm-cli.js")
  ) {
    candidates.push(npmExecPath);
  }
  return candidates;
}
