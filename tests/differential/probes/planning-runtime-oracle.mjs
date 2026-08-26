/**
 * Planning-runtime oracle probe (differential harness, Stage 3R R13.4).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes the scenario against the REAL TypeScript reference planning
 * model, policy, validation, flow, and the task-runtime plan lifecycle,
 * and prints the canonical observation object as JSON on stdout. This is
 * a thin scenario adapter: it does not reimplement runtime behavior.
 *
 * Deterministic: every timestamp comes from the injected clock; planner
 * outcomes are scripted; no ambient clock, randomness, or environment.
 */
import { readFileSync } from "node:fs";
import { createTaskContract } from "../../../packages/core/src/tasks/task-contract.js";
import { createTaskRuntime } from "../../../packages/core/src/tasks/task-runtime.js";
import { createTaskRuntimeSnapshot } from "../../../packages/core/src/tasks/task-snapshot.js";
import {
  PLANNING_LIMITS,
  createTaskPlan,
  reviseTaskPlan,
  summarizePlan,
  hasMeaningfulAcceptanceCriteria,
} from "../../../packages/core/src/planning/planning-model.js";
import { computeTaskPlanContentDigest } from "../../../packages/core/src/identity/contract-plan-identity.js";
import {
  containsGodotSceneOrResourceReference,
  containsProtectedConfigReference,
  createPlanningPolicy,
} from "../../../packages/core/src/planning/planning-policy.js";
import {
  extractPlanCandidateJson,
  isSafePlanPath,
  validatePlanCandidate,
} from "../../../packages/core/src/planning/planning-validation.js";
import { createPlanningFlow } from "../../../packages/core/src/planning/planning-flow.js";

const input = JSON.parse(readFileSync(0, "utf8"));
const NOW_MS = Number(input.nowMs ?? 1_700_000_000_000);

const REV_HANDLE = "rev_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";

function contractWith(criteria) {
  return createTaskContract({
    id: "task-1",
    request: "Implement the bounded feature",
    acceptanceCriteria: criteria,
  });
}

const CONTRACT = contractWith([
  { id: "ac1", description: "feature works", verificationKind: "deterministic" },
  { id: "ac2", description: "tests pass", verificationKind: "review" },
]);

function planContent(overrides = {}) {
  return {
    objective: "Implement the feature",
    scope: { inScope: ["src/a.ts"], outOfScope: ["docs"] },
    nonGoals: ["no public API change"],
    touchpoints: [
      {
        id: "tp1",
        path: "src/a.ts",
        confidence: "verified",
        revision: REV_HANDLE,
        evidence: "read:src/a.ts",
      },
      { id: "tp2", path: "src/b*.ts", confidence: "candidate" },
    ],
    constraints: [{ id: "con1", description: "stay within scope" }],
    risks: [{ id: "risk1", severity: "low", description: "minor regression risk" }],
    steps: [
      { id: "s1", title: "Edit a", expectedTouchpoints: ["tp1"], verification: ["ac1"] },
      { id: "s2", title: "Verify b", expectedTouchpoints: ["tp2"], verification: ["ac2"] },
    ],
    validation: { checks: ["check-only parse"], requirements: ["workspace mutation"] },
    rollback: { description: "revert commits" },
    rationale: "straightforward",
    ...overrides,
  };
}

function createPlan(overrides = {}) {
  return createTaskPlan({
    id: overrides.id ?? "plan-task-1",
    taskId: "task-1",
    taskContractRevision: 1,
    taskContractDigest: CONTRACT.digest.value,
    depth: "full",
    content: overrides.content ?? planContent(),
    createdAt: NOW_MS,
  });
}

function newRuntime(contract = CONTRACT) {
  const runtime = createTaskRuntime({ now: () => NOW_MS });
  const handle = runtime.createTask({
    contract,
    snapshot: createTaskRuntimeSnapshot({
      runtimeVersion: "task-runtime-1",
      provider: null,
      sandboxProfileId: null,
      capabilityPolicyRevision: null,
      workspaceIdentity: null,
      godotEngineFingerprint: null,
      workflow: null,
    }),
    steps: [],
  });
  return { runtime, handle };
}

/** Scripted read-only planner returning queued outcomes in order. */
function scriptedPlanner(queue) {
  return {
    async plan() {
      const next = queue.shift();
      if (next === undefined) {
        throw new Error("scripted planner exhausted");
      }
      return next;
    },
  };
}

