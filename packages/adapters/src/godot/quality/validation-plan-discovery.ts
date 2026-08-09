import { lstat } from "node:fs/promises";
import { join } from "node:path";
import type { ValidationPlanDiscovery } from "@solaris/core";
import { readFileBounded } from "../../fs/file-read.js";

/** Maximum root package.json size read for validation-plan discovery. */
const MAX_PACKAGE_JSON_BYTES = 1024 * 1024;
/** Maximum discovered npm script entries. */
const MAX_SCRIPT_ENTRIES = 200;

/**
 * Validation-plan discovery (ADR 0013 §21). Reads only the bounded root
 * package.json of the workspace (regular file, no symlinks, size-capped)
 * and returns its `scripts` map; the deterministic selection policy lives
 * in core. Nothing is executed or installed here — discovery is read-only.
 *
 * Absence and unreadability are distinct: a missing package.json means the
 * project has no test runner (`not_applicable`), while a package.json that
 * exists but cannot be read (permissions, symlink, oversized, invalid
 * JSON) is an infrastructure condition that must surface as
 * `validation_incomplete` — never silently treated as "no test runner".
 */
export function createValidationPlanDiscovery(options: {
  readonly workspaceRoot: string;
}): ValidationPlanDiscovery {
  return {
    async discover(signal?: AbortSignal): Promise<{
      readonly packageScripts: Readonly<Record<string, string>> | null;
      readonly unreadable: boolean;
    }> {
      if (signal?.aborted) {
        throw new DOMException("The validation-plan discovery was aborted.", "AbortError");
      }
      const packageJsonPath = join(options.workspaceRoot, "package.json");
      let absent = false;
      try {
        const stats = await lstat(packageJsonPath);
        if (stats.isSymbolicLink() || !stats.isFile()) {
          return { packageScripts: null, unreadable: true };
        }
        if (stats.size > MAX_PACKAGE_JSON_BYTES) {
          return { packageScripts: null, unreadable: true };
        }
      } catch (error: unknown) {
        if (isNotFoundError(error)) {
          absent = true;
        } else {
          return { packageScripts: null, unreadable: true };
        }
      }
      if (absent) {
        return { packageScripts: null, unreadable: false };
      }
      const bytes = await readFileBounded(packageJsonPath, MAX_PACKAGE_JSON_BYTES);
      if (bytes === null) {
        return { packageScripts: null, unreadable: true };
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(bytes.toString("utf8"));
      } catch {
        return { packageScripts: null, unreadable: true };
      }
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
        return { packageScripts: null, unreadable: true };
      }
      const record = parsed as Record<string, unknown>;
      const scriptsValue = record["scripts"];
      if (
        typeof scriptsValue !== "object" ||
        scriptsValue === null ||
        Array.isArray(scriptsValue)
      ) {
        return { packageScripts: null, unreadable: false };
      }
      const scripts: Record<string, string> = {};
      const entries = Object.entries(scriptsValue as Record<string, unknown>);
      if (entries.length > MAX_SCRIPT_ENTRIES) {
        return { packageScripts: null, unreadable: true };
      }
      for (const [name, body] of entries) {
        if (typeof body === "string") {
          scripts[name] = body;
        }
      }
      return { packageScripts: scripts, unreadable: false };
    },
  };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}
