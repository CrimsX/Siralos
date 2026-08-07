import { createHash } from "node:crypto";
import type { GodotApiDumpSummary } from "@solaris/core";

export type GodotApiDumpExtraction =
  | { readonly ok: true; readonly summary: GodotApiDumpSummary }
  | { readonly ok: false; readonly message: string };

/**
 * Validates the high-level structure of an `extension_api.json` dump and
 * extracts only bounded profile metadata. The complete dump never enters
 * provider context and is never persisted by this milestone.
 */
export function extractGodotApiDumpSummary(
  content: string,
  fileSizeBytes: number,
): GodotApiDumpExtraction {
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return { ok: false, message: "The API dump is not valid JSON." };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, message: "The API dump is not a JSON object." };
  }
  const root = parsed as Record<string, unknown>;
  const header = root["header"];
  if (typeof header !== "object" || header === null || Array.isArray(header)) {
    return { ok: false, message: "The API dump has no valid header object." };
  }
  const headerRecord = header as Record<string, unknown>;
  const headerVersion =
    typeof headerRecord["version_full_name"] === "string"
      ? headerRecord["version_full_name"]
      : null;
  const apiHash = typeof headerRecord["hash"] === "string" ? headerRecord["hash"] : null;
  const classCount = countField(root, "classes");
  const builtinClassCount = countField(root, "builtin_classes");
  const globalEnumCount = countField(root, "global_enums");
  const utilityFunctionCount = countField(root, "utility_functions");
  const configurations = root["configurations"];
  const configurationVersion =
    typeof configurations === "object" &&
    configurations !== null &&
    !Array.isArray(configurations) &&
    typeof (configurations as Record<string, unknown>)["format_version"] === "number"
      ? ((configurations as Record<string, unknown>)["format_version"] as number)
      : null;
  const sha256 = createHash("sha256").update(content, "utf8").digest("hex");
  return {
    ok: true,
    summary: {
      headerVersion,
      apiHash,
      classCount,
      builtinClassCount,
      globalEnumCount,
      utilityFunctionCount,
      configurationVersion,
      fileSizeBytes,
      sha256,
    },
  };
}

function countField(root: Record<string, unknown>, key: string): number | null {
  const value = root[key];
  return Array.isArray(value) ? value.length : null;
}