function planStateJson(state) {
  return {
    planId: state.planId,
    planRevision: state.planRevision,
    planDigest: state.planDigest,
    depth: state.depth,
    state: state.state,
    approval: state.approval,
    staleReason: state.staleReason,
  };
}

function rejection(result) {
  return result.status === "ok" ? { ok: true } : { ok: false, reason: result.reason ?? null };
}

// ---------------------------------------------------------------------------
// Cases.
// ---------------------------------------------------------------------------

function caseModelIdentity() {
  const planA = createPlan();
  const contentB = planContent({ objective: "Implement the feature faster" });
  const planB = reviseTaskPlan(planA, { content: contentB });
  const singleCriterion = contractWith([
    { id: "ac1", description: "works", verificationKind: "deterministic" },
  ]);
  const userOnly = contractWith([
    { id: "ac1", description: "looks right", verificationKind: "user" },
    { id: "ac2", description: "feels right", verificationKind: "user" },
  ]);
  return {
    name: "plan-model-identity",
    revisionA: planA.revision,
    digestA: planA.digest.value,
    digestADeterministic:
      computeTaskPlanContentDigest(planA) === planA.digest.value &&
      computeTaskPlanContentDigest(planA) === computeTaskPlanContentDigest(planA),
    revisionB: planB.revision,
    digestB: planB.digest.value,
    idStable: planA.id === planB.id,
    previousUntouched: planA.revision === 1,
    summaryA: summarizePlan(planA),
    summaryB: summarizePlan(planB),
    meaningfulTwoMixed: hasMeaningfulAcceptanceCriteria(CONTRACT),
    meaningfulSingle: hasMeaningfulAcceptanceCriteria(singleCriterion),
    meaningfulUserOnly: hasMeaningfulAcceptanceCriteria(userOnly),
    maxPlanRevisions: PLANNING_LIMITS.maxPlanRevisions,
    frozenPlan: Object.isFrozen(planA),
    frozenSteps: Object.isFrozen(planA.steps),
    createdAtA: planA.createdAt,
    createdAtB: planB.createdAt,
  };
}

