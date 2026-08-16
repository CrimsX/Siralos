import { describe, expect, it } from "vitest";
import {
  awaitCurrent,
  boundLineLength,
  classifyPressure,
  createContextProjector,
  createDefaultPolicy,
  createEvidenceProjector,
  createRevisionGuard,
  createToolProjector,
  createWatermarkCache,
  estimateTokens,
  trimConversationPreservingPairs,
  truncateText,
  DEVELOP_OFFLINE_PROFILE,
  type ConversationItem,
  type RegisteredToolInfo,
} from "../index.js";

describe("context estimation and pressure", () => {
  it("estimates tokens deterministically from UTF-8 bytes", () => {
    expect(estimateTokens("")).toBe(0);
    expect(estimateTokens("abcd")).toBe(1);
    expect(estimateTokens("abcdefgh")).toBe(2);
    expect(estimateTokens("é")).toBe(1); // 2 bytes -> ceil(2/4)
    expect(estimateTokens("a")).toBe(1);
  });

  it("classifies pressure against the working maximum", () => {
    expect(classifyPressure(1000, 10_000).state).toBe("normal");
    expect(classifyPressure(7000, 10_000).state).toBe("warn");
    expect(classifyPressure(8500, 10_000).state).toBe("auto");
    expect(classifyPressure(10_000, 10_000).state).toBe("hard");
    expect(classifyPressure(11_000, 10_000).state).toBe("hard");
  });
});

describe("context projector", () => {
  const projector = createContextProjector();

  it("sorts segments into stability classes with a stable fingerprint", () => {
    const projection = projector.project({
      segments: [
        {
          id: "v2",
          stability: "volatile",
          title: "Latest diagnostics",
          content: "error at line 4",
        },
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Siralos." },
        {
          id: "c1",
          stability: "contextual",
          title: "Task contract",
          content: "Add a health component",
        },
      ],
    });
    expect(projection.stableSegments.map((segment) => segment.id)).toEqual(["s1"]);
    expect(projection.contextualSegments.map((segment) => segment.id)).toEqual(["c1"]);
    expect(projection.volatileSegments.map((segment) => segment.id)).toEqual(["v2"]);
    expect(projection.stableFingerprint).toMatch(/^[0-9a-f]{64}$/);
  });

  it("volatile changes never change the stable fingerprint or stable bytes", () => {
    const base = projector.project({
      segments: [
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Siralos." },
        {
          id: "c1",
          stability: "contextual",
          title: "Task contract",
          content: "Add a health component",
        },
        {
          id: "v1",
          stability: "volatile",
          title: "Latest diagnostics",
          content: "error at line 4",
        },
      ],
    });
    const changed = projector.project({
      segments: [
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Siralos." },
        {
          id: "c1",
          stability: "contextual",
          title: "Task contract",
          content: "Add a health component",
        },
        { id: "v1", stability: "volatile", title: "Latest diagnostics", content: "all clean" },
      ],
    });
    expect(changed.stableFingerprint).toBe(base.stableFingerprint);
    expect(changed.stableSegments[0]?.content).toBe(base.stableSegments[0]?.content);
    expect(changed.stablePrefixBytes).toBe(base.stablePrefixBytes);
    expect(changed.volatileSegments[0]?.content).not.toBe(base.volatileSegments[0]?.content);
  });
});

