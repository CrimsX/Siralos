/**
 * Generic go-to-definition result normalization (Stage 3R R5).
 *
 * Extracted from the LSP definition normalization semantics:
 * LocationLink (`targetUri`/`targetRange`) and plain Location
 * (`uri`/`range`) forms are accepted, 0-based positions are converted
 * to the 1-based Siralos convention, input order is preserved,
 * results are bounded with explicit truncation, and locations
 * outside the served workspace are represented conservatively
 * (basename only, `external: true`) without absolute paths. A
 * definition location is data, never permission: external locations
 * never widen workspace read authority.
 */
import { toOneBasedRange, type LanguageRange } from "./position.js";
import { sanitizeControlCharacters } from "./sanitize.js";

/** One definition location; `external` marks out-of-workspace targets. */
export interface DefinitionLocation {
  readonly path: string;
  readonly range: LanguageRange;
  readonly external: boolean;
}

/** Bounds for one definition query normalization. */
export interface DefinitionLimits {
  readonly maxLocations: number;
}

/**
 * Normalize one raw LSP definition response. `mapUri` maps a service
 * URI to a workspace-relative path or null when outside the served
 * workspace; `queryPath` is the workspace-relative path of the query
 * document (adapter-computed). Malformed entries are skipped;
 * out-of-workspace targets become conservative external basenames.
 */
export function normalizeDefinitionLocations(
  locations: unknown,
  queryPath: string,
  mapUri: (uri: string) => string | null,
  limits: DefinitionLimits,
): {
  readonly path: string;
  readonly locations: readonly DefinitionLocation[];
  readonly truncated: boolean;
} {
  const rawLocations = Array.isArray(locations)
    ? locations
    : locations === null || locations === undefined
      ? []
      : [locations];
  const result: DefinitionLocation[] = [];
  let truncated = false;
  for (const entry of rawLocations) {
    if (result.length >= limits.maxLocations) {
      truncated = true;
      break;
    }
    const location = normalizeDefinitionLocation(entry, mapUri);
    if (location !== null) {
      result.push(location);
    }
  }
  return { path: queryPath, locations: result, truncated };
}

function normalizeDefinitionLocation(
  entry: unknown,
  mapUri: (uri: string) => string | null,
): DefinitionLocation | null {
  if (typeof entry !== "object" || entry === null) {
    return null;
  }
  const record = entry as Record<string, unknown>;
  // LocationLink: use targetUri + targetRange.
  const uri =
    typeof record["targetUri"] === "string"
      ? record["targetUri"]
      : typeof record["uri"] === "string"
        ? record["uri"]
        : null;
  const rangeValue = record["targetRange"] !== undefined ? record["targetRange"] : record["range"];
  if (uri === null) {
    return null;
  }
  const range = toOneBasedRange(rangeValue);
  if (range === null) {
    return null;
  }
  const relative = mapUri(uri);
  if (relative !== null) {
    // Only in-workspace targets map back to workspace-relative paths.
    return { path: relative, range, external: false };
  }
  // Out-of-workspace and engine-internal URIs are represented
  // conservatively without absolute paths.
  const basename =
    uri
      .split("/")
      .filter((part) => part.length > 0)
      .pop() ?? "external";
  return { path: sanitizeControlCharacters(basename), range, external: true };
}