function caseValidationStrict() {
  const candidates = [
    ["not-an-object", "nope"],
    ["depth-mismatch", { ...planContent(), depth: "light" }],
    ["empty-objective", planContent({ objective: "   " })],
    ["oversized-objective", planContent({ objective: "x".repeat(2049) })],
    [
      "policy-claim-objective",
      planContent({ objective: "This plan will allow unrestricted shell execution." }),
    ],
    [
      "secret-objective",
      planContent({ objective: "Use password = hunter2hunter2 for the service." }),
    ],
    ["missing-steps", planContent({ steps: undefined })],
    ["empty-steps", planContent({ steps: [] })],
    ["invalid-step-id", planContent({ steps: [stepOf("1bad")] })],
    ["duplicate-step-id", planContent({ steps: [stepOf("s1"), stepOf("s1")] })],
    ["unknown-touchpoint-ref", planContent({ steps: [stepWith("s1", ["tpX"])] })],
    ["unknown-criterion-ref", planContent({ steps: [stepWith("s1", ["tp1"], ["acX"])] })],
    [
      "verified-without-revision",
      planContent({
        touchpoints: [{ id: "tp1", path: "src/a.ts", confidence: "verified" }],
      }),
    ],
    [
      "verified-bad-handle",
      planContent({
        touchpoints: [{ id: "tp1", path: "src/a.ts", confidence: "verified", revision: "rev_xyz" }],
      }),
    ],
    [
      "absolute-path",
      planContent({
        touchpoints: [{ id: "tp1", path: "/etc/passwd", confidence: "candidate" }],
      }),
    ],
    [
      "traversal-path",
      planContent({
        touchpoints: [{ id: "tp1", path: "a/../b", confidence: "candidate" }],
      }),
    ],
    [
      "glob-on-verified",
      planContent({
        touchpoints: [
          {
            id: "tp1",
            path: "src/*.ts",
            confidence: "verified",
            revision: REV_HANDLE,
          },
        ],
      }),
    ],
    [
      "reference-namespace-path",
      planContent({
        touchpoints: [{ id: "tp1", path: "@reference/x", confidence: "candidate" }],
      }),
    ],
    ["oversized-statement", planContent({ nonGoals: ["y".repeat(513)] })],
    ["missing-validation", planContent({ validation: undefined })],
    ["empty-checks", planContent({ validation: { checks: [] } })],
    [
      "policy-claim-requirement",
      planContent({
        validation: {
          checks: ["parse"],
          requirements: ["disable sandbox restrictions"],
        },
      }),
    ],
    ["rollback-not-object", planContent({ rollback: "revert" })],
    [
      "valid-candidate-drops-unknown-keys",
      { ...planContent(), unknownKey: "must not cross the boundary" },
    ],
  ];
  const results = [];
  for (const [tag, raw] of candidates) {
    const outcome = validatePlanCandidate(raw, { contract: CONTRACT, depth: "full" });
    results.push(
      outcome.ok
        ? { tag, ok: true, touchpointCount: outcome.content.touchpoints.length }
        : { tag, ok: false, reasons: outcome.reasons },
    );
  }
  const noneDepth = validatePlanCandidate(planContent(), { contract: CONTRACT, depth: "none" });
  const lightOverflow = validatePlanCandidate(planContent({ steps: lightOverflowSteps() }), {
    contract: CONTRACT,
    depth: "light",
  });
  const pathChecks = [
    ["backslash", isSafePlanPath("a\\b", { allowGlob: false })],
    ["drive", isSafePlanPath("C:/x", { allowGlob: false })],
    ["dot-segment", isSafePlanPath("./a", { allowGlob: false })],
    ["empty", isSafePlanPath("", { allowGlob: false })],
    ["doublestar-glob-ok", isSafePlanPath("src/**/*.ts", { allowGlob: true })],
    ["question-mark-outside-glob", isSafePlanPath("a?b", { allowGlob: false })],
  ];
  const extracted = extractPlanCandidateJson('```json\n{"objective":"x"}\n```');
  const extractedPlain = extractPlanCandidateJson('{"objective":"y"}');
  const extractedInvalid = extractPlanCandidateJson("[1,2]");
  return {
    name: "plan-validation-strict",
    results,
    noneDepthOk: noneDepth.ok === true ? null : (noneDepth.reasons[0] ?? null),
    lightOverflowReason: lightOverflow.ok === true ? null : (lightOverflow.reasons[0] ?? null),
    pathChecks,
    fencedExtractionObjective: extracted?.objective ?? null,
    plainExtractionObjective: extractedPlain?.objective ?? null,
    invalidExtractionIsNull: extractedInvalid === null,
  };
}

function lightOverflowSteps() {
  const steps = [];
  for (let index = 1; index <= 7; index += 1) {
    steps.push(stepOf(`s${index}`));
  }
  return steps;
}

function stepOf(id) {
  return { id, title: `Step ${id}`, expectedTouchpoints: [] };
}

function stepWith(id, touchpoints, verification) {
  return {
    id,
    title: `Step ${id}`,
    expectedTouchpoints: touchpoints,
    ...(verification === undefined ? {} : { verification }),
  };
}

function casePolicyDepth() {
  const policy = createPlanningPolicy();
  const base = {
    request: "implement the feature",
    explicitPlanRequest: false,
    inspectionOnly: false,
    expectedMutation: true,
    acceptanceCriterionCount: 2,
    protectedConfigInvolved: false,
    spansMultipleSubsystems: false,
    researchRequired: false,
    capabilityUncertainty: false,
    narrowRepair: false,
    knownTouchpoints: 3,
  };
  const inputs = [
    ["explicit-full", { ...base, explicitPlanRequest: true }],
    ["explicit-light", { ...base, explicitPlanRequest: true, requestedDepth: "light" }],
    ["inspection-only", { ...base, inspectionOnly: true }],
    ["no-mutation", { ...base, expectedMutation: false }],
    ["protected-config", { ...base, protectedConfigInvolved: true }],
    ["multi-subsystem", { ...base, spansMultipleSubsystems: true }],
    ["research-required", { ...base, researchRequired: true }],
    ["capability-uncertainty", { ...base, capabilityUncertainty: true }],
    [
      "scene-relationships-full",
      { ...base, involvesGodotSceneOrResource: true, knownTouchpoints: 3 },
    ],
    [
      "scene-simple-stays-light",
      { ...base, involvesGodotSceneOrResource: true, knownTouchpoints: 2 },
    ],
    ["mixed-surface", { ...base, surface: "mixed" }],
    ["narrow-repair", { ...base, narrowRepair: true, knownTouchpoints: 2 }],
    ["unknown-surface", { ...base, knownTouchpoints: 0 }],
    ["broad-criteria", { ...base, acceptanceCriterionCount: 4 }],
    ["bounded-non-trivial", base],
  ];
  const decisions = inputs.map(([tag, decisionInput]) => {
    const decision = policy.decide(decisionInput);
    return { tag, depth: decision.depth, reason: decision.reason };
  });
  const markers = {
    agentsMention: containsProtectedConfigReference("update AGENTS.md handling"),
    siralosDirMention: containsProtectedConfigReference("see .siralos/rules"),
    phraseMention: containsProtectedConfigReference("this touches behavioral config"),
    plainMention: containsProtectedConfigReference("update the readme"),
    sceneExtension: containsGodotSceneOrResourceReference("fix player.tscn layout"),
    resourceTree: containsGodotSceneOrResourceReference("the resource tree is large"),
    inheritedScene: containsGodotSceneOrResourceReference("an inherited scene broke"),
    plainSceneWord: containsGodotSceneOrResourceReference("the scene is nice"),
  };
  return { name: "planning-policy-depth", decisions, markers };
}

