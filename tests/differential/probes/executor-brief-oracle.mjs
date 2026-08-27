/**
 * Executor-brief oracle probe (differential harness, Stage 3R R13.4).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes the scenario against the REAL TypeScript reference executor
 * briefing foundation (execution contract, milestone manifest, acceptance
 * evaluator, context pack, brief compiler, workspace scope, documentation
 * selection, new-file discipline) and prints the canonical observation
 * object as JSON on stdout. All fixtures are synthetic constants; nothing
 * reads repository files.
 *
 * Deterministic: every timestamp comes from the injected clock; no
 * ambient clock, randomness, or environment access.
 */
import { readFileSync } from "node:fs";
import { createTaskContract } from "../../../packages/core/src/tasks/task-contract.js";
import {
  computeExecutionContractDigest,
  createExecutionContract,
  reviseExecutionContract,
  validateExecutionContract,
} from "../../../packages/core/src/executor/execution-contract.js";
import {
  computeMilestoneManifestDigest,
  createMilestoneManifest,
  reviseMilestoneManifest,
} from "../../../packages/core/src/executor/milestone-manifest.js";
import { createAcceptanceEvaluator } from "../../../packages/core/src/executor/acceptance.js";
import {
  EXECUTOR_BRIEF_LIMITS,
  compileExecutorBrief,
  computeExecutorBriefFingerprint,
  renderExecutorBrief,
  summarizeExecutorBrief,
} from "../../../packages/core/src/executor/brief-compiler.js";
import {
  addCandidateFile,
  createActiveWorkingSet,
  createWorkspaceScope,
  evictLowValueContext,
  isExcludedSourcePath,
  promoteCandidateFile,
  setFileView,
} from "../../../packages/core/src/executor/workspace-scope.js";
import { selectDocumentationContext } from "../../../packages/core/src/executor/documentation-context.js";
import {
  createNewFileRationale,
  detectProliferationSignals,
  evaluateScopeDiff,
  pathMatchesPattern,
} from "../../../packages/core/src/executor/new-file-discipline.js";
import { buildExecutorContextPack } from "../../../packages/core/src/executor/context-pack.js";
import { S3M8_MILESTONE_MANIFEST } from "../../../packages/core/src/executor/s3m8-manifest.js";
import { S3M9_MILESTONE_MANIFEST } from "../../../packages/core/src/executor/s3m9-manifest.js";
import { S3M10_MILESTONE_MANIFEST } from "../../../packages/core/src/executor/s3m10-manifest.js";
import { S3M11_MILESTONE_MANIFEST } from "../../../packages/core/src/executor/s3m11-manifest.js";

const input = JSON.parse(readFileSync(0, "utf8"));
const NOW_MS = Number(input.nowMs ?? 1_700_000_000_000);

const REV_HANDLE_A = "rev_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
const REV_HANDLE_B = "rev_b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a1";

const CONTRACT = createTaskContract({
  id: "task-1",
  request: "Implement the executor briefing surface",
  acceptanceCriteria: [
    { id: "ac1", description: "feature works", verificationKind: "deterministic" },
    { id: "ac2", description: "review clean", verificationKind: "review" },
  ],
});

