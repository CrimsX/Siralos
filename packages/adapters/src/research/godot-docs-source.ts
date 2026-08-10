import {
  type ResearchBounds,
  type ResearchOutcome,
  type ResearchRequest,
  type ResearchSourcePort,
  type ResearchTransportPort,
} from "@solaris/core";
import {
  boundedErrorMessage,
  researchDocumentOutcome,
  transportErrorToResearchOutcome,
} from "./normalization.js";

/**
 * Godot official documentation source (Stage 3 milestone 5).
 *
 * IMPORTANT — evidence-source distinction: the Godot API index
 * (version-matched, from the engine dump), Godot docs, and Godot source are
 * DISTINCT evidence sources; this adapter covers ONLY the published
 * documentation at docs.godotengine.org.
 *
 * Version awareness: the docs site publishes per minor version
 * (`/en/4.7/`, `/en/stable/`). Resolution preference order (documented
 * chain): requested version exact → requested minor → `stable`. Patch
 * versions are not published, so `4.7.1` maps to `4.7`; one-segment or
 * unpublished versions fall back to `stable`. ANY fallback is EXPLICITLY
 * marked in provenance (`fallback: true` + `fallbackReason`) — mismatched
 * guidance is never served silently.
 *
 * Topic mapping: a `topic` yields the class page
 * `.../classes/class_{topic}.html` (`CharacterBody2D` →
 * `class_characterbody2d`; a topic already prefixed with `class_` is used
 * as-is); a request without a topic yields the site search page
 * `.../search.html?q={query}` (search pages are normalized text — kept
 * narrow). The exact fetched URL is recorded in provenance `resource`.
 *
 * Everything is bounded through the transport and `buildResearchDocument`;
 * the source never throws.
 */

const DOCS_BASE = "https://docs.godotengine.org/en";

export interface DocsVersionResolution {
  readonly usedVersion: string;
  readonly fallback: boolean;
  readonly fallbackReason: string | null;
}

/**
 * Map a requested version to a published docs version. Null (no request)
 * → `stable` with no fallback marking (nothing was asked for).
 */
export function resolveDocsVersion(requested: string | null): DocsVersionResolution {
  if (requested === null) {
    return { usedVersion: "stable", fallback: false, fallbackReason: null };
  }
  const match = /^(\d+)(?:\.(\d+))?(?:\.(\d+))?(.*)$/.exec(requested);
  if (match === null) {
    return {
      usedVersion: "stable",
      fallback: true,
      fallbackReason: "requested version is not published on the docs site; using stable",
    };
  }
  const major = match[1] ?? "";
  const minor = match[2];
  const patch = match[3];
  const rest = match[4] ?? "";
  if (minor === undefined) {
    // One-segment major ("4") or major with a non-numeric tail ("4.x"):
    // docs are published per minor version.
    return {
      usedVersion: "stable",
      fallback: true,
      fallbackReason: "docs are published per minor version; using stable",
    };
  }
  if (patch !== undefined) {
    return {
      usedVersion: `${major}.${minor}`,
      fallback: true,
      fallbackReason: "patch version not published; using minor docs",
    };
  }
  if (rest === "" || rest === "-stable") {
    return { usedVersion: `${major}.${minor}`, fallback: false, fallbackReason: null };
  }
  return {
    usedVersion: "stable",
    fallback: true,
    fallbackReason: "requested version is not published on the docs site; using stable",
  };
}

export function buildDocsUrl(usedVersion: string, request: ResearchRequest): string {
  const topic = (request.topic ?? "").trim();
  if (topic.length > 0) {
    const slug = topic.toLowerCase();
    const classSlug = slug.startsWith("class_") ? slug : `class_${slug}`;
    return `${DOCS_BASE}/${usedVersion}/classes/${encodeURIComponent(classSlug)}.html`;
  }
  return `${DOCS_BASE}/${usedVersion}/search.html?q=${encodeURIComponent(request.query)}`;
}

export interface GodotDocsResearchSourceOptions {
  readonly transport: ResearchTransportPort;
  /** Clock for provenance timestamps (tests inject a fixed clock). */
  readonly now?: () => number;
}

export function createGodotDocsResearchSource(
  options: GodotDocsResearchSourceOptions,
): ResearchSourcePort {
  return {
    kind: "godot-docs",
    id: "godot-docs",
    label: "Godot official documentation",
    async fetch(
      request: ResearchRequest,
      bounds: ResearchBounds,
      signal: AbortSignal,
    ): Promise<ResearchOutcome> {
      if (signal.aborted) {
        return { status: "cancelled" };
      }
      try {
        const resolution = resolveDocsVersion(request.version);
        const url = buildDocsUrl(resolution.usedVersion, request);
        const now = (options.now ?? Date.now)();
        const outcome = await options.transport.get(url, {
          maxBytes: Math.min(request.maxBytes ?? bounds.maxDownloadBytes, bounds.maxDownloadBytes),
          maxRedirects: bounds.maxRedirects,
          timeoutMs: bounds.timeoutMs,
          signal,
          allowedHosts: ["docs.godotengine.org"],
        });
        if (outcome.status !== "ok") {
          return transportErrorToResearchOutcome(outcome);
        }
        if (outcome.statusCode === 404) {
          return { status: "failed", reason: "not found" };
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
            requestedVersion: request.version,
            usedVersion: resolution.usedVersion,
            fallback: resolution.fallback,
            fallbackReason: resolution.fallbackReason,
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