async function caseFlowPhases() {
  const observations = [];
  const { handle } = newRuntime();
  const queue = [];
  const flow = createPlanningFlow({
    handle,
    planner: scriptedPlanner(queue),
    now: () => NOW_MS,
  });
  const earlyRun = await flow.run();
  observations.push({
    op: "run-before-route",
    status: earlyRun.status,
    message: earlyRun.message ?? null,
  });
  const inspectionDecision = flow.route({ ...inspectionInput() });
  observations.push({
    op: "route-inspection",
    depth: inspectionDecision.depth,
    reason: inspectionDecision.reason,
  });
  const routedNoneRun = await flow.run();
  observations.push({ op: "run-at-none", status: routedNoneRun.status });
  const fullDecision = flow.route(fullInput());
  observations.push({ op: "route-full", depth: fullDecision.depth, reason: fullDecision.reason });
  queue.push({ status: "ready", content: planContent() });
  const plannedOne = await flow.run();
  observations.push({
    op: "run-planned-one",
    status: plannedOne.status,
    revision: plannedOne.plan?.revision ?? null,
    planId: plannedOne.plan?.id ?? null,
  });
  queue.push({ status: "ready", content: planContent({ objective: "Adjusted objective" }) });
  const plannedTwo = await flow.run();
  observations.push({
    op: "run-planned-two",
    status: plannedTwo.status,
    revision: plannedTwo.plan?.revision ?? null,
  });
  observations.push({ op: "approve", ...rejection(flow.approve()) });
  observations.push({ op: "mutation-blocked-clean", blocked: flow.mutationExecutionBlocked() });
  handle.invalidatePlan("manual invalidation");
  observations.push({ op: "mutation-blocked-stale", blocked: flow.mutationExecutionBlocked() });
  handle.cancel("no longer needed");
  queue.push({ status: "ready", content: planContent() });
  const terminalRun = await flow.run();
  observations.push({
    op: "run-terminal",
    status: terminalRun.status,
    message: terminalRun.message ?? null,
  });
  const activityTypes = handle.activityLog().map((event) => event.type);
  return { name: "planning-flow-phases", observations, activityTypes };
}

function inspectionInput() {
  return {
    request: "inspect the workspace",
    explicitPlanRequest: false,
    inspectionOnly: true,
    expectedMutation: false,
    acceptanceCriterionCount: 0,
    protectedConfigInvolved: false,
    spansMultipleSubsystems: false,
    researchRequired: false,
    capabilityUncertainty: false,
    narrowRepair: false,
    knownTouchpoints: 0,
  };
}

function fullInput() {
  return {
    request: "implement the feature",
    explicitPlanRequest: true,
    inspectionOnly: false,
    expectedMutation: true,
    acceptanceCriterionCount: 2,
    protectedConfigInvolved: false,
    spansMultipleSubsystems: false,
    researchRequired: false,
    capabilityUncertainty: false,
    narrowRepair: false,
    knownTouchpoints: 3,
  };
}

