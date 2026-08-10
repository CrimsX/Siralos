import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Authored summary of the trusted user-level configuration surface
 * (`schemas/user-config.schema.json`), for the built-in `@solaris`
 * self-reference (Stage 3 milestone 6).
 *
 * The summary is deliberately small: it documents what configuration
 * EXISTS (sections, keys, allowed values) so the model can answer "what
 * can I configure?" from the installed runtime. It never contains values.
 *
 * Drift protection: `config-schema-summary.test.ts` (adapters) parses the
 * actual schema file and asserts the summary's sections/keys/enums are
 * exactly the schema's properties/enums, so a schema change without a
 * summary update fails the test.
 */

export interface ConfigSchemaKey {
  readonly name: string;
  readonly description: string;
  /** Allowed literal values when the schema constrains them. */
  readonly allowed?: readonly string[];
  /** JSON-Schema primitive shape ("string" | "boolean" | "object" | "integer"). */
  readonly shape: string;
}

export interface ConfigSchemaSection {
  readonly name: string;
  readonly description: string;
  readonly keys: readonly ConfigSchemaKey[];
}

export const CONFIG_SCHEMA_SUMMARY: readonly ConfigSchemaSection[] = [
  {
    name: "sandbox",
    description: "Session sandbox profile and backend selection.",
    keys: [
      {
        name: "profile",
        description: "Session sandbox profile.",
        allowed: ["inspect", "develop-offline"],
        shape: "string",
      },
      {
        name: "backend",
        description: "Sandbox backend selection.",
        allowed: ["auto", "anthropic-runtime"],
        shape: "string",
      },
    ],
  },
  {
    name: "godot",
    description:
      "Trusted user-level Godot installation configuration. Project files cannot select or broaden executables.",
    keys: [
      {
        name: "activeInstallation",
        description:
          "Installation id used by default; must reference a configured or discovered installation.",
        shape: "string",
      },
      {
        name: "installations",
        description:
          "Map of installation id to { path (absolute), editionHint: standard|dotnet|unknown }.",
        shape: "object",
      },
      {
        name: "discoverOnPath",
        description: "Whether fixed-name PATH discovery is enabled (default true).",
        shape: "boolean",
      },
    ],
  },
  {
    name: "quality",
    description:
      "Trusted user-level development-quality configuration. An untrusted repository cannot alter these settings.",
    keys: [
      {
        name: "reviewProvider",
        description:
          "Provider profile used for the independent change reviewer; must reference an existing configured provider.",
        shape: "string",
      },
    ],
  },
  {
    name: "references",
    description:
      "Declared external read-only references, alias to declaration. Aliases match ^[a-z][a-z0-9._-]{1,63}$; at most 16 references. Unknown keys are rejected at every level so credential fields cannot hide.",
    keys: [
      {
        name: "<alias>",
        description:
          "One reference declaration: kind (local-directory|repository), path, repository, optional ref { kind: commit|tag|branch, ... }, optional description.",
        allowed: ["local-directory", "repository"],
        shape: "object",
      },
    ],
  },
] as const;

export interface ConfigSchemaRevision {
  readonly sections: readonly ConfigSchemaSection[];
  readonly revision: string;
}

/** Stable revision of the documented configuration surface. */
export const CONFIG_SCHEMA_REVISION: string = sha256Hex(canonicalizeJson(CONFIG_SCHEMA_SUMMARY));
