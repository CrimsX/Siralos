import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createToolProjector, parseReferenceDeclarationsSection } from "@siralos/core";
import { createReferenceServices } from "@siralos/adapters";
import { createCliApplication } from "./create-application.js";
import { createReferenceEvidenceRing, observeReferenceTools } from "./reference-research.js";
import { formatReferences } from "../output.js";

const tempDirectories: string[] = [];

async function makeTempDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-cli-test-"));
  tempDirectories.push(directory);
  return directory;
}

async function withConfigFile(content: unknown): Promise<string> {
  const directory = await makeTempDirectory();
  const path = join(directory, "config.json");
  await writeFile(path, JSON.stringify(content));
  return path;
}

async function makeReferenceDirectory(name = "engine-docs"): Promise<string> {
  const directory = await makeTempDirectory();
  const reference = join(directory, name);
  await mkdir(reference);
  await writeFile(join(reference, "README.md"), "# Engine docs\n");
  return reference;
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

function toolNames(tools: readonly { definition: { readonly name: string } }[]): string[] {
  return tools.map((tool) => tool.definition.name);
}

describe("createCliApplication reference and research wiring", () => {
  it("builds reference services, registers reference tools, and keeps research tools registered but hidden under the default policy", async () => {
    const reference = await makeReferenceDirectory();
    const configPath = await withConfigFile({
      sandbox: { profile: "inspect" },
      references: {
        "engine-docs": {
          kind: "local-directory",
          path: reference,
          description: "Engine documentation",
        },
      },
    });
    const app = await createCliApplication({ configPath });
    try {
      expect(app.referenceConfigError).toBeNull();
      const list = app.references.list();
      expect(list).toHaveLength(1);
      const entry = list[0];
      expect(entry).toBeDefined();
      if (entry === undefined) {
        return;
      }
      expect(entry.alias).toBe("engine-docs");
      expect(entry.kind).toBe("local-directory");
      expect(entry.trust).toBe("explicit-user");
      expect(entry.status).toBe("ready");
      expect(app.references.revision(entry.id)).not.toBeNull();
      expect(app.referenceMaterializer.status(entry.id)).toBe("not-required");

      const names = toolNames(app.tools);
      expect(names).toContain("reference.list");
      expect(names).toContain("reference.read");
      expect(names).toContain("reference.search");
      expect(names).toContain("research.repository");
      expect(names).toContain("research.godot_docs");

      // Research tools are registered but hidden under the default deny
      // policy for research.fetch; reference inspection is available.
      const projection = createToolProjector({
        policy: app.security.policy,
        profile: app.security.profile,
      }).project({ mode: "generic", registeredTools: app.tools });
      const byName = new Map(projection.tools.map((tool) => [tool.name, tool.visibility] as const));
      expect(byName.get("research.repository")).toBe("hidden");
      expect(byName.get("research.godot_docs")).toBe("hidden");
      expect(byName.get("reference.list")).toBe("available");
      expect(byName.get("reference.read")).toBe("available");

      expect([...app.research.sourceKinds()].sort()).toEqual(["godot-docs", "repository"]);
      expect(app.researchSources).toHaveLength(2);
    } finally {
      app.close();
    }
  });

  it("registers NO reference tools when no references are declared, and still registers research tools", async () => {
    const configPath = await withConfigFile({ sandbox: { profile: "inspect" } });
    const app = await createCliApplication({ configPath });
    try {
      expect(app.references.list()).toHaveLength(0);
      expect(app.referenceConfigError).toBeNull();
      const names = toolNames(app.tools);
      expect(names).not.toContain("reference.list");
      expect(names).not.toContain("reference.read");
      expect(names).not.toContain("reference.search");
      expect(names).toContain("research.repository");
      expect(names).toContain("research.godot_docs");
    } finally {
      app.close();
    }
  });

  it("surfaces a references config semantic error without crashing (fail closed, empty registry)", async () => {
    // The config layer accepts the non-empty string; core rejects the
    // relative path semantically.
    const configPath = await withConfigFile({
      references: {
        bad: { kind: "local-directory", path: "relative/path" },
      },
    });
    const app = await createCliApplication({ configPath });
    try {
      expect(app.referenceConfigError).toContain("not absolute");
      expect(app.references.list()).toHaveLength(0);
      expect(toolNames(app.tools)).not.toContain("reference.list");
      expect(
        formatReferences(app.references, app.referenceMaterializer, app.referenceConfigError),
      ).toContain("References configuration error");
    } finally {
      app.close();
    }
  });
});

describe("reference evidence ring and tool observation seam", () => {
  it("records bounded observations from successful reference tool executions", async () => {
    const reference = await makeReferenceDirectory();
    const parsed = parseReferenceDeclarationsSection({
      "engine-docs": {
        alias: "engine-docs",
        kind: "local-directory",
        source: { kind: "local-directory", path: reference },
      },
    });
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    const services = await createReferenceServices({
      declarations: parsed.declarations,
      workspaceRoot: process.cwd(),
      trustFor: () => "explicit-user",
    });
    const ring = createReferenceEvidenceRing();
    const wrapped = observeReferenceTools(services.tools, services.registry, ring);
    try {
      const result = await wrapped[0]?.execute({ reference: "engine-docs" }, {});
      expect(result?.status).toBe("success");
      const views = ring.list();
      expect(views).toHaveLength(1);
      const [view] = views;
      expect(view?.alias).toBe("engine-docs");
      expect(view?.operation).toBe("list");
      expect(view?.path).toBe(".");
      expect(view?.revision.identity.kind).toBe("local-directory");
    } finally {
      services.close();
    }
  });

  it("keeps at most 4 observations in the ring", () => {
    const ring = createReferenceEvidenceRing();
    for (let index = 0; index < 6; index += 1) {
      ring.record({
        referenceId: `ref_${index}` as never,
        alias: `ref-${index}` as never,
        revision: {
          identity: { kind: "local-directory", canonicalPath: "/tmp/x", fingerprint: "f" },
          resolvedAtMs: index,
        },
        path: ".",
        operation: "list",
        mode: null,
        sha256: null,
        evidenceId: null,
      });
    }
    expect(ring.list()).toHaveLength(4);
  });
});