describe("tool projector", () => {
  const policy = createDefaultPolicy("develop-offline");
  const profile = DEVELOP_OFFLINE_PROFILE;

  function tool(name: string, capability: RegisteredToolInfo["capability"]): RegisteredToolInfo {
    return {
      definition: { name, description: `${name} tool`, inputSchema: { type: "object" } },
      capability,
    };
  }

  const projector = createToolProjector({ policy, profile });

  it("classifies tools into available/gated/hidden", () => {
    const projection = projector.project({
      mode: "generic",
      registeredTools: [
        tool("workspace.read", "workspace.read"),
        tool("workspace.write", "workspace.write"),
        tool("network.outbound", "network.outbound"),
      ],
    });
    expect(projection.counts).toEqual({ available: 1, gated: 1, hidden: 1 });
    expect(projection.requestTools.map((t) => t.name)).toEqual([
      "workspace.read",
      "workspace.write",
    ]);
  });

  it("review mode hides mutation tools regardless of policy", () => {
    const projection = projector.project({
      mode: "review",
      registeredTools: [
        tool("workspace.read", "workspace.read"),
        tool("workspace.edit_file", "workspace.write"),
        tool("workspace.create_file", "workspace.write"),
      ],
    });
    expect(projection.counts).toEqual({ available: 1, gated: 0, hidden: 2 });
    expect(projection.requestTools.map((t) => t.name)).toEqual(["workspace.read"]);
  });

  it("produces a stable ABI fingerprint for identical projections", () => {
    const tools = [
      tool("workspace.read", "workspace.read"),
      tool("workspace.search", "workspace.read"),
    ];
    const first = projector.project({ mode: "generic", registeredTools: tools });
    const second = projector.project({ mode: "generic", registeredTools: tools });
    expect(first.fingerprint).toBe(second.fingerprint);
  });

  it("projects native prepare tools only for native/mixed development surfaces (S3M11)", () => {
    const tools = [
      tool("workspace.apply_text_changeset", "workspace.write"),
      tool("godot.prepare_scene_change", "godot.inspect"),
      tool("godot.prepare_resource_change", "godot.inspect"),
      tool("godot.inspect_scene", "godot.inspect"),
    ];
    // Script-only: native prepare tools are absent from the schema.
    const scriptOnly = projector.project({
      mode: "development",
      registeredTools: tools,
      surface: "script_only",
    });
    const scriptOnlyNames = scriptOnly.requestTools.map((t) => t.name);
    expect(scriptOnlyNames).not.toContain("godot.prepare_scene_change");
    expect(scriptOnlyNames).not.toContain("godot.prepare_resource_change");
    expect(scriptOnlyNames).toContain("workspace.apply_text_changeset");
    // Undefined surface fails closed the same way.
    const untyped = projector.project({ mode: "development", registeredTools: tools });
    expect(untyped.requestTools.map((t) => t.name)).not.toContain("godot.prepare_scene_change");
    // Mixed: native prepare tools are projected.
    const mixed = projector.project({
      mode: "development",
      registeredTools: tools,
      surface: "mixed",
    });
    const mixedNames = mixed.requestTools.map((t) => t.name);
    expect(mixedNames).toContain("godot.prepare_scene_change");
    expect(mixedNames).toContain("godot.prepare_resource_change");
    // Native-only projects them too.
    const nativeOnly = projector.project({
      mode: "development",
      registeredTools: tools,
      surface: "native_only",
    });
    expect(nativeOnly.requestTools.map((t) => t.name)).toContain("godot.prepare_scene_change");
    // Review mode never projects them regardless of surface.
    const review = projector.project({
      mode: "review",
      registeredTools: tools,
      surface: "mixed",
    });
    expect(review.requestTools.map((t) => t.name)).not.toContain("godot.prepare_scene_change");
  });
});

