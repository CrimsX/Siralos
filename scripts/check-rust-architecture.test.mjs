/**
 * Guard tests for the Rust architecture checker's domain-neutrality
 * exemptions (Stage 3R R10c). The src/runtime exemption exists solely
 * so readiness/doctor sources can mirror the TypeScript oracle's
 * capability-input vocabulary verbatim; these tests pin that boundary:
 * exactly the exempt files may carry the vocabulary, and the vocabulary
 * itself must remain present (otherwise the exemption is dead and
 * should be removed).
 */
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { FORBIDDEN_CORE_SYMBOL_PATTERN, runChecks } from "./check-rust-architecture.mjs";

const ROOT = join(import.meta.dirname, "..");
const RUNTIME_DIR = join(ROOT, "crates", "siralos-core", "src", "runtime");

/** The only runtime sources allowed to carry the vocabulary. */
const EXEMPT_RUNTIME_FILES = ["readiness.rs", "doctor.rs"];

describe("rust architecture domain-neutrality exemption", () => {
  it("keeps the oracle vocabulary in the exempt runtime files", () => {
    for (const file of EXEMPT_RUNTIME_FILES) {
      const content = readFileSync(join(RUNTIME_DIR, file), "utf8");
      expect(
        FORBIDDEN_CORE_SYMBOL_PATTERN.test(content),
        `${file} no longer matches the forbidden pattern; narrow the exemption`,
      ).toBe(true);
    }
  });

  it("carries no forbidden vocabulary in any other runtime source", () => {
    const others = readdirSync(RUNTIME_DIR).filter(
      (entry) => entry.endsWith(".rs") && !EXEMPT_RUNTIME_FILES.includes(entry),
    );
    expect(others.length).toBeGreaterThan(0);
    for (const file of others) {
      const content = readFileSync(join(RUNTIME_DIR, file), "utf8");
      expect(
        FORBIDDEN_CORE_SYMBOL_PATTERN.test(content),
        `${file} uses exempted vocabulary outside the exemption`,
      ).toBe(false);
    }
  });

  it("keeps the whole workspace clean of violations", () => {
    expect(runChecks(ROOT)).toEqual([]);
  });
});
