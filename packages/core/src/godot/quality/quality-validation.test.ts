import { describe, expect, it } from "vitest";
import {
  classifyValidationGate,
  discoverValidationPlan,
  type ValidationRunOutcome,
} from "./quality-validation.js";

describe("validation-plan discovery", () => {
  it("discovers the root check script when it exists", () => {
    const plan = discoverValidationPlan({ check: "npm run lint && npm test" }, ["a.gd"]);
    const commands = plan.optional;
    expect(commands).toHaveLength(1);
    expect(commands[0]?.id).toBe("npm-check");
    expect(commands[0]?.command?.scriptName).toBe("check");
    expect(commands[0]?.kind).toBe("npm-script");
  });

  it("discovers the root test script when check is absent", () => {
    const plan = discoverValidationPlan({ test: "vitest run" }, ["a.gd"]);
    expect(plan.optional.map((step) => step.id)).toEqual(["npm-test"]);
  });

  it("never invents script names", () => {
    const plan = discoverValidationPlan({ build: "tsc -b", start: "node index.js" }, ["a.gd"]);
    expect(plan.optional).toHaveLength(0);
  });

  it("never selects npm install, npm ci, npx, or npm exec", () => {
    const plan = discoverValidationPlan(
      { install: "npm ci", ci: "npm ci", npx: "npx tsc", exec: "npm exec tsc", check: "npm run lint" },
      ["a.gd"],
    );
    expect(plan.optional.map((step) => step.id)).toEqual(["npm-check"]);
  });

  it("skips the redundant test script when check aggregates it", () => {
    const plan = discoverValidationPlan(
      { check: "npm run lint && npm run test", test: "vitest run" },
      ["a.gd"],
    );
    expect(plan.optional.map((step) => step.id)).toEqual(["npm-check"]);
  });

  it("prefers check over lint and typecheck", () => {
    const plan = discoverValidationPlan({ lint: "eslint", check: "npm run lint", typecheck: "tsc" }, ["a.gd"]);
    expect(plan.optional.map((step) => step.id)).toEqual(["npm-check"]);
  });

  it("adds lint when no aggregate script exists", () => {
    const plan = discoverValidationPlan({ lint: "eslint ." }, ["a.gd"]);
    expect(plan.optional.map((step) => step.id)).toEqual(["npm-lint"]);
  });

  it("always includes the intrinsic GDScript gates for changed scripts", () => {
    const plan = discoverValidationPlan(null, ["scripts/player/player.gd"]);
    expect(plan.mandatory.map((step) => step.id)).toEqual(["gdscript-check", "lsp-diagnostics"]);
  });

  it("omits GDScript gates when no .gd file changed", () => {
    const plan = discoverValidationPlan(null, ["README.md"]);
    expect(plan.mandatory).toHaveLength(0);
  });
});

describe("validation-gate classification", () => {
  it("reports not_applicable when no project test runner exists (never an infrastructure failure)", () => {
    const plan = discoverValidationPlan(null, ["a.gd"]);
    const classification = classifyValidationGate(plan, [], true);
    expect(classification.status).toBe("not_applicable");
    expect(classification.evidenceKinds).toContain("no-project-test-runner");
  });

  it("passes when the project check command exits zero", () => {
    const plan = discoverValidationPlan({ check: "npm run lint" }, ["a.gd"]);
    const outcomes: readonly ValidationRunOutcome[] = [
      { step: plan.optional[0] as NonNullable<typeof plan.optional>[0], status: "passed", exitCode: 0, summary: "exit 0" },
    ];
    const classification = classifyValidationGate(plan, outcomes, true);
    expect(classification.status).toBe("passed");
  });

  it("blocks when a required test exits nonzero", () => {
    const plan = discoverValidationPlan({ test: "vitest run" }, ["a.gd"]);
    const outcomes: readonly ValidationRunOutcome[] = [
      { step: plan.optional[0] as NonNullable<typeof plan.optional>[0], status: "failed", exitCode: 1, summary: "exit 1" },
    ];
    const classification = classifyValidationGate(plan, outcomes, true);
    expect(classification.status).toBe("blocked");
  });

  it("reports not_run when a required command was denied", () => {
    const plan = discoverValidationPlan({ check: "npm run lint" }, ["a.gd"]);
    const outcomes: readonly ValidationRunOutcome[] = [
      { step: plan.optional[0] as NonNullable<typeof plan.optional>[0], status: "denied", exitCode: null, summary: "denied" },
    ];
    const classification = classifyValidationGate(plan, outcomes, true);
    expect(classification.status).toBe("not_run");
  });

  it("reports not_run when a required command is infrastructure-unavailable", () => {
    const plan = discoverValidationPlan({ check: "npm run lint" }, ["a.gd"]);
    const outcomes: readonly ValidationRunOutcome[] = [
      { step: plan.optional[0] as NonNullable<typeof plan.optional>[0], status: "unavailable", exitCode: null, summary: "runner unavailable" },
    ];
    const classification = classifyValidationGate(plan, outcomes, true);
    expect(classification.status).toBe("not_run");
  });

  it("reports not_run when a required command produced no outcome", () => {
    const plan = discoverValidationPlan({ check: "npm run lint" }, ["a.gd"]);
    const classification = classifyValidationGate(plan, [], true);
    expect(classification.status).toBe("not_run");
  });

  it("blocks when the intrinsic parser gate did not pass", () => {
    const plan = discoverValidationPlan(null, ["a.gd"]);
    const classification = classifyValidationGate(plan, [], false);
    expect(classification.status).toBe("blocked");
  });

  it("passes when the intrinsic gates pass and there are no commands", () => {
    const plan = discoverValidationPlan(null, []);
    const classification = classifyValidationGate(plan, [], true);
    expect(classification.status).toBe("not_applicable");
  });
});
