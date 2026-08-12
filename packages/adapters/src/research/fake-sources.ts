import {
  normalizeRepositoryOrigin,
  type ResearchBounds,
  type ResearchContentType,
  type ResearchOutcome,
  type ResearchProvenance,
  type ResearchRequest,
  type ResearchSourcePort,
  type ResearchSourceRef,
} from "@siralos/core";
import { validateResearchPath } from "./github-source.js";
import { buildResearchDocument, classifyContentType } from "./normalization.js";

/**
 * Fake research sources (Stage 3 milestone 5).
 *
 * Deterministic, network-free stand-ins for the real GitHub and Godot docs
 * sources — the DEFAULT for tests and the behavior harness. Both serve fixed
 * fixtures with the same provenance semantics as their real counterparts
 * (commit shas get `resolvedRevision`; branches/tags do not; docs fallbacks
 * are explicitly marked), enforce the same bounds via
 * `buildResearchDocument`, and honor the abort signal at entry. They never
 * throw.
 */

export interface GodotDocsPageFixture {
  readonly title: string;
  readonly sections: readonly { readonly heading: string | null; readonly text: string }[];
}

export interface GodotDocsFallback {
  readonly usedVersion: string;
  readonly reason: string;
}

export interface GodotDocsFixture {
  /** Version → topic → page. */
  readonly versions: Readonly<Record<string, Readonly<Record<string, GodotDocsPageFixture>>>>;
  /**
   * Explicit fallback chain per requested version: when the exact version is
   * absent, serve `usedVersion` with `fallback: true` + `reason`.
   */
  readonly fallbacks?: Readonly<Record<string, GodotDocsFallback>>;
}

export interface FakeGodotDocsSourceOptions {
  /** Clock for provenance timestamps (tests inject a fixed clock). */
  readonly now?: () => number;
}

function renderPageMarkdown(page: GodotDocsPageFixture): string {
  return page.sections
    .map((section) =>
      section.heading === null ? section.text : `# ${section.heading}\n\n${section.text}`,
    )
    .join("\n\n");
}

/**
 * Fake Godot docs: serves version-matched topics from fixtures; when the
 * exact version is absent, falls back through the fixture's explicit fallback
 * chain with `fallback: true` + reason. Unknown version/topic → `failed`
 * "not found". A topic is required (the fake never serves search pages).
 */
export function createFakeGodotDocsSource(
  fixtures: GodotDocsFixture,
  options: FakeGodotDocsSourceOptions = {},
): ResearchSourcePort {
  return {
    kind: "godot-docs",
    id: "godot-docs-fake",
    label: "Fake Godot docs",
    fetch(
      request: ResearchRequest,
      bounds: ResearchBounds,
      signal: AbortSignal,
    ): Promise<ResearchOutcome> {
      if (signal.aborted) {
        return Promise.resolve({ status: "cancelled" });
      }
      const requestedVersion = request.version ?? "stable";
      const topic = (request.topic ?? "").trim();
      if (topic.length === 0) {
        return Promise.resolve({ status: "failed", reason: "not found" });
      }
      const direct = fixtures.versions[requestedVersion]?.[topic];
      if (direct !== undefined) {
        return Promise.resolve(
          servePage(request, bounds, {
            page: direct,
            usedVersion: requestedVersion,
            fallback: false,
            fallbackReason: null,
            resource: `docs:${requestedVersion}:${topic}`,
            now: (options.now ?? Date.now)(),
          }),
        );
      }
      const fallback = fixtures.fallbacks?.[requestedVersion];
      if (fallback !== undefined) {
        const page = fixtures.versions[fallback.usedVersion]?.[topic];
        if (page !== undefined) {
          return Promise.resolve(
            servePage(request, bounds, {
              page,
              usedVersion: fallback.usedVersion,
              fallback: true,
              fallbackReason: fallback.reason,
              resource: `docs:${fallback.usedVersion}:${topic}`,
              now: (options.now ?? Date.now)(),
            }),
          );
        }
      }
      return Promise.resolve({
        status: "failed",
        reason: "not found",
      });
    },
  };
}

function servePage(
  request: ResearchRequest,
  bounds: ResearchBounds,
  opts: {
    readonly page: GodotDocsPageFixture;
    readonly usedVersion: string;
    readonly fallback: boolean;
    readonly fallbackReason: string | null;
    readonly resource: string;
    readonly now: number;
  },
): ResearchOutcome {
  const source = request.source;
  const rawText = renderPageMarkdown(opts.page);
  const document = buildResearchDocument({
    source,
    title: opts.page.title,
    contentType: "text/markdown",
    rawText,
    rawByteLength: new TextEncoder().encode(rawText).length,
    provenance: {
      source,
      requestedRef: null,
      resolvedRevision: null,
      requestedVersion: request.version,
      usedVersion: opts.usedVersion,
      fallback: opts.fallback,
      fallbackReason: opts.fallbackReason,
      fetchedAtMs: opts.now,
      resource: opts.resource,
    },
    bounds,
    now: opts.now,
  });
  return { status: "document", document };
}

export interface FakeRepositoryFileFixture {
  readonly contentType: string;
  readonly body: string;
}

export interface FakeRepositoryReleaseFixture {
  readonly body: string;
}

/**
 * Fixture set keyed by the canonical `owner/repo` origin. `files` is keyed by
 * ref (use "HEAD" for the default branch) then by path; `releases` maps
 * version → body (the numerically-latest version is served as "latest").
 */
export type FakeRepositoryResearchFixture = Readonly<
  Record<
    string,
    {
      readonly releases: Readonly<Record<string, FakeRepositoryReleaseFixture>>;
      readonly files: Readonly<Record<string, Readonly<Record<string, FakeRepositoryFileFixture>>>>;
    }
  >