function caseSetLifecycle() {
  const observations = [];
  const { handle } = newRuntime();
  observations.push({
    op: "first-revision-two",
    ...rejection(handle.setPlan(revPlan("plan-task-1", 2))),
  });
  observations.push({ op: "set-rev-one", ...rejection(handle.setPlan(revPlan("plan-task-1", 1))) });
  observations.push({
    op: "skip-revision",
    ...rejection(handle.setPlan(revPlan("plan-task-1", 3))),
  });
  observations.push({
    op: "wrong-task",
    ...rejection(handle.setPlan(wrongTaskPlan())),
  });
  observations.push({
    op: "stale-contract-binding",
    ...rejection(handle.setPlan(staleBindingPlan())),
  });
  observations.push({ op: "set-rev-two", ...rejection(handle.setPlan(revPlan("plan-task-1", 2))) });
  observations.push({
    op: "replacement-must-start-at-one",
    ...rejection(handle.setPlan(revPlan("plan-task-1b", 2))),
  });
  observations.push({
    op: "replacement-set",
    ...rejection(handle.setPlan(createPlan({ id: "plan-task-1b" }))),
  });
  observations.push({
    op: "id-reuse-refused",
    ...rejection(handle.setPlan(revPlan("plan-task-1", 1))),
  });
  observations.push({ op: "current-plan-id", id: handle.currentPlan()?.id ?? null });
  handle.approvePlan("plan-task-1b", 1);
  observations.push({
    op: "approval-invalidated-by-new-revision",
    ...rejection(handle.setPlan(revisePlanTo(handle.currentPlan(), 2))),
  });
  observations.push({
    op: "approval-state-after-invalidation",
    approval: handle.snapshot().plan.approval,
  });
  handle.cancel("done");
  observations.push({
    op: "terminal-refusal",
    ...rejection(handle.setPlan(revPlan("plan-task-1c", 1))),
  });
  return { name: "plan-set-lifecycle", observations };
}

/** Build a minimal valid plan bound to revision 1 at an arbitrary revision number. */
function revPlan(id, revision) {
  if (revision === 1) {
    return createPlan({ id });
  }
  return revisePlanTo(createPlan({ id }), revision);
}

function revisePlanTo(previous, revision) {
  let plan = previous;
  while (plan.revision < revision) {
    plan = reviseTaskPlan(plan, { content: planContent() });
  }
  return plan;
}

function wrongTaskPlan() {
  const plan = createPlan();
  return { ...plan, taskId: "task-other" };
}

function staleBindingPlan() {
  const plan = createPlan();
  return { ...plan, taskContractRevision: 2 };
}

function caseStalenessContractAdvance() {
  const { handle } = newRuntime();
  handle.setPlan(createPlan());
  handle.approvePlan("plan-task-1", 1);
  const before = handle.snapshot().plan;
  handle.reviseContract({ id: "task-1", request: "Implement the bounded feature now" });
  const after = handle.snapshot().plan;
  const invalidationEvents = handle
    .activityLog()
    .filter((event) => event.type === "plan_invalidated")
    .map((event) => ({
      planId: event.planId,
      revision: event.revision,
      reason: event.reason,
    }));
  return {
    name: "plan-staleness-contract-advance",
    before: planStateJson(before),
    after: planStateJson(after),
    invalidationEvents,
    contractRevisionAfter: handle.contract().revision,
  };
}

function caseApprovalBinding() {
  const observations = [];
  const { handle } = newRuntime();
  const plan = createPlan();
  handle.setPlan(plan);
  observations.push({ op: "approve-wrong-id", ...rejection(handle.approvePlan("plan-other", 1)) });
  observations.push({
    op: "approve-wrong-revision",
    ...rejection(handle.approvePlan("plan-task-1", 2)),
  });
  observations.push({ op: "approve-current", ...rejection(handle.approvePlan("plan-task-1", 1)) });
  observations.push({
    op: "approval-record",
    approvedAtMatchesClock: NOW_MS,
    requirementsDescriptive: plan.validation.requirements ?? [],
  });
  handle.invalidatePlan("reset");
  observations.push({ op: "approve-stale", ...rejection(handle.approvePlan("plan-task-1", 1)) });

  const second = newRuntime();
  const tampered = {
    ...createPlan(),
    digest: {
      algorithm: "sha256",
      artifactType: "TaskPlan",
      schemaVersion: 1,
      value: "f".repeat(64),
    },
  };
  second.handle.setPlan(tampered);
  observations.push({
    op: "approve-tampered-digest",
    ...rejection(second.handle.approvePlan("plan-task-1", 1)),
  });

  const third = newRuntime();
  third.handle.setPlan(createPlan());
  third.handle.approvePlan("plan-task-1", 1);
  third.handle.reviseContract({ id: "task-1", request: "Implement the bounded feature again" });
  observations.push({
    op: "approve-after-contract-advance",
    ...rejection(third.handle.approvePlan("plan-task-1", 1)),
  });
  return { name: "plan-approval-binding", observations };
}

