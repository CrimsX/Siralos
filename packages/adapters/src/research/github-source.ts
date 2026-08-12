import {
  normalizeRepositoryOrigin,
  type ResearchBounds,
  type ResearchOutcome,
  type ResearchRequest,
  type ResearchSourcePort,
  type ResearchTransportPort,
} from "@siralos/core";
import {
  boundedErrorMessage,
  researchDocumentOutcome,
  transportErrorToResearchOutcome,
} from "./normalization.js";

/**
 * GitHub repository research source (Stage 3 milestone 5).
 *
 * Narrow, read-only scope:
 *
 *   (a) known file content — a request with `path` (validated relative, no
 *       "..", no absolute paths) and an optional `ref`; and
 *   (b) latest release notes — a request without `path` whose query/topic
 *       mentions "release" (the GitHub releases API).
 *
 * The GitHub origin (`owner/repo`, `https://github.com/owner/repo`, or
 * `.git` form) is carried by the request's required `query` field and
 * normalized with core's `normalizeRepositoryOrigin`; anything else is
 * refused before any fetch. `topic`/`path` select the resource.
 *
 * Revision semantics: a `ref` that is a full 40-hex commit sha is used
 * directly and recorded as `resolvedRevision` (immutable identity). Tags and
 * branches are NOT claimed as immutable commits: `resolvedRevision` stays
 * null and provenance records only `requestedRef`. No commit-resolution API
 * call is made in this milestone.
 *
 * Everything is bounded through the transport (`maxDownloadBytes`) and
 * `buildResearchDocument`; the source never throws.
 */

const COMMIT_SHA_PATTERN = /^[0-9a-fA-F]{40}$/;
const GITHUB_ORIGIN_PREFIX = "https://github.com/";

function resolveRef(ref: string | null):
  | {
      readonly ok: true;
      readonly refPart: string;
      readonly requestedRef: string | null;
      readonly resolvedRevision: string | null;
    }
  | { readonly ok: false; readonly reason: string } {
  if (ref === null) {
    return { ok: true, refPart: "HEAD", requestedRef: null, resolvedRevision: null };
  }
  // The ref is embedded into a URL path: reject traversal/control/absolute
  // forms so URL normalization can never fetch from a different repository
  // than the declared origin (defense in depth; the request model bounds
  // the length).
  if (
    ref.length === 0 ||
    ref.startsWith("/") ||
    ref.includes("\\") ||
    ref.includes("\0") ||
    ref.split("/").some((segment) => segment === ".." || segment === ".")
  ) {
    return { ok: false, reason: "the repository ref must be a plain ref name or commit sha" };
  }
  if (COMMIT_SHA_PATTERN.test(ref)) {
    return { ok: true, refPart: ref, requestedRef: ref, resolvedRevision: ref };
  }
  return { ok: true, refPart: ref, requestedRef: ref, resolvedRevision: null };
}

/** Encode each path segment (slashes preserved as separators). */
function encodePathSegments(value: string): string {
  return value
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

/** Defense-in-depth path validation (the service validates requests too). */
export function validateResearchPath(
  path: string,
): { readonly ok: true } | { readonly ok: false; readonly reason: string } {
  if (path.length === 0) {
    return { ok: false, reason: "a repository research request requires a non-empty path" };
  }
  if (path.startsWith("/") || path.includes("\\") || path.includes("\0")) {
    return { ok: false, reason: "the resource path must be relative with forward slashes" };
  }
  const segments = path.split("/");
  if (segments.some((segment) => segment === ".." || segment === ".")) {
    return { ok: false, reason: 'the resource path must not contain ".." or "." segments' };
  }
  return { ok: true };
}

export interface GitHubResearchSourceOptions {
  readonly transport: ResearchTransportPort;
  /** Clock for provenance timestamps (tests inject a fixed clock). */
  readonly now?: () => number;
}

export function createGitHubResearchSource(
  options: GitHubResearchSourceOptions,
): ResearchSourcePort {
  return {
    kind: "repository",
    id: "github",
    label: "GitHub repository research",
    async fetch(
      request: ResearchRequest,
      bounds: ResearchBounds,
      signal: AbortSignal,
    ): Promise<ResearchOutcome> {
      if (signal.aborted) {
        return { status: "cancelled" };
      }
      try {
        const originResult = normalizeRepositoryOrigin(request.query);
        if (!originResult.ok) {
          return { status: "refused", reason: `invalid repository origin: ${originResult.reason}` };
        }
        const repoPath = originResult.origin.slice(GITHUB_ORIGIN_PREFIX.length);
        const now = (options.now ?? Date.now)();
        const transportOptions = {
          maxBytes: Math.min(request.maxBytes ?? bounds.maxDownloadBytes, bounds.maxDownloadBytes),
          maxRedirects: bounds.maxRedirects,
          timeoutMs: bounds.timeoutMs,
          signal,
          allowedHosts: ["api.github.com", "raw.githubusercontent.com"],
        };
        const wantsRelease =
          request.path === null &&
          `${request.query} ${request.topic ?? ""}`.toLowerCase().includes("release");
        if (wantsRelease) {
          const url = `https://api.github.com/repos/${repoPath}/releases/latest`;
          const outcome = await options.transport.get(url, transportOptions);
          if (outcome.status !== "ok") {
            return transportErrorToResearchOutcome(outcome);
          }
          if (outcome.statusCode === 404) {
            return { status: "failed", reason: "resource not found" };
          }
          if (outcome.statusCode === 403 || outcome.statusCode === 429) {
            return { status: "failed", reason: "rate limited" };
          }
          if (outcome.statusCode < 200 || outcome.statusCode >= 300) {
            return { status: "failed", reason: `HTTP ${outcome.statusCode}` };
          }
          return researchDocumentOutcome(outcome, {
            source: request.source,
            title: null,
            provenance: {
              requestedRef: null,
              resolvedRevision: null,
              requestedVersion: null,
              usedVersion: null,
              fallback: false,
              fallbackReason: null,
              resource: url,
            },
            bounds,
            now,
          });
        }
        if (request.path === null) {
          return {
            status: "refused",
            reason: "a repository research request requires a path (or a release topic)",
          };
        }
        const pathResult = validateResearchPath(request.path);
        if (!pathResult.ok) {
          return { status: "refused", reason: pathResult.reason };
        }
        const refResolution = resolveRef(request.ref);
        if (!refResolution.ok) {
          return { status: "refused", reason: refResolution.reason };
        }
        const { refPart, requestedRef, resolvedRevision } = refResolution;
        const url = `https://raw.githubusercontent.com/${repoPath}/${encodePathSegments(refPart)}/${encodePathSegments(request.path)}`;
        const outcome = await options.transport.get(url, transportOptions);
        if (outcome.status !== "ok") {
          return transportErrorToResearchOutcome(outcome);
        }
        if (outcome.statusCode === 404) {
          return { status: "failed", reason: "resource not found" };
        }
        if (outcome.statusCode === 403 || outcome.statusCode === 429) {
          return { status: "failed", reason: "rate limited" };
        }
        if (outcome.statusCode < 200 || outcome.statusCode >= 300) {
          return { status: "failed", reason: `HTTP ${outcome.statusCode}` };
        }
        return researchDocumentOutcome(outcome, {
          source: request.source,
          title: null,
          provenance: {
            requestedRef,
            resolvedRevision,
            requestedVersion: null,
            usedVersion: null,
            fallback: false,
            fallbackReason: null,
            resource: url,
          },
          bounds,
          now,
        });
      } catch (error: unknown) {
        return { status: "failed", reason: boundedErrorMessage(error) };
      }
    },
  };
}