>;

export interface FakeRepositorySourceOptions {
  /** Clock for provenance timestamps (tests inject a fixed clock). */
  readonly now?: () => number;
}

function pickLatestVersion(versions: readonly string[]): string | null {
  let best: string | null = null;
  for (const version of versions) {
    if (best === null || compareVersions(version, best) > 0) {
      best = version;
    }
  }
  return best;
}

function compareVersions(a: string, b: string): number {
  const aParts = a.split(".").map((part) => (/^\d+$/.test(part) ? Number(part) : Number.NaN));
  const bParts = b.split(".").map((part) => (/^\d+$/.test(part) ? Number(part) : Number.NaN));
  const length = Math.max(aParts.length, bParts.length);
  for (let index = 0; index < length; index += 1) {
    const aValue = aParts[index] ?? 0;
    const bValue = bParts[index] ?? 0;
    if (Number.isNaN(aValue) || Number.isNaN(bValue)) {
      return a.localeCompare(b);
    }
    if (aValue !== bValue) {
      return aValue - bValue;
    }
  }
  return 0;
}

const COMMIT_SHA_PATTERN = /^[0-9a-fA-F]{40}$/;
const GITHUB_ORIGIN_PREFIX = "https://github.com/";

/**
 * Fake GitHub repository research: serves fixture file content at `path`+`ref`
 * and the latest release, with provenance mirroring the real GitHub source
 * (commit shas → `resolvedRevision`; branches/tags → null). Unclassifiable
 * fixture content types → `unsupported-content`.
 */
export function createFakeRepositorySource(
  fixtures: FakeRepositoryResearchFixture,
  options: FakeRepositorySourceOptions = {},
): ResearchSourcePort {
  return {
    kind: "repository",
    id: "github-fake",
    label: "Fake GitHub repository research",
    fetch(
      request: ResearchRequest,
      bounds: ResearchBounds,
      signal: AbortSignal,
    ): Promise<ResearchOutcome> {
      if (signal.aborted) {
        return Promise.resolve({ status: "cancelled" });
      }
      const originResult = normalizeRepositoryOrigin(request.query);
      if (!originResult.ok) {
        return Promise.resolve({
          status: "refused",
          reason: `invalid repository origin: ${originResult.reason}`,
        });
      }
      const repoKey = originResult.origin.slice(GITHUB_ORIGIN_PREFIX.length);
      const fixture = fixtures[repoKey];
      if (fixture === undefined) {
        return Promise.resolve({
          status: "failed",
          reason: `repository "${repoKey}" is not in the fixture set`,
        });
      }
      const now = (options.now ?? Date.now)();
      const wantsRelease =
        request.path === null &&
        `${request.query} ${request.topic ?? ""}`.toLowerCase().includes("release");
      if (wantsRelease) {
        const latest = pickLatestVersion(Object.keys(fixture.releases));
        if (latest === null) {
          return Promise.resolve({ status: "failed", reason: "resource not found" });
        }
        const release = fixture.releases[latest];
        if (release === undefined) {
          return Promise.resolve({ status: "failed", reason: "resource not found" });
        }
        return Promise.resolve(
          serveRepositoryDocument(bounds, {
            source: request.source,
            title: null,
            contentType: "text/markdown",
            rawText: release.body,
            provenance: {
              source: request.source,
              requestedRef: null,
              resolvedRevision: null,
              requestedVersion: null,
              usedVersion: latest,
              fallback: false,
              fallbackReason: null,
              fetchedAtMs: now,
              resource: `releases:latest:${latest}`,
            },
            now,
          }),
        );
      }
      if (request.path === null) {
        return Promise.resolve({
          status: "refused",
          reason: "a repository research request requires a path (or a release topic)",
        });
      }
      const pathResult = validateResearchPath(request.path);
      if (!pathResult.ok) {
        return Promise.resolve({ status: "refused", reason: pathResult.reason });
      }
      const ref = request.ref ?? "HEAD";
      const file = fixture.files[ref]?.[request.path];
      if (file === undefined) {
        return Promise.resolve({ status: "failed", reason: "resource not found" });
      }
      const contentType = classifyContentType(file.contentType);
      if (contentType === null) {
        return Promise.resolve({
          status: "unsupported-content",
          reason: `unsupported content type ${file.contentType}`,
        });
      }
      const resolvedRevision = COMMIT_SHA_PATTERN.test(ref) ? ref : null;
      return Promise.resolve(
        serveRepositoryDocument(bounds, {
          source: request.source,
          title: null,
          contentType,
          rawText: file.body,
          provenance: {
            source: request.source,
            requestedRef: request.ref,
            resolvedRevision,
            requestedVersion: null,
            usedVersion: null,
            fallback: false,
            fallbackReason: null,
            fetchedAtMs: now,
            resource: `files:${ref}:${request.path}`,
          },
          now,
        }),
      );
    },
  };
}

function serveRepositoryDocument(
  bounds: ResearchBounds,
  opts: {
    readonly source: ResearchSourceRef;
    readonly title: string | null;
    readonly contentType: ResearchContentType;
    readonly rawText: string;
    readonly provenance: ResearchProvenance;
    readonly now: number;
  },
): ResearchOutcome {
  const document = buildResearchDocument({
    source: opts.source,
    title: opts.title,
    contentType: opts.contentType,
    rawText: opts.rawText,
    rawByteLength: new TextEncoder().encode(opts.rawText).length,
    provenance: opts.provenance,
    bounds,
    now: opts.now,
  });
  return { status: "document", document };
}
