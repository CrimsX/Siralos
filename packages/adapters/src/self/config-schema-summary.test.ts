import { describe, expect, it } from "vitest";
import { CONFIG_SCHEMA_SUMMARY } from "@solaris/core";

/**
 * Drift guard between the authored CONFIG_SCHEMA_SUMMARY (documented in
 * the @solaris self-reference) and the actual trusted configuration
 * schema file (schemas/user-config.schema.json). A schema change without
 * a summary update fails here.
 */

interface SchemaNode {
  properties?: Record<string, SchemaNode>;
  additionalProperties?: SchemaNode | boolean;
  enum?: unknown[];
  type?: unknown;
}

async function loadSchema(): Promise<SchemaNode> {
  const { readFile } = await import("node:fs/promises");
  const schemaPath = new URL("../../../../schemas/user-config.schema.json", import.meta.url);
  return JSON.parse(await readFile(schemaPath, "utf8")) as SchemaNode;
}

describe("config schema summary conformance", () => {
  it("documents exactly the schema's top-level sections", async () => {
    const schema = await loadSchema();
    const schemaSections = Object.keys(schema.properties ?? {}).sort();
    const summarySections = CONFIG_SCHEMA_SUMMARY.map((section) => section.name).sort();
    expect(summarySections).toEqual(schemaSections);
  });

  it("documents exactly the schema's keys per section", async () => {
    const schema = await loadSchema();
    for (const section of CONFIG_SCHEMA_SUMMARY) {
      const schemaSection = schema.properties?.[section.name];
      // The references section is an alias-keyed map (additionalProperties);
      // the summary documents it as a single "<alias>" key.
      const schemaKeys =
        schemaSection?.properties === undefined
          ? ["<alias>"]
          : Object.keys(schemaSection.properties).sort();
      const summaryKeys = section.keys.map((key) => key.name).sort();
      expect(summaryKeys, `section ${section.name}`).toEqual(schemaKeys);
    }
  });

  it("documents exactly the schema's allowed enums", async () => {
    const schema = await loadSchema();
    for (const section of CONFIG_SCHEMA_SUMMARY) {
      for (const key of section.keys) {
        const schemaSection = schema.properties?.[section.name];
        // The alias key's allowed values live under the declaration's
        // `kind` property (additionalProperties map).
        const schemaKey =
          schemaSection?.properties?.[key.name] ??
          (typeof schemaSection?.additionalProperties === "object"
            ? schemaSection.additionalProperties?.properties?.kind
            : undefined);
        const schemaEnum = schemaKey?.enum ?? [];
        const summaryAllowed = [...(key.allowed ?? [])].sort();
        if (schemaEnum.length === 0) {
          expect(
            key.allowed,
            `section ${section.name}.${key.name} should not declare allowed values`,
          ).toBeUndefined();
        } else {
          expect(summaryAllowed, `section ${section.name}.${key.name}`).toEqual(
            [...schemaEnum].sort(),
          );
        }
      }
    }
  });

  it("documents exactly the schema's key shapes (JSON type)", async () => {
    const schema = await loadSchema();
    for (const section of CONFIG_SCHEMA_SUMMARY) {
      for (const key of section.keys) {
        const schemaSection = schema.properties?.[section.name];
        // The alias key's shape is the declaration object
        // (additionalProperties map); the summary documents it as "object".
        const schemaKey =
          schemaSection?.properties?.[key.name] ??
          (typeof schemaSection?.additionalProperties === "object"
            ? schemaSection.additionalProperties
            : undefined);
        const schemaType =
          key.name === "<alias>"
            ? "object"
            : Array.isArray(schemaKey?.type)
              ? (schemaKey.type as unknown[]).join("|")
              : typeof schemaKey?.type === "string"
                ? schemaKey.type
                : "unknown";
        expect(key.shape, `section ${section.name}.${key.name}`).toBe(schemaType);
      }
    }
  });

  it("has a stable revision", async () => {
    const { CONFIG_SCHEMA_REVISION } = await import("@solaris/core");
    expect(CONFIG_SCHEMA_REVISION).toMatch(/^[0-9a-f]{64}$/);
  });
});
