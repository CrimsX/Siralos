import { join } from "node:path";
import { type ValidationPlanDiscovery } from "@solaris/core";
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
 */
export function createValidationPlanDiscovery(options: {
  readonly workspaceRoot: string;
}): ValidationPlanDiscovery {
  return {
    async discover(signal?: AbortSignal): Promise<{
      readonly packageScripts: Readonly<Record<string, string>> | null;
    }> {
      if (signal?.aborted) {
        throw new DOMException("The validation-plan discovery was aborted.", "AbortError");
      }
      const bytes = await readFileBounded(
        join(options.workspaceRoot, "package.json"),
        MAX_PACKAGE_JSON_BYTES,
      );
      if (bytes === null) {
        return { packageScripts: null };
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(bytes.toString("utf8"));
      } catch {
        return { packageScripts: null };
      }
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
        return { packageScripts: null };
      }
      const record = parsed as Record<string, unknown>;
      const scriptsValue = record["scripts"];
      if (
        typeof scriptsValue !== "object" ||
        scriptsValue === null ||
        Array.isArray(scriptsValue)
      ) {
        return { packageScripts: null };
      }
      const scripts: Record<string, string> = {};
      const entries = Object.entries(scriptsValue as Record<string, unknown>);
      if (entries.length > MAX_SCRIPT_ENTRIES) {
        return { packageScripts: null };
      }
      for (const [name, body] of entries) {
        if (typeof body === "string") {
          scripts[name] = body;
        }
      }
      return { packageScripts: scripts };
    },
  };
}
