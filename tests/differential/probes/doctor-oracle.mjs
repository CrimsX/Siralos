import { readFileSync } from "node:fs";
import {
  DOCTOR_AREAS,
  countDoctorReport,
  doctorExitCodeFor,
  normalizeDoctorRequest,
  DoctorInvocationError,
} from "../../../packages/core/src/doctor/doctor-model.ts";
import {
  sanitizeSecretsOnly,
  toSafeReport,
} from "../../../packages/core/src/doctor/safe-report.ts";
import {
  computeSelfReferenceRevision,
  toolAbiRevision,
} from "../../../packages/core/src/self/self-reference.ts";
import {
  CONFIG_SCHEMA_SUMMARY,
  CONFIG_SCHEMA_REVISION,
} from "../../../packages/core/src/self/config-schema-summary.ts";
import { canonicalizeJson, sha256Hex } from "../../../packages/core/src/godot/digest.js";

const runtime = (platformOverride) => ({
  version: "0.0.0",
  nodeMajor: 24,
  platform: platformOverride ?? "differential-fake",
});

const check = (overrides) => ({
  id: "sample.check",
  area: "runtime",
  status: "pass",
  summary: "ok",
  ...overrides,
});

function runCase(inputCase) {
  switch (inputCase.name) {
    case "counts-and-exit-codes": {
      const failing = {
        schemaVersion: 1,
        generatedAtMs: 0,
        runtime: runtime(),
        requestedAreas: ["runtime"],
        checks: [
          check({ id: "a", status: "pass" }),
          check({ id: "b", status: "warn", summary: "w" }),
          check({ id: "c", status: "fail", summary: "f" }),
          check({ id: "d", status: "skip", summary: "s" }),
        ],
      };
      const clean = {
        schemaVersion: 1,
        generatedAtMs: 0,
        runtime: runtime(),
        requestedAreas: ["runtime"],
        checks: [check({ id: "a" }), check({ id: "b", status: "skip" })],
      };
      const withCounts = (report) => ({ ...report, counts: countDoctorReport(report) });
      return {
        failing: {
          counts: countDoctorReport(failing),
          exit: doctorExitCodeFor(withCounts(failing)),
        },
        clean: { counts: countDoctorReport(clean), exit: doctorExitCodeFor(withCounts(clean)) },
      };
    }
    case "area-normalization": {
      let unknownArea = null;
      try {
        normalizeDoctorRequest({ areas: ["runtime", "not-an-area"] });
      } catch (error) {
        unknownArea = error instanceof DoctorInvocationError ? error.code : String(error);
      }
      return {
        all: [...normalizeDoctorRequest({})],
        reordered: [...normalizeDoctorRequest({ areas: ["godot", "runtime"] })],
        emptyMeansAll: [...normalizeDoctorRequest({ areas: [] })].length === DOCTOR_AREAS.length,
        unknownArea,
      };
    }
    case "safe-report-redaction": {
      const report = {
        schemaVersion: 1,
        generatedAtMs: 0,
        runtime: runtime(),
        requestedAreas: ["workspace"],
        checks: [
          check({
            id: "paths",
            area: "workspace",
            status: "fail",
            summary:
              "cannot read C:\\Users\\someone\\repo\\file.txt under /home/someone/repo and \\\\server\\share\\x",
          }),
          check({
            id: "secrets",
            area: "workspace",
            status: "warn",
            summary:
              "found sk-abcdef123456 and AKIAIOSFODNN7EXAMPLE and Bearer abc.def.ghi_jkl-123",
          }),
          check({
            id: "clean-relative",
            area: "workspace",
            summary: "relative src/app.ts stays intact; /doctor stays intact",
          }),
        ],
      };
      const safe = toSafeReport(report);
      return {
        checks: safe.checks.map((entry) => ({ id: entry.id, summary: entry.summary })),
        detailsDropped: !("details" in safe.checks[0]) && !("remediation" in safe.checks[0]),
        errorCategories: safe.errorCategories,
        secretsOnlyRelativeKept: sanitizeSecretsOnly(
          "see src/app.ts for Bearer abcdefghijkl1234567890",
        ).includes("src/app.ts"),
      };
    }
    case "self-reference-revision": {
      const parts = {
        version: inputCase.runtime.version,
        nodeMajor: inputCase.runtime.nodeMajor,
        platform: inputCase.runtime.platform,
        commandCatalogRevision: "catalog".padEnd(64, "0").slice(0, 64),
        configSchemaRevision: "config".padEnd(64, "0").slice(0, 64),
        capabilitySchemaRevision: "caps".padEnd(64, "0").slice(0, 64),
        toolAbiRevision: "abi".padEnd(64, "0").slice(0, 64),
      };
      const tools = [
        {
          definition: {
            name: "workspace.list",
            description: "List entries",
            inputSchema: { type: "object" },
          },
          capability: "workspace.read",
        },
        {
          definition: {
            name: "workspace.read",
            description: "Read a file",
            inputSchema: { type: "object" },
          },
          capability: "workspace.read",
        },
      ];
      const changedVersion = computeSelfReferenceRevision({ ...parts, version: "9.9.9" });
      return {
        revision: computeSelfReferenceRevision(parts),
        stableRepeat: computeSelfReferenceRevision(parts) === computeSelfReferenceRevision(parts),
        sensitiveToVersion: changedVersion !== computeSelfReferenceRevision(parts),
        toolAbi: toolAbiRevision(tools),
        name: "@siralos",
      };
    }
    case "config-schema-stability": {
      const recomputed = sha256Hex(canonicalizeJson(CONFIG_SCHEMA_SUMMARY));
      return {
        sectionNames: CONFIG_SCHEMA_SUMMARY.map((section) => section.name),
        stable: recomputed === CONFIG_SCHEMA_REVISION,
      };
    }
    default:
      throw new Error(`unknown capability-doctor fixture case ${inputCase.name}`);
  }
}

const input = JSON.parse(readFileSync(0, "utf8"));
const results = input.cases.map((inputCase) => runCase({ ...inputCase, runtime: input.runtime }));
process.stdout.write(JSON.stringify({ cases: results }));