describe("evidence projector", () => {
  const encoder = new TextEncoder();
  const lineByteLengths = (text: string): number[] =>
    text.split("\n").map((line) => encoder.encode(line).length);

  it("redacts configured secrets from model views", () => {
    const projector = createEvidenceProjector({ secrets: ["sk-live-1234"] });
    const view = projector.projectForModel({
      rawText: "connecting with sk-live-1234 and more sk-live-1234",
    });
    expect(view.text).not.toContain("sk-live-1234");
    expect(view.text).toContain("[REDACTED]");
    expect(view.transformations).toContain("redact-secrets");
  });

  it("strips ANSI sequences and control characters", () => {
    const projector = createEvidenceProjector();
    const view = projector.projectForModel({ rawText: "\u001B[31mred\u001B[0m line\u0007" });
    expect(view.text).toContain("red line");
    expect(view.text).not.toContain("\u001B");
    expect(view.transformations).toContain("strip-ansi-control");
  });

  it("collapses repeated lines with a count marker", () => {
    const projector = createEvidenceProjector();
    const view = projector.projectForModel({
      rawText: "progress 1/10\nprogress 1/10\nprogress 1/10\n",
    });
    expect(view.text).toContain("\u00D73");
    expect(view.text.split("\n").filter((line) => line.includes("progress"))).toHaveLength(1);
  });

  it("truncates deterministically with explicit metadata", () => {
    const projector = createEvidenceProjector({ maxTotalBytes: 64 });
    const raw = "x".repeat(10_000);
    const view = projector.projectForModel({ rawText: raw });
    expect(view.truncated).toBe(true);
    expect(view.shownBytes).toBeLessThanOrEqual(64);
    expect(view.text).toContain("[truncated]");
    expect(view.originalBytes).toBe(10_000);
    expect(view.evidenceId).toBeNull();
  });

  it("enforces the ASCII line bound through projectForModel", () => {
    const raw = "a".repeat(2048);
    const view = createEvidenceProjector({
      maxLineBytes: 1024,
      maxTotalBytes: 32_768,
    }).projectForModel({
      rawText: raw,
    });

    expect(lineByteLengths(view.text)).toEqual([1024, 1024]);
    expect(view.transformations).toEqual(["bound-lines"]);
    expect(view.truncated).toBe(false);
    expect(view.shownBytes).toBe(2049);
    expect(view.originalBytes).toBe(2048);
  });

  it("leaves an exact line boundary unchanged", () => {
    const raw = "a".repeat(1024);
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(view.text).toBe(raw);
    expect(view.transformations).toEqual([]);
    expect(view.shownBytes).toBe(view.originalBytes);
  });

  it("splits a line at maxLineBytes plus one", () => {
    const raw = "a".repeat(1025);
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(lineByteLengths(view.text)).toEqual([1024, 1]);
    expect(view.transformations).toEqual(["bound-lines"]);
  });

  it("keeps a supplementary scalar intact at the integrated boundary", () => {
    const raw = `${"a".repeat(1021)}😀`;
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(view.text).toBe(`${"a".repeat(1021)}\n😀`);
    expect(lineByteLengths(view.text)).toEqual([1021, 4]);
    expect(view.text).not.toContain("\uFFFD");
    expect(view.transformations).toEqual(["bound-lines"]);
  });

  it("preserves the complete scalar for an impossible sub-scalar bound", () => {
    const view = createEvidenceProjector({ maxLineBytes: 3 }).projectForModel({ rawText: "😀😀" });

    expect(view.text).toBe("😀\n😀");
    expect(lineByteLengths(view.text)).toEqual([4, 4]);
    expect(view.transformations).toEqual(["bound-lines"]);
    expect(view.text).not.toContain("\uFFFD");
  });

  it("keeps redaction while applying the mandatory line bound", () => {
    const raw = `${"a".repeat(1020)} secret-token`;
    const view = createEvidenceProjector({
      secrets: ["secret-token"],
      maxLineBytes: 1024,
      maxTotalBytes: 10_000,
    }).projectForModel({ rawText: raw });

    expect(view.text).not.toContain("secret-token");
    expect(view.text).toContain("[REDACTED]");
    expect(lineByteLengths(view.text).every((bytes) => bytes <= 1024)).toBe(true);
    expect(view.transformations).toEqual(["redact-secrets", "bound-lines"]);
  });

  it("keeps stripped controls removed while applying the line bound", () => {
    const raw = `\u001B[31m${"a".repeat(2048)}\u0007`;
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(view.text).not.toContain("\u001B");
    expect(view.text).not.toContain("\u0007");
    expect(lineByteLengths(view.text)).toEqual([1024, 1024]);
    expect(view.transformations).toEqual(["strip-ansi-control", "bound-lines"]);
  });

  it("composes repeat collapse with mandatory line bounding", () => {
    const line = "a".repeat(1024);
    const raw = [line, line, line].join("\n");
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(view.text).toBe(`${line}\n ×3`);
    expect(lineByteLengths(view.text)).toEqual([1024, 4]);
    expect(view.transformations).toEqual(["collapse-repeated-lines", "bound-lines"]);
  });

  it("reverts optional collapse without restoring secrets or removing the line bound", () => {
    const view = createEvidenceProjector({
      secrets: ["s"],
      maxLineBytes: 20,
      maxTotalBytes: 1_000,
    }).projectForModel({ rawText: "s\ns\ns" });

    expect(view.text).not.toContain("s");
    expect(view.text).toContain("[REDACTED]");
    expect(lineByteLengths(view.text).every((bytes) => bytes <= 20)).toBe(true);
    expect(view.transformations).toEqual(["redact-secrets", "bound-lines"]);
    expect(view.transformations).not.toContain("collapse-repeated-lines");
  });

  it("rejects a worse collapse without disabling the line bound", () => {
    const raw = `${"a".repeat(2048)}\n\n\n`;
    const view = createEvidenceProjector({ maxLineBytes: 1024 }).projectForModel({ rawText: raw });

    expect(view.transformations).toEqual(["bound-lines"]);
    expect(view.transformations).not.toContain("collapse-repeated-lines");
    expect(lineByteLengths(view.text).every((bytes) => bytes <= 1024)).toBe(true);
  });

  it("truncates after line bounding and preserves scalar-safe metadata", () => {
    const raw = "b".repeat(2048);
    const view = createEvidenceProjector({
      maxLineBytes: 1024,
      maxTotalBytes: 1024,
    }).projectForModel({
      rawText: raw,
    });

    expect(view.truncated).toBe(true);
    expect(view.shownBytes).toBe(1024);
    expect(view.originalBytes).toBe(2048);
    expect(view.text).toContain("[truncated]");
    expect(lineByteLengths(view.text).every((bytes) => bytes <= 1024)).toBe(true);
    expect(view.transformations).toEqual(["bound-lines", "truncate"]);
  });

  it("preserves the terminal truncation marker when it exceeds the configured line bound", () => {
    const raw = "a".repeat(100);
    const markerLine = "… [truncated]";
    const markerLineBytes = encoder.encode(markerLine).length;
    const tinyBound = createEvidenceProjector({
      maxLineBytes: 3,
      maxTotalBytes: 1,
    }).projectForModel({ rawText: raw });
    const fittingBound = createEvidenceProjector({
      maxLineBytes: markerLineBytes,
      maxTotalBytes: 1,
    }).projectForModel({ rawText: raw });

    expect(tinyBound.truncated).toBe(true);
    expect(tinyBound.transformations).toEqual(["bound-lines", "truncate"]);
    expect(tinyBound.text).toBe(`\n${markerLine}`);
    expect(tinyBound.text).not.toContain("�");
    expect(lineByteLengths(tinyBound.text)).toEqual([0, markerLineBytes]);
    expect(lineByteLengths(tinyBound.text)[1]).toBeGreaterThan(3);
    expect(tinyBound.shownBytes).toBe(encoder.encode(tinyBound.text).length);
    expect(tinyBound.originalBytes).toBe(encoder.encode(raw).length);

    expect(fittingBound.truncated).toBe(true);
    expect(fittingBound.text).toBe(`\n${markerLine}`);
    expect(lineByteLengths(fittingBound.text)).toEqual([0, markerLineBytes]);
    expect(lineByteLengths(fittingBound.text)[1]).toBeLessThanOrEqual(markerLineBytes);
    expect(fittingBound.transformations).toEqual(["bound-lines", "truncate"]);
  });

  it("bounds lines at Unicode-scalar boundaries", () => {
    const raw = `${"a".repeat(1021)}😀`;
    const bounded = boundLineLength(raw, 1024);

    expect(bounded).toBe(`${"a".repeat(1021)}\n😀`);
    expect(bounded.split("\n").every((line) => new TextEncoder().encode(line).length <= 1024)).toBe(
      true,
    );
  });

  it("keeps supplementary characters intact when a scalar or marker cannot fit", () => {
    expect(boundLineLength("😀😀", 3)).toBe("😀\n😀");
    expect(truncateText("😀".repeat(6), 19)).toEqual({
      text: "\n… [truncated]",
      truncated: true,
    });
    expect(truncateText("😀".repeat(6), 20)).toEqual({
      text: "😀\n… [truncated]",
      truncated: true,
    });
  });

  it("never-worse rule: clean text is not inflated by the reduction path", () => {
    const projector = createEvidenceProjector({ maxTotalBytes: 1_000_000 });
    const raw = "a".repeat(500);
    const view = projector.projectForModel({ rawText: raw });
    expect(view.shownBytes).toBe(500);
    expect(view.truncated).toBe(false);
    expect(view.text).toBe(raw);
  });
});