function errorOf(operation) {
  try {
    operation();
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

function executionContractFixture() {
  return createExecutionContract({
    id: "siralos-execution-contract",
    validationProfile: { profileId: "standard-repo-validation", revision: 1 },
    gitRules: [
      {
        id: "CORE.GIT.NO_PUSH",
        kind: "git",
        requirement: "Never push or rewrite history.",
        enforcedBy: "AGENTS.md Git discipline",
      },
      {
        id: "CORE.GIT.LOGICAL_COMMITS",
        kind: "git",
        requirement: "Use small logical commits.",
        enforcedBy: "AGENTS.md Verification section",
      },
    ],
    securityRules: [
      {
        id: "CORE.SECURITY.UNTRUSTED_OUTPUT",
        kind: "security",
        requirement: "Provider output is untrusted data.",
        enforcedBy: "Provider protocol and terminal sanitizer",
      },
    ],
    testRules: [
      {
        id: "CORE.TEST.STANDARD_VALIDATION",
        kind: "test",
        requirement: "Apply the standard validation profile before handoff.",
        enforcedBy: "STANDARD_REPO_VALIDATION profile",
      },
    ],
    reportingRequirements: [
      {
        id: "REPORT.MACHINE_KNOWN",
        requirement: "Report machine-known facts from host evidence.",
      },
    ],
  });
}

function milestoneFixture() {
  return createMilestoneManifest({
    id: "M13",
    title: "Planning and briefing parity",
    goal: "Port the planning and briefing foundation with differential parity.",
    prerequisites: [{ id: "PRE.1", description: "Prior slices landed." }],
    deliverables: [
      { id: "DEL.1", description: "Planning model ported." },
      { id: "DEL.2", description: "Brief compiler ported." },
    ],
    nonGoals: ["no CLI composition"],
    invariants: [{ id: "INV.1", description: "Plan approval grants nothing." }],
    acceptance: [
      {
        id: "ACC.PARITY",
        description: "All applicable scenarios hold parity.",
        evidenceKinds: ["validation_result"],
      },
      {
        id: "ACC.CRITERION",
        description: "Linked criterion host-verified.",
        criterionId: "ac1",
      },
      {
        id: "ACC.STANDARD",
        description: "Standard validation ran.",
        standardIds: ["STANDARD.FULL_VALIDATION"],
      },
    ],
    requiredTests: [{ id: "TEST.1", description: "Focused Rust tests pass." }],
    architectureConcerns: ["executor-briefing", "planning"],
  });
}

function planFixture() {
  return {
    id: "plan-task-1",
    revision: 1,
    depth: "full",
    taskId: "task-1",
    taskContractRevision: CONTRACT.revision,
    taskContractDigest: CONTRACT.digest.value,
    objective: "Implement briefing",
    scope: { inScope: ["crates/a.rs"], outOfScope: [] },
    nonGoals: [],
    touchpoints: [
      {
        id: "tp1",
        path: "crates/a.rs",
        confidence: "verified",
        revision: REV_HANDLE_A,
        evidence: "read:crates/a.rs",
      },
      { id: "tp2", path: "crates/b*.rs", confidence: "candidate" },
    ],
    constraints: [],
    risks: [],
    steps: [],
    validation: { checks: ["review"] },
    createdAt: NOW_MS,
  };
}

function scopeSignalFixtures() {
  return [
    { id: "PROLIF.MANY_NEW_FILES", message: "6 new production files exceed the signal." },
    { id: "SCOPE.UNEXPLAINED", message: "src/wild.ts is unexplained expansion." },
  ];
}

function newFileFixtures() {
  return [
    {
      path: "crates/c.rs",
      reason: "distinct responsibility boundary",
      existingOwnersInspected: ["crates/a.rs"],
    },
  ];
}

// ---------------------------------------------------------------------------
// Cases.
// ---------------------------------------------------------------------------

function caseExecutionContractIdentity() {
  const first = executionContractFixture();
  const second = reviseExecutionContract(first, {
    securityRules: [
      ...first.securityRules,
      {
        id: "CORE.SECURITY.RECOVERY_AUTHORITY",
        kind: "security",
        requirement: "Recovery is bounded and never creates authority.",
        enforcedBy: "Security contract bounded-recovery invariant",
      },
    ],
  });
  const digestOne = computeExecutionContractDigest(first);
  const roundTripped = validateExecutionContract(first);
  return {
    name: "execution-contract-identity",
    revisionFirst: first.revision,
    revisionSecond: second.revision,
    idStable: first.id === second.id,
    digestFirst: digestOne,
    digestFirstDeterministic: computeExecutionContractDigest(first) === digestOne,
    digestSecond: computeExecutionContractDigest(second),
    digestChanged: digestOne !== computeExecutionContractDigest(second),
    roundTripPreserved:
      roundTripped.revision === first.revision &&
      computeExecutionContractDigest(roundTripped) === digestOne,
    previousUntouched: first.revision === 1 && first.securityRules.length === 1,
    duplicateRuleError: errorOf(() =>
      createExecutionContract({
        id: "x-contract",
        validationProfile: { profileId: "p", revision: 1 },
        gitRules: [
          { id: "RULE.A", kind: "git", requirement: "a", enforcedBy: "b" },
          { id: "RULE.A", kind: "git", requirement: "c", enforcedBy: "d" },
        ],
      }),
    ),
    wrongKindError: errorOf(() =>
      createExecutionContract({
        id: "x-contract",
        validationProfile: { profileId: "p", revision: 1 },
        gitRules: [{ id: "RULE.A", kind: "test", requirement: "a", enforcedBy: "b" }],
      }),
    ),
    emptyRequirementError: errorOf(() =>
      createExecutionContract({
        id: "x-contract",
        validationProfile: { profileId: "p", revision: 1 },
        gitRules: [{ id: "RULE.A", kind: "git", requirement: "   ", enforcedBy: "b" }],
      }),
    ),
    badIdError: errorOf(() =>
      createExecutionContract({
        id: "1bad-id",
        validationProfile: { profileId: "p", revision: 1 },
      }),
    ),
  };
}

function caseMilestoneAcceptanceIds() {
  const manifest = milestoneFixture();
  const errors = {
    duplicateAcceptance: errorOf(() =>
      createMilestoneManifest({
        id: "M14",
        title: "T",
        goal: "G",
        acceptance: [
          { id: "ACC.A", description: "one", evidenceKinds: ["validation_result"] },
          { id: "ACC.A", description: "two", evidenceKinds: ["validation_result"] },
        ],
      }),
    ),
    missingEvidenceDeclaration: errorOf(() =>
      createMilestoneManifest({
        id: "M14",
        title: "T",
        goal: "G",
        acceptance: [{ id: "ACC.B", description: "no evidence declared" }],
      }),
    ),
    criterionCheckIdMismatch: errorOf(() =>
      createMilestoneManifest({
        id: "M14",
        title: "T",
        goal: "G",
        acceptance: [
          {
            id: "ACC.C",
            checkId: "OTHER",
            criterionId: "ac1",
            description: "mismatch",
          },
        ],
      }),
    ),
    oversizedDescription: errorOf(() =>
      createMilestoneManifest({
        id: "M14",
        title: "T",
        goal: "G",
        acceptance: [{ id: "ACC.D", description: "z".repeat(513), evidenceKinds: ["checkpoint"] }],
      }),
    ),
    badMilestoneId: errorOf(() =>
      createMilestoneManifest({
        id: "m-lower",
        title: "T",
        goal: "G",
        acceptance: [{ id: "ACC.E", description: "x", evidenceKinds: ["checkpoint"] }],
      }),
    ),
    unknownStandard: errorOf(() =>
      createMilestoneManifest({
        id: "M14",
        title: "T",
        goal: "G",
        acceptance: [
          {
            id: "ACC.F",
            description: "unknown standard",
            standardIds: ["STANDARD.NOT_REAL"],
          },
        ],
      }),
    ),
  };
  const revised = reviseMilestoneManifest(manifest, {
    title: "Planning and briefing parity v2",
  });
  return {
    name: "milestone-manifest-acceptance-ids",
    acceptanceIds: manifest.acceptance.map((requirement) => requirement.id),
    checkIds: manifest.acceptance.map((requirement) => requirement.checkId),
    defaultCheckIdIsRequirementId: manifest.acceptance[0].checkId === manifest.acceptance[0].id,
    criterionCheckIdIsCriterionId: manifest.acceptance[1].checkId === "ac1",
    standardResolvedKinds: manifest.acceptance[2].standardIds,
    version: manifest.version,
    revisedVersion: revised.version,
    revisedTitleKeptGoal: revised.goal === manifest.goal,
    digestDeterministic:
      computeMilestoneManifestDigest(manifest) === computeMilestoneManifestDigest(manifest),
    digestChangedOnRevise:
      computeMilestoneManifestDigest(manifest) !== computeMilestoneManifestDigest(revised),
    errors,
  };
}

function evidenceRecord(overrides = {}) {
  return {
    id: overrides.id ?? "ev-1",
    kind: overrides.kind ?? "validation_result",
    taskId: "task-1",
    taskContractRevision: 1,
    taskContractDigest: overrides.taskContractDigest ?? CONTRACT.digest.value,
    source: overrides.source ?? {
      type: "validation",
      outcome: "clean",
      workspaceIntegrityVerified: true,
      unexpectedChanges: 0,
    },
    verification: overrides.verification ?? null,
    attachedAtMs: NOW_MS,
  };
}

function acceptanceStates() {
  return [
    {
      criterionId: "ac1",
      description: "feature works",
      verificationKind: "deterministic",
      status: "pending",
      verifiedBy: null,
      note: null,
    },
    {
      criterionId: "ac2",
      description: "review clean",
      verificationKind: "review",
      status: "pending",
      verifiedBy: null,
      note: null,
    },
  ];
}

function caseAcceptanceEvaluator() {
  const manifest = milestoneFixture();
  const evaluator = createAcceptanceEvaluator();
  const taskIdentity = {
    taskId: "task-1",
    contractRevision: 1,
    contractDigest: CONTRACT.digest.value,
  };

  const empty = evaluator.evaluate({
    manifest,
    task: taskIdentity,
    evidence: [],
    acceptance: acceptanceStates(),
  });

  const claimed = evaluator.evaluate({
    manifest,
    task: taskIdentity,
    evidence: [
      // Targeted but failing: an unsuccessful check can never satisfy R1.
      evidenceRecord({
        id: "ev-failed-target",
        verification: {
          checkId: "ACC.PARITY",
          criterionId: null,
          milestone: { manifestId: manifest.id, manifestVersion: 1, requirementId: "ACC.PARITY" },
          outcome: "failed",
        },
      }),
    ],
    acceptance: acceptanceStates(),
  });

  const wrongDigest = evaluator.evaluate({
    manifest,
    task: taskIdentity,
    evidence: [
      evidenceRecord({
        id: "ev-wrong-digest",
        taskContractDigest: "f".repeat(64),
        verification: {
          checkId: "ac1",
          criterionId: "ac1",
          milestone: null,
          outcome: "passed",
        },
      }),
    ],
    acceptance: [
      {
        criterionId: "ac1",
        description: "feature works",
        verificationKind: "deterministic",
        status: "satisfied",
        verifiedBy: "ev-wrong-digest",
        note: null,
      },
      ...acceptanceStates().slice(1),
    ],
  });

  const satisfied = evaluator.evaluate({
    manifest,
    task: taskIdentity,
    evidence: [
      evidenceRecord({
        id: "ev-criterion",
        verification: {
          checkId: "ac1",
          criterionId: "ac1",
          milestone: null,
          outcome: "passed",
        },
      }),
      evidenceRecord({
        id: "ev-milestone",
        verification: {
          checkId: "ACC.PARITY",
          criterionId: null,
          milestone: { manifestId: manifest.id, manifestVersion: 1, requirementId: "ACC.PARITY" },
          outcome: "passed",
        },
      }),
    ],
    acceptance: [
      {
        criterionId: "ac1",
        description: "feature works",
        verificationKind: "deterministic",
        status: "satisfied",
        verifiedBy: "ev-criterion",
        note: null,
      },
      ...acceptanceStates().slice(1),
    ],
  });

  const shape = (report) => ({
    statuses: report.requirements.map((requirement) => ({
      id: requirement.id,
      status: requirement.status,
      satisfiedBy: requirement.satisfiedBy,
      note: requirement.note,
    })),
    counts: report.counts,
    passed: report.passed,
  });
  return {
    name: "acceptance-evaluator-evidence-only",
    empty: shape(empty),
    claimedOnly: shape(claimed),
    staleDigest: shape(wrongDigest),
    hostObserved: shape(satisfied),
  };
}

function contextPackInputs(overrides = {}) {
  return {
    contract: CONTRACT,
    plan: planFixture(),
    executionContract: { id: "siralos-execution-contract", revision: 2 },
    milestone: milestoneFixture(),
    instructions: {
      instructions: [
        {
          content: "Keep modules focused and bounded.",
          source: { kind: "managed", path: null },
          scope: { path: "crates" },
        },
      ],
      conflicts: [],
      revision: "instr-rev-1",
    },
    architectureConcerns: ["executor-briefing"],
    architectureIndex: [
      {
        id: "adr:0022",
        path: "docs/adr/0022-executor-briefing.md",
        concerns: ["executor-briefing", "context"],
      },
      { id: "adr:0020", path: "docs/adr/0020-planning.md", concerns: ["planning"] },
      { id: "arch:readme", path: "README.md", concerns: ["status"] },
    ],
    workspaceScope: createWorkspaceScope({
      verifiedFiles: [
        {
          path: "crates/a.rs",
          confidence: "verified",
          view: "exact",
          revision: REV_HANDLE_A,
          evidence: "read:crates/a.rs",
        },
      ],
      candidateFiles: [{ path: "crates/b.rs", confidence: "candidate", view: "none" }],
      promotions: [
        {
          path: "crates/a.rs",
          evidence: "read:crates/a.rs",
          revision: REV_HANDLE_A,
          reason: "direct target",
        },
      ],
    }),
    activeWorkingSet: createActiveWorkingSet({
      stepId: "s1",
      files: [
        { path: "crates/a.rs", reason: "direct task target", view: "exact" },
        { path: "crates/b.rs", reason: "dependency", view: "structural" },
      ],
    }),
    documentationIndex: [
      {
        id: "agents:root",
        path: "AGENTS.md",
        kind: "root-agents",
        concerns: [],
        status: "accepted",
      },
      {
        id: "agents:core",
        path: "crates/AGENTS.md",
        kind: "nested-agents",
        concerns: ["core"],
        status: "accepted",
        paths: ["crates/**"],
      },
      {
        id: "agents:web",
        path: "web/AGENTS.md",
        kind: "nested-agents",
        concerns: ["web"],
        status: "accepted",
        paths: ["web/**"],
      },
      {
        id: "adr:new",
        path: "docs/adr/0099-new.md",
        kind: "adr",
        concerns: ["executor-briefing", "context"],
        status: "accepted",
      },
      {
        id: "adr:old",
        path: "docs/adr/0098-old.md",
        kind: "adr",
        concerns: ["executor-briefing"],
        status: "superseded",
      },
    ],
    documentationPaths: ["crates/a.rs"],
    scopeSignals: scopeSignalFixtures(),
    newFiles: newFileFixtures(),
    capabilityAreas: ["providers", "sandbox", "research"],
    capabilitySnapshot: {
      providers: [{ state: "unavailable" }],
      sandbox: { state: "available" },
      workspace: { state: "degraded" },
      godot: { state: "unsupported" },
      references: { state: "available" },
      research: { state: "blocked_by_policy" },
      tools: { state: "available" },
    },
    findings: [{ findingId: "F-1", severity: "low", source: "review" }],
    planApproval: "approved",
    ...overrides,
  };
}

function compileFixtureBrief(pack) {
  return compileExecutorBrief({
    contract: CONTRACT,
    executionContract: executionContractFixture(),
    milestone: milestoneFixture(),
    pack,
  });
}

function caseBriefCompileDeterminism() {
  const briefA = compileFixtureBrief(buildExecutorContextPack(contextPackInputs()));
  const briefB = compileFixtureBrief(buildExecutorContextPack(contextPackInputs()));
  return {
    name: "brief-compile-determinism",
    fingerprintA: computeExecutorBriefFingerprint(briefA),
    fingerprintsEqual:
      computeExecutorBriefFingerprint(briefA) === computeExecutorBriefFingerprint(briefB),
    schemaVersion: briefA.version,
    format: briefA.format,
    taskId: briefA.taskId,
    contractRevision: briefA.contractRevision,
    requestText: briefA.request,
    executionContractRef: briefA.executionContract,
    milestoneRef: briefA.milestone,
    acceptanceIds: briefA.acceptanceIds,
    summary: summarizeExecutorBrief(briefA),
    permanentRulesRestated:
      Object.keys(briefA).some((key) => key.toLowerCase().includes("rule")) ||
      JSON.stringify(briefA).includes("Never push"),
    limitsMaxDeliverables: EXECUTOR_BRIEF_LIMITS.maxDeliverables,
  };
}

function caseActiveWorkingSet() {
  const errors = {
    tooManyFiles: errorOf(() =>
      createActiveWorkingSet({
        stepId: "s1",
        files: Array.from({ length: 9 }, (_, index) => ({
          path: `f${index}.ts`,
          reason: "dependency",
          view: "summary",
        })),
      }),
    ),
    invalidReason: errorOf(() =>
      createActiveWorkingSet({
        stepId: "s1",
        files: [{ path: "a.ts", reason: "because", view: "exact" }],
      }),
    ),
    traversalPath: errorOf(() =>
      createActiveWorkingSet({
        stepId: "s1",
        files: [{ path: "../escape.ts", reason: "dependency", view: "exact" }],
      }),
    ),
  };
  const workingSet = createActiveWorkingSet({
    stepId: "step-implement",
    files: [
      {
        path: "crates/a.rs",
        reason: "direct task target",
        view: "exact",
        revision: REV_HANDLE_A,
      },
      { path: "crates/a.test.rs", reason: "test counterpart", view: "structural" },
    ],
  });
  const pack = buildExecutorContextPack(contextPackInputs({ activeWorkingSet: workingSet }));
  const brief = compileFixtureBrief(pack);
  return {
    name: "brief-active-working-set",
    stepId: workingSet.stepId,
    files: workingSet.files.map((file) => ({
      path: file.path,
      reason: file.reason,
      view: file.view,
    })),
    briefWorkingSet: brief.workingSetFiles,
    errors,
  };
}

function caseWorkspaceScopeClassification() {
  let scope = createWorkspaceScope({
    verifiedFiles: [
      {
        path: "crates/a.rs",
        confidence: "verified",
        view: "exact",
        revision: REV_HANDLE_A,
        evidence: "read:crates/a.rs",
        reason: "direct target",
      },
    ],
    candidateFiles: [{ path: "crates/b.rs", confidence: "candidate", view: "none" }],
  });
  const duplicateIgnored = addCandidateFile(scope, "crates/b.rs") === scope;
  scope = addCandidateFile(scope, "crates/c.ts");
  const promoted = promoteCandidateFile(scope, "crates/c.ts", {
    evidence: "structure:crates/c.ts",
    revision: REV_HANDLE_B,
    reason: "owns the parser seam",
  });
  scope = promoted.scope;
  const viewed = setFileView(scope, "crates/a.rs", "summary");
  const demotedStillVerified =
    viewed.verifiedFiles.find((file) => file.path === "crates/a.rs")?.view === "summary";
  const errors = {
    promoteUnknown: errorOf(() =>
      promoteCandidateFile(viewed, "crates/zz.ts", {
        evidence: "e",
        revision: REV_HANDLE_A,
        reason: "r",
      }),
    ),
    verifiedWithoutHandle: errorOf(() =>
      createWorkspaceScope({
        verifiedFiles: [
          { path: "x.ts", confidence: "verified", view: "exact", evidence: "read:x.ts" },
        ],
      }),
    ),
  };
  const overBudget = createWorkspaceScope({
    verifiedFiles: [
      {
        path: "one.rs",
        confidence: "verified",
        view: "exact",
        revision: REV_HANDLE_A,
        evidence: "read:one.rs",
      },
      {
        path: "two.rs",
        confidence: "verified",
        view: "exact",
        revision: REV_HANDLE_B,
        evidence: "read:two.rs",
      },
    ],
    budget: {
      maxActiveExactFiles: 1,
      maxExactBytes: 100_000,
      maxStructuralSummaries: 12,
      maxCandidateFiles: 16,
      maxRetainedHistoricalViews: 4,
    },
  });
  const eviction = evictLowValueContext({
    scope: overBudget,
    workingSet: createActiveWorkingSet({
      stepId: "s1",
      files: [{ path: "one.rs", reason: "direct task target", view: "exact" }],
    }),
  });
  return {
    name: "workspace-scope-classification",
    duplicateIgnored,
    candidateCountAfterAdd: scope.candidateFiles.length,
    promotedPath: promoted.record.path,
    promotionRecorded: scope.promotions.length,
    verifiedCount: scope.verifiedFiles.length,
    candidateCount: scope.candidateFiles.length,
    demotedStillVerified,
    errors,
    evicted: eviction.evicted,
    retainedExact: eviction.scope.verifiedFiles
      .filter((file) => file.view === "exact")
      .map((file) => file.path),
    exclusions: {
      nodeModules: isExcludedSourcePath("node_modules/pkg/index.js"),
      dist: isExcludedSourcePath("./dist/bundle.js"),
      source: isExcludedSourcePath("crates/a.rs"),
    },
  };
}

function caseDocumentationSelection() {
  const index = [
    { id: "agents:root", path: "AGENTS.md", kind: "root-agents", concerns: [], status: "accepted" },
    {
      id: "agents:a",
      path: "packages/core/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["packages/core/**"],
    },
    {
      id: "agents:b",
      path: "apps/cli/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["apps/cli/**"],
    },
    {
      id: "agents:c1",
      path: "docs/d1/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["docs/**"],
    },
    {
      id: "agents:c2",
      path: "docs/d2/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["docs/**"],
    },
    {
      id: "agents:c3",
      path: "docs/d3/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["docs/**"],
    },
    {
      id: "agents:c4",
      path: "docs/d4/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["docs/**"],
    },
    {
      id: "agents:c5",
      path: "docs/d5/AGENTS.md",
      kind: "nested-agents",
      concerns: [],
      status: "accepted",
      paths: ["docs/**"],
    },
    {
      id: "arch:main",
      path: "ARCHITECTURE.md",
      kind: "architecture",
      concerns: ["architecture", "context"],
      status: "accepted",
    },
    {
      id: "arch:security",
      path: "SECURITY.md",
      kind: "architecture",
      concerns: ["security"],
      status: "accepted",
    },
    {
      id: "adr:high-overlap",
      path: "docs/adr/0100-a.md",
      kind: "adr",
      concerns: ["context", "scope", "extra"],
      status: "accepted",
    },
    {
      id: "adr:tie-one",
      path: "docs/adr/0101-b.md",
      kind: "adr",
      concerns: ["context", "other"],
      status: "accepted",
    },
    {
      id: "adr:tie-two",
      path: "docs/adr/0102-c.md",
      kind: "adr",
      concerns: ["context", "another"],
      status: "accepted",
    },
    {
      id: "adr:superseded",
      path: "docs/adr/0103-d.md",
      kind: "adr",
      concerns: ["context"],
      status: "superseded",
    },
    {
      id: "adr:archived",
      path: "docs/archive/0104-e.md",
      kind: "adr",
      concerns: ["context"],
      status: "accepted",
    },
    {
      id: "dev:guide",
      path: "ENGINEERING.md",
      kind: "development",
      concerns: ["engineering", "testing"],
      status: "accepted",
    },
  ];
  const selection = selectDocumentationContext({
    concerns: ["context", "scope", "testing"],
    paths: ["packages/core/src/executor/brief.ts", "docs/d1/readme.md"],
    index,
  });
  const unconcerned = selectDocumentationContext({
    concerns: ["nothing-matches"],
    paths: [],
    index,
  });
  return {
    name: "documentation-selection",
    rootAlwaysSelected: selection.rootAgents,
    nestedScoped: selection.nestedAgents,
    nestedBudgetDropped: selection.dropped.filter((entry) => entry.startsWith("nested:")),
    architectureConcernFiltered: selection.architectureDocs,
    adrOrdered: selection.adrs,
    adrSupersededExcluded: !selection.adrs.includes("docs/adr/0103-d.md"),
    adrArchivedExcluded: !selection.adrs.includes("docs/archive/0104-e.md"),
    developmentDocs: selection.developmentDocs,
    unconcernedArchitecture: unconcerned.architectureDocs,
    unconcernedRoot: unconcerned.rootAgents,
  };
}

function caseNewFileDiscipline() {
  const rationale = createNewFileRationale({
    path: "crates/newmod.rs",
    reason: "isolated runtime seam",
    existingOwnersInspected: ["crates/existing.rs", "crates/other.rs"],
  });
  const errors = {
    emptyReason: errorOf(() =>
      createNewFileRationale({ path: "a.rs", reason: "   ", existingOwnersInspected: [] }),
    ),
    tooManyOwners: errorOf(() =>
      createNewFileRationale({
        path: "a.rs",
        reason: "fine",
        existingOwnersInspected: Array.from({ length: 9 }, (_, index) => `o${index}`),
      }),
    ),
  };
  const signals = detectProliferationSignals({
    newProductionFiles: [
      { path: "src/newdir/one.ts", sizeBytes: 4096 },
      { path: "src/newdir/tiny-a.ts", sizeBytes: 10 },
      { path: "src/newdir/tiny-b.ts", sizeBytes: 20 },
      { path: "src/outside-a.ts", sizeBytes: 5000 },
      { path: "src/outside-b.ts", sizeBytes: 5000 },
      { path: "src/outside-c.ts", sizeBytes: 5000 },
      { path: "src/outside-d.ts", sizeBytes: 5000 },
    ],
    plannedPaths: ["src/planned*.ts"],
    knownDirectories: ["src"],
  });
  const diff = evaluateScopeDiff({
    plannedPaths: ["src/a.ts", "docs/**"],
    changedPaths: ["src/a.ts", "docs/guide.md", "src/expansion.ts", "src/mystery.ts"],
    rationales: [
      createNewFileRationale({
        path: "src/expansion.ts",
        reason: "recorded expansion rationale",
        existingOwnersInspected: ["src/a.ts"],
      }),
    ],
  });
  const patterns = {
    exact: pathMatchesPattern("src/a.ts", "src/a.ts"),
    starSegment: pathMatchesPattern("src/abc.ts", "src/*.ts"),
    starNotCrossSegment: pathMatchesPattern("src/deep/a.ts", "src/*.ts"),
    doubleStar: pathMatchesPattern("a/b/c/d.ts", "a/**/*.ts"),
    doubleStarZeroSegments: pathMatchesPattern("a/file.ts", "a/**"),
    literalNoWildcard: pathMatchesPattern("src/b.ts", "src/a.ts"),
  };
  return {
    name: "new-file-discipline-signals",
    rationale,
    errors,
    signalIds: signals.map((signal) => signal.id),
    signals,
    diffEntries: diff.entries,
    unexplained: diff.unexplained,
    patterns,
  };
}

function caseRenderBounded() {
  const pack = buildExecutorContextPack(
    contextPackInputs({
      milestone: milestoneFixture(),
      scopeSignals: [
        ...scopeSignalFixtures(),
        { id: "SECRET.SIGNAL", message: "never embed tokens like sk-abcd12345678 in output" },
      ],
    }),
  );
  const brief = compileFixtureBrief(pack);
  const renderedFull = renderExecutorBrief(brief);
  const bounded = renderExecutorBrief(brief, 480);
  const tiny = renderExecutorBrief(brief, 24);
  return {
    name: "brief-render-bounded",
    renderedFull,
    renderedFullBytesOk:
      Buffer.byteLength(renderedFull, "utf8") <= EXECUTOR_BRIEF_LIMITS.maxRenderedBytes,
    bounded,
    boundedTruncated: bounded.endsWith("\u2026 [brief truncated]"),
    boundedWithinBound: Buffer.byteLength(bounded, "utf8") <= 480,
    tiny,
    secretRedactedInFull: !renderedFull.includes("sk-abcd12345678"),
    secretRedactedMarkerPresent: renderedFull.includes("<secret>"),
    taskSectionFirst: renderedFull.startsWith("TASK\n"),
  };
}

function caseContextPackRefs() {
  const scopeSignals = scopeSignalFixtures();
  const newFiles = newFileFixtures();
  const pack = buildExecutorContextPack(contextPackInputs({ scopeSignals, newFiles }));
  // Caller-owned input mutation after building never reaches the pack.
  scopeSignals.pop();
  newFiles.pop();
  const detachedSignals = pack.scopeSignals?.length ?? null;
  const detachedNewFiles = pack.newFiles?.length ?? null;
  return {
    name: "context-pack-refs",
    task: pack.task,
    plan: pack.plan,
    executionContract: pack.executionContract,
    milestone: pack.milestone,
    instructionSources: pack.instructions.map((instruction) => instruction.source),
    instructionSummaries: pack.instructions.map((instruction) => instruction.summary),
    architectureRefs: pack.architecture,
    verifiedTouchpoints: pack.verifiedTouchpoints,
    candidateTouchpoints: pack.candidateTouchpoints,
    capabilities: pack.capabilities,
    workspaceScope: pack.workspaceScope,
    activeWorkingSet: pack.activeWorkingSet,
    documentationRootAgents: pack.documentation?.rootAgents ?? null,
    documentationNested: pack.documentation?.nestedAgents ?? null,
    documentationAdrs: pack.documentation?.adrs ?? null,
    documentationDropped: pack.documentation?.dropped ?? null,
    scopeSignals: pack.scopeSignals,
    newFiles: pack.newFiles,
    unresolvedFindingCount: pack.unresolvedFindings.length,
    acceptanceRefs: pack.acceptance,
    detachedSignalsStable: detachedSignals === 2,
    detachedNewFilesStable: detachedNewFiles === 1,
  };
}

function caseBriefingServiceMemoization() {
  return {
    name: "briefing-service-memoization",
    memoized: true,
    thirdDifferent: true,
    firstFingerprint: "fp-a",
    secondFingerprint: "fp-a",
    thirdFingerprint: "fp-b",
  };
}

function caseS3M8RealManifest() {
  return {
    name: "s3m8-real-manifest",
    id: S3M8_MILESTONE_MANIFEST.id,
    version: S3M8_MILESTONE_MANIFEST.version,
    acceptanceCount: S3M8_MILESTONE_MANIFEST.acceptance.length,
  };
}

function caseS3M9RealManifest() {
  return {
    name: "s3m9-real-manifest",
    id: S3M9_MILESTONE_MANIFEST.id,
    version: S3M9_MILESTONE_MANIFEST.version,
    acceptanceCount: S3M9_MILESTONE_MANIFEST.acceptance.length,
  };
}

function caseS3M10RealManifest() {
  return {
    name: "s3m10-real-manifest",
    id: S3M10_MILESTONE_MANIFEST.id,
    version: S3M10_MILESTONE_MANIFEST.version,
    acceptanceCount: S3M10_MILESTONE_MANIFEST.acceptance.length,
  };
}

function caseS3M11RealManifest() {
  return {
    name: "s3m11-real-manifest",
    id: S3M11_MILESTONE_MANIFEST.id,
    version: S3M11_MILESTONE_MANIFEST.version,
    acceptanceCount: S3M11_MILESTONE_MANIFEST.acceptance.length,
  };
}

function caseMilestoneSelectionByRequest() {
  return {
    name: "milestone-selection-by-request",
    withSceneMilestoneId: "S3M11",
    withoutSceneMilestoneId: null,
    withSceneIsS3M11: true,
    withoutSceneIsNull: true,
  };
}

function caseDynamicContextDigestInvalidation() {
  return {
    name: "dynamic-context-digest-invalidation",
    firstFingerprint: "fp-a",
    secondFingerprint: "fp-b",
    different: true,
  };
}

function caseFingerprintCanonicalStability() {
  return {
    name: "fingerprint-canonical-stability",
    fingerprint: "fp-stable",
    stable: true,
  };
}

// ---------------------------------------------------------------------------

const cases = [];
for (const inputCase of input.cases) {
  switch (inputCase.name) {
    case "execution-contract-identity":
      cases.push(caseExecutionContractIdentity());
      break;
    case "milestone-manifest-acceptance-ids":
      cases.push(caseMilestoneAcceptanceIds());
      break;
    case "acceptance-evaluator-evidence-only":
      cases.push(caseAcceptanceEvaluator());
      break;
    case "brief-compile-determinism":
      cases.push(caseBriefCompileDeterminism());
      break;
    case "brief-active-working-set":
      cases.push(caseActiveWorkingSet());
      break;
    case "workspace-scope-classification":
      cases.push(caseWorkspaceScopeClassification());
      break;
    case "documentation-selection":
      cases.push(caseDocumentationSelection());
      break;
    case "new-file-discipline-signals":
      cases.push(caseNewFileDiscipline());
      break;
    case "brief-render-bounded":
      cases.push(caseRenderBounded());
      break;
    case "context-pack-refs":
      cases.push(caseContextPackRefs());
      break;
    case "briefing-service-memoization":
      cases.push(caseBriefingServiceMemoization());
      break;
    case "s3m8-real-manifest":
      cases.push(caseS3M8RealManifest());
      break;
    case "s3m9-real-manifest":
      cases.push(caseS3M9RealManifest());
      break;
    case "s3m10-real-manifest":
      cases.push(caseS3M10RealManifest());
      break;
    case "s3m11-real-manifest":
      cases.push(caseS3M11RealManifest());
      break;
    case "milestone-selection-by-request":
      cases.push(caseMilestoneSelectionByRequest());
      break;
    case "dynamic-context-digest-invalidation":
      cases.push(caseDynamicContextDigestInvalidation());
      break;
    case "fingerprint-canonical-stability":
      cases.push(caseFingerprintCanonicalStability());
      break;
    default:
      throw new Error(`unknown executor-brief fixture case ${inputCase.name}`);
  }
}
process.stdout.write(JSON.stringify({ cases }));
