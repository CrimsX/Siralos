import { describe, expect, it } from "vitest";
import {
  awaitCurrent,
  classifyPressure,
  createContextProjector,
  createDefaultPolicy,
  createEvidenceProjector,
  createRevisionGuard,
  createToolProjector,
  createWatermarkCache,
  estimateTokens,
  trimConversationPreservingPairs,
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
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Solaris." },
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
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Solaris." },
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
        { id: "s1", stability: "stable", title: "Instructions", content: "You are Solaris." },
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