describe("watermark cache", () => {
  it("evicts down to the low watermark only when the high watermark is reached", () => {
    const cache = createWatermarkCache<string>({ highWatermark: 4, lowWatermark: 2 });
    for (const key of ["a", "b", "c"]) {
      cache.set(key, key);
    }
    expect(cache.size).toBe(3); // below high: no cleanup
    cache.set("d", "d");
    expect(cache.size).toBe(4); // at high: still no cleanup
    cache.set("e", "e"); // exceeds high: cleanup to low
    expect(cache.size).toBe(2);
    expect(cache.get("a")).toBeUndefined();
    expect(cache.get("e")).toBe("e");
  });
});

describe("stale-result guard", () => {
  it("discards results bound to an advanced revision", () => {
    const guard = createRevisionGuard(1);
    const bound = guard.bind({ projected: true });
    expect(guard.isCurrent(bound)).toBe(true);
    guard.advance();
    expect(guard.isCurrent(bound)).toBe(false);
  });

  it("awaitCurrent resolves null for stale async results", async () => {
    const guard = createRevisionGuard(1);
    let resolve!: (value: string) => void;
    const pending = new Promise<string>((r) => {
      resolve = r;
    });
    const awaited = awaitCurrent(guard, pending);
    guard.advance(); // state advanced while the helper was in flight
    resolve("late result");
    expect(await awaited).toBeNull();
  });
});

