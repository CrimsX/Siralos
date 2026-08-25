import { readFileSync } from "node:fs";
import {
  computeResearchDocumentId,
  defaultResearchBounds,
} from "../../../packages/core/src/research/research-model.ts";
import {
  DEFAULT_RESEARCH_VIEW_MAX_BYTES,
  createResearchService,
  formatResearchEvidenceView,
} from "../../../packages/core/src/research/research-service.ts";
import { createDefaultPolicy } from "../../../packages/core/src/security/default-policy.ts";
import {
  SANDBOX_PROFILE_IDS,
  getBuiltInProfile,
} from "../../../packages/core/src/security/profile.ts";
import { evaluatePermission } from "../../../packages/core/src/security/permission-evaluator.ts";
import {
  TRUNCATION_MARKER,
  buildResearchDocument,
  classifyContentType,
  normalizeJsonToSections,
  normalizeMarkdownToSections,
} from "../../../packages/adapters/src/research/normalization.ts";
import {
  createFakeGodotDocsSource,
  createFakeRepositorySource,
} from "../../../packages/adapters/src/research/fake-sources.ts";

const input = JSON.parse(readFileSync(0, "utf8"));
const NOW_MS = Number(input.nowMs ?? 1_700_000_000_000);

const COMMIT_SHA = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";

function allowPolicy() {
  return {
    rules: { ...createDefaultPolicy("inspect").rules, "research.fetch": "allow" },
  };
}

function askPolicy() {
  return {
    rules: { ...createDefaultPolicy("inspect").rules, "research.fetch": "ask" },
  };
}

/** Source-port spy proving whether the port was ever invoked. */
function spyingSource(inner) {
  const state = { calls: 0 };
  return {
    state,
    kind: inner.kind,
    id: inner.id,
    label: inner.label,
    fetch(request, bounds, signal) {
      state.calls += 1;
      return inner.fetch(request, bounds, signal);
    },
  };
}

function staticSource(kind, id, label, outcomeFactory) {
  return {
    kind,
    id,
    label,
    fetch(request, bounds, signal) {
      return Promise.resolve(outcomeFactory(request, bounds, signal));
    },
  };
}

const GODOT_FIXTURE = {
  versions: {
    4.3: {
      "first-person": {
        title: "First person tutorial",
        sections: [
          { heading: "Setup", text: "Install Godot 4.3 to follow along." },
          { heading: null, text: "Appendix notes." },
        ],
      },
    },
  },
  fallbacks: {
    4.4: { usedVersion: "4.3", reason: "version 4.4 is not published; serving 4.3" },
  },
};

const bodyOf = (tag) => `doc-${tag}${"x".repeat(24)}`;
const REPO_FIXTURE = {
  "owner/repo": {
    releases: {},
    files: {
      HEAD: {
        "notes/doc-1.md": { contentType: "text/markdown", body: bodyOf("aa") },
        "notes/doc-2.md": { contentType: "text/markdown", body: bodyOf("bb") },
        "notes/doc-3.md": { contentType: "text/markdown", body: bodyOf("cc") },
        "notes/doc-4.md": { contentType: "text/markdown", body: bodyOf("dd") },
        README: { contentType: "text/markdown", body: "Head readme body." },
      },
      [COMMIT_SHA]: {
        README: { contentType: "text/markdown", body: "Pinned readme body." },
      },
      main: {
        README: { contentType: "text/markdown", body: "Main branch body." },
      },
    },
  },
};

function fakeSources() {
  return [
    createFakeGodotDocsSource(GODOT_FIXTURE, { now: () => NOW_MS }),
    createFakeRepositorySource(REPO_FIXTURE, { now: () => NOW_MS }),
  ];
}

function baseRequest(overrides = {}) {
  return {
    source: { kind: "godot-docs", id: "godot-docs-fake", label: "Fake Godot docs" },
    query: "hello",
    ...overrides,
  };
}

function serviceWith(options = {}) {
  const sources = options.sources ?? fakeSources();
  return createResearchService({
    policy: options.policy ?? allowPolicy(),
    profile: getBuiltInProfile("inspect"),
    sources,
    currentTask: options.currentTask ?? (() => ({ taskId: "task-1", taskContractRevision: 1 })),
    bounds: options.bounds,
    maxEvidenceBytes: options.maxEvidenceBytes,
  });
}

