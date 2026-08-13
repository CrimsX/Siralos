import type { MilestoneManifest } from "./milestone-manifest.js";
import { createMilestoneManifest } from "./milestone-manifest.js";

/**
 * Stage 3R — R2 manifest: Differential Behavioral Harness (ADR 0033).
 *
 * The migration's audit remediation gate: a deterministic scenario
 * corpus runs against the Siralos TypeScript reference and the Siralos
 * Rust candidate; canonical outcome records are compared exactly; the
 * audit report records coverage and per-scenario status; required
 * deviations block the gate until remediated.
 *
 * Acceptance IDs are stable (S3R2.*) and decoupled from individual test
 * filenames so tests, reports, milestone evaluation, and future
 * evolution can reference them.
 */

export const S3R2_MILESTONE_MANIFEST: MilestoneManifest = createMilestoneManifest({
  id: "S3R2",
  title: "Differential Behavioral Harness",
  goal: "Establish the audit remediation gate for the Siralos Rust migration: a deterministic, offline harness that runs a versioned digest-bound scenario corpus against the TypeScript reference and the Rust candidate, machine-compares canonical outcome records, emits a per-commit migration audit, and blocks acceptance until required drift is remediated.",
  prerequisites: [
    {
      id: "prereq-r1-workspace",
      description: "Stage 3R R1 Rust workspace skeleton with siralos-core/adapters/cli.",
    },
    {
      id: "prereq-oracle",
      description: "Siralos TypeScript reference implementation (behavioral oracle).",
    },
  ],
  deliverables: [
    {
      id: "deliver-adr",
      description:
        "ADR 0033: harness contract, record schema, comparison semantics, remediation loop.",
    },
    {
      id: "deliver-corpus",
      description: "Versioned scenario corpus (inputs only) with per-scenario SHA-256 digests.",
    },
    {
      id: "deliver-oracle-runner",
      description: "TypeScript oracle runner emitting canonical deterministic outcome records.",
    },
    {
      id: "deliver-candidate-runner",
      description: "Rust candidate runner (siralos-harness) emitting identical canonical records.",
    },
    {
      id: "deliver-comparator",
      description: "Exact canonical comparator with audit report and typed exit codes (0/1/2).",
    },
    {
      id: "deliver-audit",
      description: "Per-run audit.json: coverage, per-scenario status, deviation inventory.",
    },
  ],
  invariants: [
    {
      id: "inv-canonical-records",
      description: "Both runners emit byte-identical canonical records for the same corpus.",
    },
    {
      id: "inv-deterministic",
      description:
        "Consecutive runs of either runner produce identical output; the audit contains no timestamps.",
    },
    {
      id: "inv-offline",
      description:
        "The harness is offline: no network, no live providers, no mutations outside its gitignored output directory.",
    },
    {
      id: "inv-oracle-authority",
      description:
        "The TypeScript reference is the oracle; the candidate must match it, never the reverse.",
    },
    {
      id: "inv-no-ports",
      description:
        "R2 ports no subsystem; the harness proves itself on the state-dir and version-identity subjects.",
    },
  ],
  acceptance: [
    {
      id: "S3R2.ADR.REGISTERED",
      description:
        "ADR 0033 exists, is registered in the runtime documentation index, and the docs-consistency check passes.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.CORPUS.SCHEMA",
      description:
        "The corpus manifest validates: every scenario file exists, parses, declares id/subject/platforms/parity, and its digest matches the recomputed canonical SHA-256.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.ORACLE.DETERMINISTIC",
      description:
        "Two consecutive oracle runs produce byte-identical canonical records, and the records are canonical sorted-key JSON.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.CANDIDATE.DETERMINISTIC",
      description:
        "The Rust candidate runner emits canonical records for the same corpus; its serialization is byte-identical to the oracle format (sorted keys, no whitespace).",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.PARITY.CURRENT_SURFACE",
      description:
        "The differential gate holds parity on the current subjects (state-dir resolution, product version identity) on the host platform.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.DEVIATION.DETECTED",
      description:
        "A seeded deviation (record mismatch or one-sided run) makes the gate red with a field-level deviation report; informational deviations are recorded but never fail.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.GATE.WIRED",
      description:
        "check:differential runs oracle, candidate, and comparator in sequence and is wired into npm run check and the GitHub Actions Rust workflow.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.AUDIT.EMITTED",
      description:
        "Every gate run emits audit.json with corpus version, platform, coverage counts, parity/skipped/informational lists, deviation inventory, and parityHeld.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.REMEDIATION.DRIFT",
      description:
        "The harness audit drove real remediation: state-dir resolution now mirrors the reference's USERPROFILE/os-profile semantics (empty USERPROFILE is a failure; HOMEDRIVE/HOMEPATH are not consulted), verified by parity on the state-dir scenarios.",
      evidenceKinds: ["validation_result"],
    },
    {
      id: "S3R2.REGRESSION",
      description:
        "Full repository gate green: TypeScript checks and tests, identity ratchet, Rust architecture check, differential gate, fmt, clippy -D warnings, and all Rust tests.",
      evidenceKinds: ["validation_result"],
    },
  ],
  architectureConcerns: ["rust", "process", "testing", "architecture"],
  requiredTests: [
    {
      id: "test-differential-harness",
      description: "tests/differential/harness.test.mjs (corpus, determinism, comparator).",
    },
  ],
  nonGoals: [
    "No TypeScript subsystem is ported to Rust in R2.",
    "No Godot domain work: no Godot package, no engine integration.",
  ],
});