function caseRevisionCap() {
  const { handle } = newRuntime();
  const accepted = [];
  const rejected = [];
  handle.setPlan(createPlan());
  accepted.push(1);
  for (let revision = 2; revision <= PLANNING_LIMITS.maxPlanRevisions + 1; revision += 1) {
    const result = handle.setPlan(revPlan("plan-task-1", revision));
    if (result.status === "ok") {
      accepted.push(revision);
    } else {
      rejected.push({ revision, reason: result.reason });
    }
  }
  return {
    name: "plan-revision-cap",
    accepted,
    rejected,
    historyLength: handle.planRevisions().length,
  };
}

function caseImmutabilityDetach() {
  const content = planContent();
  const plan = createTaskPlan({
    id: "plan-task-1",
    taskId: "task-1",
    taskContractRevision: 1,
    taskContractDigest: CONTRACT.digest.value,
    depth: "full",
    content,
    createdAt: NOW_MS,
  });
  // Caller-owned input mutation after creation never reaches the plan.
  content.objective = "mutated by caller";
  content.steps.pop();
  content.scope.inScope.push("extra.ts");
  const { handle } = newRuntime();
  handle.setPlan(plan);
  const storedFirst = handle.planRevisions()[0];
  const detached = structuredClone(handle.currentPlan());
  detached.steps.pop();
  detached.touchpoints[0].path = "mutated.ts";
  const revisions = handle.planRevisions();
  revisions.pop();
  return {
    name: "plan-immutability-detach",
    storedObjective: storedFirst.objective,
    storedStepCount: storedFirst.steps.length,
    storedInScope: storedFirst.scope.inScope,
    currentObjective: handle.currentPlan().objective,
    currentStepCount: handle.currentPlan().steps.length,
    currentFirstTouchpoint: handle.currentPlan().touchpoints[0].path,
    detachedMutationIsolated:
      handle.currentPlan().steps.length === 2 &&
      handle.currentPlan().touchpoints[0].path === "src/a.ts",
    historyLengthAfterAccessorPop: handle.planRevisions().length,
    accessorReturnsFreshCopies: handle.currentPlan() !== handle.currentPlan(),
  };
}

function caseInvalidateReasons() {
  const { handle } = newRuntime();
  handle.setPlan(createPlan());
  handle.invalidatePlan("surface changed under us");
  const firstReason = handle.snapshot().plan.staleReason;
  handle.invalidatePlan("second explicit invalidation");
  const events = handle
    .activityLog()
    .filter((event) => event.type === "plan_invalidated")
    .map((event) => ({ sequence: event.sequence, reason: event.reason }));
  return {
    name: "plan-invalidate-reasons",
    firstReason,
    secondReason: handle.snapshot().plan.staleReason,
    events,
    approvalAfterInvalidation: handle.snapshot().plan.approval,
  };
}

// ---------------------------------------------------------------------------

const cases = [];
for (const inputCase of input.cases) {
  switch (inputCase.name) {
    case "plan-model-identity":
      cases.push(caseModelIdentity());
      break;
    case "plan-validation-strict":
      cases.push(caseValidationStrict());
      break;
    case "planning-policy-depth":
      cases.push(casePolicyDepth());
      break;
    case "planning-flow-phases":
      cases.push(await caseFlowPhases());
      break;
    case "plan-set-lifecycle":
      cases.push(caseSetLifecycle());
      break;
    case "plan-staleness-contract-advance":
      cases.push(caseStalenessContractAdvance());
      break;
    case "plan-approval-binding":
      cases.push(caseApprovalBinding());
      break;
    case "plan-revision-cap":
      cases.push(caseRevisionCap());
      break;
    case "plan-immutability-detach":
      cases.push(caseImmutabilityDetach());
      break;
    case "plan-invalidate-reasons":
      cases.push(caseInvalidateReasons());
      break;
    default:
      throw new Error(`unknown planning-runtime fixture case ${inputCase.name}`);
  }
}
process.stdout.write(JSON.stringify({ cases }));