function provenanceSummary(document) {
  const provenance = document.provenance;
  return {
    requestedRef: provenance.requestedRef,
    resolvedRevision: provenance.resolvedRevision,
    usedVersion: provenance.usedVersion,
    fallback: provenance.fallback,
    fallbackReason: provenance.fallbackReason,
    resource: provenance.resource,
  };
}

async function runCase(inputCase) {
  switch (inputCase.name) {
    case "denied-by-default-gate-first": {
      const profiles = [];
      for (const profileId of SANDBOX_PROFILE_IDS) {
        const source = spyingSource(fakeSources()[0]);
        const service = serviceWith({
          policy: createDefaultPolicy(profileId),
          sources: [source],
        });
        const result = await service.fetch(baseRequest());
        profiles.push({
          profileId,
          status: result.status,
          reason: result.reason ?? null,
        });
      }
      const askSource = spyingSource(fakeSources()[0]);
      const askService = serviceWith({ policy: askPolicy(), sources: [askSource] });
      const askResult = await askService.fetch(baseRequest());
      const evaluation = evaluatePermission(
        "research.fetch",
        createDefaultPolicy("inspect"),
        getBuiltInProfile("inspect"),
      );
      return {
        name: inputCase.name,
        profiles,
        denyBranch: { status: profiles[0].status, reason: profiles[0].reason },
        askBranch: { status: askResult.status, reason: askResult.reason ?? null },
        evaluatorDecisionForInspect: evaluation.decision,
        gateSpyCalls: askSource.state.calls,
      };
    }
    case "request-validation-bounds": {
      const source = spyingSource(
        staticSource("fake", "src-1", "Spy", () => ({
          status: "failed",
          reason: "unreachable",
        })),
      );
      const service = serviceWith({ sources: [source] });
      const invalid = [
        ["empty-query", { query: "   " }],
        ["oversized-query", { query: "x".repeat(513) }],
        ["absolute-path", { path: "/etc/passwd" }],
        ["backslash-path", { path: "a\\b" }],
        ["nul-path", { path: "a\0b" }],
        ["dot-path", { path: "." }],
        ["dotdot-path", { path: ".." }],
        ["dotdot-segment", { path: "a/../b" }],
        ["oversized-path", { path: "a".repeat(1025) }],
        ["oversized-ref", { ref: "r".repeat(257) }],
        ["malformed-version", { version: "four" }],
        ["zero-max-bytes", { maxBytes: 0 }],
        ["negative-max-bytes", { maxBytes: -5 }],
        ["string-max-bytes", { maxBytes: "10" }],
      ];
      const results = [];
      for (const [tag, override] of invalid) {
        const result = await service.fetch(baseRequest(override));
        results.push({
          tag,
          status: result.status,
          reason: result.reason ?? null,
        });
      }
      let normalizedMaxBytes = null;
      const capture = spyingSource(
        staticSource("fake", "capture", "Capture", (request) => {
          normalizedMaxBytes = request.maxBytes;
          return { status: "failed", reason: "capture-only" };
        }),
      );
      const captureService = serviceWith({ sources: [capture] });
      await captureService.fetch(
        baseRequest({
          source: { kind: "fake", id: "capture", label: "Capture" },
          maxBytes: 10.9,
        }),
      );
      return {
        name: inputCase.name,
        results,
        validationSpyCalls: source.state.calls,
        normalizedMaxBytes,
      };
    }
    case "source-matching-and-refusal": {
      const service = serviceWith();
      const byId = await service.fetch(
        baseRequest({
          source: { kind: "godot-docs", id: "godot-docs-fake", label: "WRONG LABEL" },
          topic: "first-person",
          version: "4.3",
        }),
      );
      const byLabel = await service.fetch(
        baseRequest({
          source: { kind: "godot-docs", id: "unknown-id", label: "Fake Godot docs" },
          topic: "first-person",
          version: "4.3",
        }),
      );
      const unconfigured = await service.fetch(
        baseRequest({
          source: { kind: "repository", id: "nope", label: "Nope" },
        }),
      );
      return {
        name: inputCase.name,
        byIdStatus: byId.status,
        byIdDocumentSourceId: byId.document?.source.id ?? null,
        byLabelStatus: byLabel.status,
        byLabelDocumentSourceId: byLabel.document?.source.id ?? null,
        unconfiguredStatus: unconfigured.status,
        unconfiguredReason: unconfigured.reason ?? null,
      };
    }
    case "task-binding-required-fail-closed": {
      const noneService = serviceWith({ currentTask: () => null });
      const noneResult = await noneService.fetch(
        baseRequest({ topic: "first-person", version: "4.3" }),
      );
      const zeroRevisionService = serviceWith({
        currentTask: () => ({ taskId: "t0", taskContractRevision: 0 }),
      });
      const zeroResult = await zeroRevisionService.fetch(
        baseRequest({ topic: "first-person", version: "4.3" }),
      );
      const blankService = serviceWith({
        currentTask: () => ({ taskId: "   ", taskContractRevision: 1 }),
      });
      const blankResult = await blankService.fetch(
        baseRequest({ topic: "first-person", version: "4.3" }),
      );
      const evidenceService = serviceWith();
      const okResult = await evidenceService.fetch(
        baseRequest({ topic: "first-person", version: "4.3" }),
      );
      return {
        name: inputCase.name,
        noneTaskStatus: noneResult.status,
        noneTaskReason: noneResult.reason ?? null,
        zeroRevisionStatus: zeroResult.status,
        blankTaskIdStatus: blankResult.status,
        evidenceTaskId: okResult.evidence?.taskId ?? null,
        evidenceRevision: okResult.evidence?.taskContractRevision ?? null,
      };
    }
    case "stale-result-discarded": {
      let flipped = false;
      const service = serviceWith({
        currentTask: () => {
          if (!flipped) {
            flipped = true;
            return { taskId: "t1", taskContractRevision: 1 };
          }
          return { taskId: "t2", taskContractRevision: 1 };
        },
      });
      const result = await service.fetch(baseRequest({ topic: "first-person", version: "4.3" }));
      return {
        name: inputCase.name,
        status: result.status,
        reason: result.reason ?? null,
        retainedEvidenceCount: service.latestEvidence().length,
      };
    }
    case "timeout-cancelled-precedence": {
      const abortSource = spyingSource(
        staticSource("fake", "abort-probe", "AbortProbe", () => ({
          status: "failed",
          reason: "unreachable",
        })),
      );
      const abortController = new AbortController();
      abortController.abort();
      const abortService = serviceWith({ sources: [abortSource] });
      const aborted = await abortService.fetch(
        baseRequest({ source: { kind: "fake", id: "abort-probe", label: "AbortProbe" } }),
        {
          signal: abortController.signal,
        },
      );
      const timeoutService = serviceWith({
        sources: [staticSource("fake", "slow", "Slow", () => ({ status: "timeout" }))],
        bounds: { ...defaultResearchBounds(), timeoutMs: 1234 },
      });
      const timeout = await timeoutService.fetch(
        baseRequest({ source: { kind: "fake", id: "slow", label: "Slow" } }),
      );
      const cancelDuringFetch = spyingSource(
        staticSource("fake", "cancel-probe", "CancelProbe", () => ({
          status: "cancelled",
        })),
      );
      const duringService = serviceWith({ sources: [cancelDuringFetch] });
      const cancelled = await duringService.fetch(
        baseRequest({ source: { kind: "fake", id: "cancel-probe", label: "CancelProbe" } }),
      );
      return {
        name: inputCase.name,
        abortedPreFetchStatus: aborted.status,
        abortedPreFetchReason: aborted.reason ?? null,
        abortedSpyCalls: abortSource.state.calls,
        timeoutStatus: timeout.status,
        timeoutReason: timeout.reason ?? null,
        cancelledStatus: cancelled.status,
        cancelledReason: cancelled.reason ?? null,
        activeRequestsAfter: timeoutService.activeRequestCount(),
      };
    }
    case "normalization-bounds-disclosure": {
      const markdownBounds = {
        ...defaultResearchBounds(),
        maxSections: 2,
        maxSectionTextBytes: 4096,
        maxHeadingBytes: 512,
      };
      const sectionLimit = normalizeMarkdownToSections(
        "# One\n\ntext one\n\n# Two\n\ntext two\n\n# Three\n\ntext three",
        markdownBounds,
      );
      const headingBounds = {
        ...defaultResearchBounds(),
        maxHeadingBytes: 12,
      };
      const headingBound = normalizeMarkdownToSections(
        "# A very long heading here\n\nbody",
        headingBounds,
      );
      const jsonBody = normalizeJsonToSections('{"body":"hello world"}', defaultResearchBounds());
      const jsonDescription = normalizeJsonToSections(
        '{"description":"desc-text"}',
        defaultResearchBounds(),
      );
      const jsonObject = normalizeJsonToSections('{"z":1,"a":"two"}', defaultResearchBounds());
      const jsonInvalid = normalizeJsonToSections("<not-json>", defaultResearchBounds());
      // text/plain truncation is exercised through the full document
      // builder (the section normalizer itself is module-private).
      const plainSource = { kind: "fake", id: "src-1", label: "Src" };
      const plainProvenance = {
        source: plainSource,
        requestedRef: null,
        resolvedRevision: null,
        requestedVersion: null,
        usedVersion: null,
        fallback: false,
        fallbackReason: null,
        fetchedAtMs: NOW_MS,
        resource: "res",
      };
      const plainDoc = buildResearchDocument({
        source: plainSource,
        title: null,
        contentType: "text/plain",
        rawText: "p".repeat(30),
        rawByteLength: 30,
        provenance: plainProvenance,
        bounds: { ...defaultResearchBounds(), maxSectionTextBytes: 20 },
        now: NOW_MS,
      });
      const plainFirst = plainDoc.sections[0];
      const plainOverflow = {
        byteLength: plainFirst?.byteLength ?? null,
        truncated: plainDoc.truncated,
        reason: plainDoc.truncationReason,
        endsWithMarker: plainFirst?.text.endsWith(TRUNCATION_MARKER) ?? false,
      };
      const classification = [
        "text/markdown",
        "text/html; charset=utf-8",
        "TEXT/PLAIN",
        "application/pdf",
        null,
      ].map((raw) => ({
        tag: raw === null ? "null" : raw,
        contentType: classifyContentType(raw),
      }));
      const docBounds = { ...defaultResearchBounds(), maxDocumentBytes: 260 };
      const built = buildResearchDocument({
        source: { kind: "fake", id: "src-1", label: "Src" },
        title: "T",
        contentType: "text/markdown",
        rawText: "# A\n\naaa\n\n# B\n\nbbb\n\n# C\n\nccc",
        rawByteLength: 30,
        provenance: {
          source: { kind: "fake", id: "src-1", label: "Src" },
          requestedRef: null,
          resolvedRevision: null,
          requestedVersion: null,
          usedVersion: null,
          fallback: false,
          fallbackReason: null,
          fetchedAtMs: NOW_MS,
          resource: "res",
        },
        bounds: docBounds,
        now: NOW_MS,
      });
      const recomputed = (
        await import("../../../packages/core/src/research/research-model.ts")
      ).computeResearchDocumentContentDigest({
        title: built.title,
        contentType: built.contentType,
        sections: built.sections,
      });
      const idA = computeResearchDocumentId("src-1", "digest-input");
      const idB = computeResearchDocumentId("src-1", "digest-input");
      return {
        name: inputCase.name,
        sectionLimit: {
          count: sectionLimit.sections.length,
          truncated: sectionLimit.truncated,
          reason: sectionLimit.reason,
          lastEndsWithMarker:
            sectionLimit.sections.at(-1)?.text.endsWith(TRUNCATION_MARKER) ?? false,
        },
        headingBound: {
          headingByteLength: new TextEncoder().encode(headingBound.sections[0]?.heading ?? "")
            .length,
          truncated: headingBound.truncated,
          reason: headingBound.reason,
        },
        jsonCases: [
          { tag: "body", text: jsonBody.sections[0]?.text ?? null },
          { tag: "description", text: jsonDescription.sections[0]?.text ?? null },
          {
            tag: "object-no-body",
            byteLength: jsonObject.sections[0]?.byteLength ?? null,
            multiline: (jsonObject.sections[0]?.text ?? "").includes("\n"),
          },
          { tag: "invalid", text: jsonInvalid.sections[0]?.text ?? null },
        ],
        plainOverflow: {
          byteLength: plainOverflow.byteLength,
          truncated: plainOverflow.truncated,
          reason: plainOverflow.reason,
          endsWithMarker: plainOverflow.endsWithMarker,
        },
        classification,
        digestCheck: {
          truncated: built.truncated,
          truncationReason: built.truncationReason,
          sectionCount: built.sections.length,
          contentDigestMatchesFinalContent: recomputed === built.contentDigest,
        },
        idSample: idA,
        idDeterministic: idA === idB,
        idFormatOk: /^rd_[0-9a-f]{24}$/.test(idA),
      };
    }
    case "provenance-fallback-semantics": {
      const service = serviceWith();
      const direct = await service.fetch(baseRequest({ topic: "first-person", version: "4.3" }));
      const fallback = await service.fetch(baseRequest({ topic: "first-person", version: "4.4" }));
      const unknownTopic = await service.fetch(
        baseRequest({ topic: "missing-topic", version: "4.3" }),
      );
      const commitPin = await service.fetch({
        source: { kind: "repository", id: "github-fake", label: "Fake GitHub repository research" },
        query: "owner/repo",
        path: "README",
        ref: COMMIT_SHA,
      });
      const branchPin = await service.fetch({
        source: { kind: "repository", id: "github-fake", label: "Fake GitHub repository research" },
        query: "owner/repo",
        path: "README",
        ref: "main",
      });
      return {
        name: inputCase.name,
        direct: direct.document ? provenanceSummary(direct.document) : null,
        directFetchedAtMatchesClock: direct.document?.fetchedAtMs === NOW_MS,
        fallbackCase: fallback.document ? provenanceSummary(fallback.document) : null,
        unknownTopicStatus: unknownTopic.status,
        unknownTopicReason: unknownTopic.reason ?? null,
        commitPin: commitPin.document ? provenanceSummary(commitPin.document) : null,
        branchPin: branchPin.document ? provenanceSummary(branchPin.document) : null,
      };
    }
    case "evidence-ring-retention": {
      const service = serviceWith({ maxEvidenceBytes: 64 });
      const idsSeen = [];
      for (let index = 1; index <= 4; index += 1) {
        const result = await service.fetch({
          source: {
            kind: "repository",
            id: "github-fake",
            label: "Fake GitHub repository research",
          },
          query: "owner/repo",
          path: `notes/doc-${index}.md`,
        });
        if (result.status !== "document") {
          throw new Error(`ring fixture fetch ${index} returned ${result.status}`);
        }
        idsSeen.push(result.evidence.evidenceId);
      }
      const snapshots = service.latestEvidence();
      const snapshotDetached = (() => {
        const copy = service.latestEvidence();
        copy.push({ sentinel: true });
        return service.latestEvidence().length === snapshots.length;
      })();
      return {
        name: inputCase.name,
        idsSeen,
        retainedIds: snapshots.map((entry) => entry.evidenceId),
        excerptByteLengths: snapshots.map((entry) => entry.byteLength),
        truncatedFlags: snapshots.map((entry) => entry.truncated),
        sequenceOrdering: snapshots.every(
          (entry, index) => index === 0 || snapshots[index - 1].evidenceId < entry.evidenceId,
        ),
        snapshotDetached,
      };
    }
    case "evidence-view-rendering": {
      const service = serviceWith();
      const result = await service.fetch(baseRequest({ topic: "first-person", version: "4.3" }));
      if (result.status !== "document") {
        throw new Error(`view fixture fetch returned ${result.status}`);
      }
      const view = formatResearchEvidenceView(result.evidence);
      const bounded = formatResearchEvidenceView(result.evidence, { maxBytes: 48 });
      return {
        name: inputCase.name,
        view,
        defaultMaxBytes: DEFAULT_RESEARCH_VIEW_MAX_BYTES,
        boundedView: bounded,
        boundedTruncated: bounded.length < view.length,
      };
    }
    default:
      throw new Error(`unknown research-policy fixture case ${inputCase.name}`);
  }
}

const cases = [];
for (const inputCase of input.cases) {
  cases.push(await runCase(inputCase));
}
process.stdout.write(JSON.stringify({ cases }));