describe("conversation pair-preserving trim", () => {
  function pair(callId: string, payload: string): ConversationItem[] {
    return [
      { type: "assistant_tool_call", callId, toolName: "workspace.read", input: { path: payload } },
      {
        type: "tool_result",
        callId,
        toolName: "workspace.read",
        result: { status: "success", output: { path: payload }, summary: payload },
      },
    ];
  }

  it("drops whole tool-call/result pairs, never one half", () => {
    const items: ConversationItem[] = [
      { type: "user_message", content: "request" },
      ...pair("call-1", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
      ...pair("call-2", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
    ];
    const result = trimConversationPreservingPairs(items, 60);
    expect(result.droppedItems).toBe(2); // one whole pair dropped
    const callIds = result.items.filter((item) => item.type === "assistant_tool_call");
    const resultIds = result.items.filter((item) => item.type === "tool_result");
    expect(callIds.length).toBe(resultIds.length);
    // The active request survives.
    expect(result.items[0]?.type).toBe("user_message");
  });

  it("returns the original list unchanged when it fits", () => {
    const items: ConversationItem[] = [{ type: "user_message", content: "hello" }];
    const result = trimConversationPreservingPairs(items, 1000);
    expect(result.droppedItems).toBe(0);
    expect(result.items).toEqual(items);
  });

  it("fails closed on a structurally invalid transcript (orphaned trailing call)", () => {
    const items: ConversationItem[] = [
      { type: "user_message", content: "request" },
      { type: "assistant_tool_call", callId: "orphan-1", toolName: "workspace.read", input: {} },
    ];
    const result = trimConversationPreservingPairs(items, 1);
    expect(result.droppedItems).toBe(0);
    expect(result.items).toEqual(items);
  });
});
