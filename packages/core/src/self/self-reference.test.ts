import { describe, expect, it } from "vitest";
import { COMMAND_CATALOG, COMMAND_CATALOG_IDS } from "../commands/command-catalog.js";
import type { Capability, CapabilityPolicy, PermissionRule } from "../security/capability.js";
import { CAPABILITY_IDS } from "../security/capability.js";
import type { RegisteredToolInfo } from "../tools/tool-registry.js";
import {
  SELF_REFERENCE_NAME,
  computeSelfReferenceRevision,
  createSelfReference,
  toolAbiRevision,
  type SelfReferenceInput,
  type SelfReferenceSectionId,
} from "./self-reference.js";

/**
 * Locally-built policy for tests: doctor/self modules never construct
 * default policies (capability rules arrive through injected sources).
 */
function denyAllPolicy(): CapabilityPolicy {
  const rules = {} as Record<Capability, PermissionRule>;
  for (const capability of CAPABILITY_IDS) {
    rules[capability] = "deny";
  }
  return { rules };
}

const POLICY = denyAllPolicy();

const FAKE_TOOLS: readonly RegisteredToolInfo[] = [
  {
    definition: {
      name: "workspace.read",
      description: "Read a workspace file",
      inputSchema: { type: "object", properties: { path: { type: "string" } } },
    },
    capability: "workspace.read",
  },
  {
    definition: {
      name: "reference.read",
      description: "Read a reference file",
      inputSchema: { type: "object", properties: { alias: { type: "string" } } },
    },
    capability: "reference.inspect",
  },
];

function makeInput(overrides: Partial<SelfReferenceInput> = {}): SelfReferenceInput {
  return {
    runtime: { version: "0.0.0", nodeMajor: 24, platform: "win32" },
    registeredTools: FAKE_TOOLS,
    sandboxProfileId: "inspect",
    policy: POLICY,
    ...overrides,
  };
}

describe("self-reference", () => {
  it("identifies the installed runtime exactly", () => {
    const self = createSelfReference(makeInput());
    expect(self.name).toBe(SELF_REFERENCE_NAME);
    const runtime = self.readSection("runtime");
    expect(runtime).not.toBeNull();
    const byKey = Object.fromEntries(runtime!.lines.map((entry) => [entry.key, entry.value]));
    expect(byKey["version"]).toBe("0.0.0");
    expect(byKey["node-major"]).toBe("24");
    expect(byKey["platform"]).toBe("win32");
    expect(byKey["revision"]).toBe(self.revision);
  });

  it("command section derives from the actual command catalog (drift prevention)", () => {
    const self = createSelfReference(makeInput());
    const commands = self.readSection("commands");
    expect(commands).not.toBeNull();
    const ids = commands!.lines
      .map((entry) => entry.key.replace(/^\//, ""))
      .filter((key) => key !== "revision");
    // Every catalog id appears in the self-reference command section.
    for (const id of COMMAND_CATALOG_IDS) {
      expect(commands!.lines.some((entry) => entry.key === `/${id}`)).toBe(true);
    }
    // And every catalog entry is non-empty and documented.
    for (const entry of COMMAND_CATALOG) {
      expect(entry.description.length).toBeGreaterThan(0);
    }
    expect(ids.length).toBeGreaterThanOrEqual(COMMAND_CATALOG_IDS.length);
  });

  it("capability section lists every capability id with its policy rule", () => {
    const self = createSelfReference(makeInput());
    const capabilities = self.readSection("capabilities");
    expect(capabilities).not.toBeNull();
    for (const capability of CAPABILITY_IDS) {
      const entry = capabilities!.lines.find((line) => line.key === capability);
      expect(entry).toBeDefined();
      expect(entry!.value).toContain(POLICY.rules[capability]);
    }
  });

  it("sandbox section lists profile ids and the active profile", () => {
    const self = createSelfReference(makeInput({ sandboxProfileId: "develop-offline" }));
    const sandbox = self.readSection("sandbox");
    expect(sandbox).not.toBeNull();
    const active = sandbox!.lines.find((entry) => entry.key === "active-profile");
    expect(active!.value).toBe("develop-offline");
    expect(sandbox!.lines.some((entry) => entry.key === "profile:inspect")).toBe(true);
    expect(sandbox!.lines.some((entry) => entry.key === "profile:godot-lsp-local")).toBe(true);
  });

  it("workspace-tools section derives from the registered tool surface", () => {
    const self = createSelfReference(makeInput());
    const tools = self.readSection("workspace-tools");
    expect(tools).not.toBeNull();
    const read = tools!.lines.find((entry) => entry.key === "workspace.read");
    expect(read).toBeDefined();
    expect(read!.value).toContain("workspace.read");
    const reference = tools!.lines.find((entry) => entry.key === "reference.read");
    expect(reference!.value).toContain("reference.inspect");
  });

  it("revision is deterministic and changes when the runtime identity changes", () => {
    const a = createSelfReference(makeInput());
    const b = createSelfReference(makeInput());
    expect(a.revision).toBe(b.revision);
    const c = createSelfReference(
      makeInput({ runtime: { version: "0.0.1", nodeMajor: 24, platform: "win32" } }),
    );
    expect(c.revision).not.toBe(a.revision);
  });

  it("revision changes when the command catalog revision changes", () => {
    const base = createSelfReference(makeInput());
    const other = computeSelfReferenceRevision({
      version: "0.0.0",
      nodeMajor: 24,
      platform: "win32",
      commandCatalogRevision: "changed",
      configSchemaRevision: "same",
      capabilitySchemaRevision: "same",
      toolAbiRevision: toolAbiRevision(FAKE_TOOLS),
    });
    expect(other).not.toBe(base.revision);
  });

  it("revision changes when the tool ABI changes", () => {
    const a = createSelfReference(makeInput());
    const moreTools = [
      ...FAKE_TOOLS,
      {
        definition: {
          name: "self.read",
          description: "Read a self section",
          inputSchema: { type: "object" },
        },
        capability: "self.inspect",
      },
    ] as const;
    const b = createSelfReference(
      makeInput({ registeredTools: moreTools as unknown as readonly RegisteredToolInfo[] }),
    );
    expect(b.revision).not.toBe(a.revision);
  });

  it("readSection returns null for unknown sections", () => {
    const self = createSelfReference(makeInput());
    expect(self.readSection("nope" as SelfReferenceSectionId)).toBeNull();
  });

  it("search is bounded and case-insensitive", () => {
    const self = createSelfReference(makeInput());
    const matches = self.search("GDScript");
    expect(matches.length).toBeGreaterThan(0);
    for (const match of matches) {
      expect(match.lines.length).toBeLessThanOrEqual(20);
    }
    expect(matches.length).toBeLessThanOrEqual(8);
    expect(self.search("")).toEqual([]);
    expect(self.search("zzzz-not-a-token")).toEqual([]);
  });

  it("sections are bounded in line count and line length", () => {
    const self = createSelfReference(makeInput());
    for (const section of self.sections) {
      expect(section.lines.length).toBeLessThanOrEqual(220);
      for (const entry of section.lines) {
        expect(entry.value.length).toBeLessThanOrEqual(240);
      }
    }
  });

  it("contains no credential-shaped or path-shaped content", () => {
    const self = createSelfReference(makeInput());
    const all = JSON.stringify(self);
    expect(all).not.toMatch(/(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]{8,}/);
    expect(all).not.toMatch(/AKIA[0-9A-Z]{16}/);
    expect(all).not.toMatch(/[A-Za-z]:\\/);
  });
});
