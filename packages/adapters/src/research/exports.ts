/**
 * Research source adapters — public surface (Stage 3 milestone 5).
 *
 * The package-level `packages/adapters/src/index.ts` is owned by another
 * writer this wave; the orchestrator merges these exports into it
 * mechanically. This module is the single authoritative export list for the
 * research adapters.
 */

export {
  TRUNCATION_MARKER,
  boundedErrorMessage,
  buildResearchDocument,
  classifyContentType,
  normalizeHtmlToSections,
  normalizeJsonToSections,
  normalizeMarkdownToSections,
  researchDocumentOutcome,
  transportErrorToResearchOutcome,
  type BuildResearchDocumentOptions,
  type HtmlNormalizationResult,
  type NormalizationResult,
  type ResearchDocumentOutcomeOptions,
} from "./normalization.js";
export {
  createFakeTransport,
  createNodeHttpsTransport,
  type FakeTransportRoute,
  type FakeTransportRoutes,
} from "./http-transport.js";
export {
  createGitHubResearchSource,
  validateResearchPath,
  type GitHubResearchSourceOptions,
} from "./github-source.js";
export {
  buildDocsUrl,
  createGodotDocsResearchSource,
  resolveDocsVersion,
  type DocsVersionResolution,
  type GodotDocsResearchSourceOptions,
} from "./godot-docs-source.js";
export {
  createFakeGodotDocsSource,
  createFakeRepositorySource,
  type FakeGodotDocsSourceOptions,
  type FakeRepositoryFileFixture,
  type FakeRepositoryReleaseFixture,
  type FakeRepositoryResearchFixture,
  type FakeRepositorySourceOptions,
  type GodotDocsFallback,
  type GodotDocsFixture,
  type GodotDocsPageFixture,
} from "./fake-sources.js";
