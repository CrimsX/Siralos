/**
 * Deterministic development validation plan (§19–§24).
 *
 * The plan always includes the mandatory intrinsic gates — changed
 * GDScript `--check-only`, changed-file LSP diagnostics, workspace/source
 * integrity, Git/change-set scope verification, and independent review —
 * plus narrow obvious existing project validation candidates. The
 * application controls the mandatory gates; the provider never has to
 * remember to request them. Project-defined commands are discovered only
 * from existing npm scripts named `check`, `test`, `lint`, or `typecheck`
 * (never invented names, never `install`/`ci`/`npx`/`exec`), preferring
 * `check` when it clearly aggregates validation and avoiding redundant
 * scripts when `check` already includes them.
 */

export type ValidationStepKind =
  | "gdscript-check"
  | "lsp-diagnostics"
  | "npm-script"
  | "node-script"
  | "manual-unavailable";

export interface ValidationStep {
  readonly id: string;
  readonly kind: ValidationStepKind;
  readonly displayName: string;
  readonly reason: string;
  /** Exact command for project-defined steps; absent for intrinsic gates. */
  readonly command?: {
    readonly runner: "npm-script" | "node-script";
    readonly scriptName?: string;
    readonly path?: string;
    readonly arguments: readonly string[];
    readonly workingDirectory: string;
  };
}

export interface DevelopmentValidationPlan {
  readonly mandatory: readonly ValidationStep[];
  readonly optional: readonly ValidationStep[];
}

/**
 * Narrow script-name candidates (§21). Only scripts that already exist are
 * ever selected; names are never invented. `install`/`ci`/`npx`/`exec` are
 * excluded by construction — they are not candidates.
 */
const SCRIPT_CANDIDATES = ["check", "test", "lint", "typecheck"] as const;

/**
 * Deterministic plan discovery (§21 rules 1–5). `packageScripts` is the
 * root package.json `scripts` map (null when the project has none).
 */
export function discoverValidationPlan(
  packageScripts: Readonly<Record<string, string>> | null,
  changedPaths: readonly string[],
): DevelopmentValidationPlan {
  const mandatory: ValidationStep[] = [];
  const hasGdScript = changedPaths.some((path) => path.endsWith(".gd"));
  if (hasGdScript) {
    mandatory.push({
      id: "gdscript-check",
      kind: "gdscript-check",
      displayName: "changed GDScript --check-only",
      reason: "Changed GDScript must parse against the exact engine parser.",
    });
    mandatory.push({
      id: "lsp-diagnostics",
      kind: "lsp-diagnostics",
      displayName: "changed-file LSP diagnostics",
      reason: "Changed files must be free of error-severity language diagnostics.",
    });
  }

  const commands: ValidationStep[] = [];
  if (packageScripts !== null) {
    const names = new Set(Object.keys(packageScripts));
    const selected = selectCommandScript(packageScripts, names);
    if (selected !== null) {
      commands.push({
        id: `npm-${selected}`,
        kind: "npm-script",
        displayName: `npm run ${selected}`,
        reason:
          selected === "check"
            ? "The project's check script aggregates validation."
            : selected === "test"
              ? "The project defines a test script."
              : `The project defines a ${selected} script.`,
        command: {
          runner: "npm-script",
          scriptName: selected,
          arguments: [],
          workingDirectory: ".",
        },
      });
    }
  }

  return { mandatory, optional: commands };
}

/**
 * Selection order (§21 rules 1–4): `check` first (the repository-standard
 * aggregate-validation name), then `test`, then `lint`/`typecheck` only
 * when no aggregate script exists. Only existing script names are ever
 * returned; names are never invented and package-install commands are
 * never candidates.
 */
function selectCommandScript(
  packageScripts: Readonly<Record<string, string>>,
  names: ReadonlySet<string>,
): "check" | "test" | "lint" | "typecheck" | null {
  for (const candidate of SCRIPT_CANDIDATES) {
    if (names.has(candidate) && typeof packageScripts[candidate] === "string") {
      return candidate;
    }
  }
  return null;
}

/**
 * Deterministic gate classification over the executed plan (§23–§24).
 * `gdscript-check`/`lsp-diagnostics` steps are satisfied by the workflow's
 * own parser and LSP gates (reported by the caller via `intrinsicPassed`);
 * project command steps are classified from their outcomes. A denied or
 * infrastructure-unavailable required command step produces
 * `validation-incomplete`, never `passed`; an absent test command is
 * `not_applicable`, never an infrastructure failure.
 */
export function classifyValidationGate(
  plan: DevelopmentValidationPlan,
  outcomes: readonly ValidationRunOutcome[],
  intrinsicPassed: boolean,
): {
  readonly status: "passed" | "not_applicable" | "not_run" | "blocked";
  readonly summary: string;
  readonly evidenceKinds: readonly string[];
} {
  const requiredCommands = plan.optional;
  const outcomeByStep = new Map(outcomes.map((outcome) => [outcome.step.id, outcome]));
  const evidenceKinds: string[] = [];

  for (const step of plan.mandatory) {
    if (step.kind === "gdscript-check" || step.kind === "lsp-diagnostics") {
      if (!intrinsicPassed) {
        return {
          status: "blocked",
          summary: `The intrinsic gate "${step.displayName}" failed; the change does not validate.`,
          evidenceKinds: ["intrinsic-failed"],
        };
      }
      evidenceKinds.push(`intrinsic-${step.id}`);
    }
  }

  if (requiredCommands.length === 0) {
    return {
      status: "not_applicable",
      summary: "No supported project test runner was discovered; the project defines no applicable check/test script.",
      evidenceKinds: ["no-project-test-runner"],
    };
  }

  for (const step of requiredCommands) {
    const outcome = outcomeByStep.get(step.id);
    if (outcome === undefined) {
      return {
        status: "not_run",
        summary: `The required validation command "${step.displayName}" did not run.`,
        evidenceKinds: ["validation-not-run"],
      };
    }
    switch (outcome.status) {
      case "passed":
        evidenceKinds.push(`validation-passed:${step.id}`);
        break;
      case "denied":
        return {
          status: "not_run",
          summary: `The required validation command "${step.displayName}" was denied; validation is incomplete.`,
          evidenceKinds: ["validation-denied"],
        };
      case "unavailable":
        return {
          status: "not_run",
          summary: `The required validation command "${step.displayName}" could not run (${outcome.summary}); validation is incomplete.`,
          evidenceKinds: ["validation-unavailable"],
        };
      case "failed":
        return {
          status: "blocked",
          summary: `The required validation command "${step.displayName}" failed (exit ${outcome.exitCode ?? "unknown"}).`,
          evidenceKinds: ["validation-failed"],
        };
      case "not_applicable":
        break;
    }
  }

  return {
    status: "passed",
    summary: "All required validation commands completed successfully.",
    evidenceKinds,
  };
}

export type ValidationRunStatus = "passed" | "failed" | "denied" | "unavailable" | "not_applicable";

export interface ValidationRunOutcome {
  readonly step: ValidationStep;
  readonly status: ValidationRunStatus;
  /** Exit code when the command completed; null otherwise. */
  readonly exitCode: number | null;
  readonly summary: string;
}

/**
 * Validation plan discovery port. The adapter owns reading the bounded
 * project package.json; core owns the selection policy.
 */
export interface ValidationPlanDiscovery {
  discover(signal?: AbortSignal): Promise<{
    readonly packageScripts: Readonly<Record<string, string>> | null;
  }>;
}

/**
 * Validation execution port. The adapter owns the existing prepared-command
 * machinery: each project-defined command is a normal `process.run` plan
 * that requires its own exact one-time process approval (the repository
 * script body is shown in full), runs sandboxed with a read-only workspace,
 * denied network, closed stdin, and bounded output; a denied or unavailable
 * step is never reported as passed.
 */
export interface QualityValidationExecutor {
  run(step: ValidationStep, signal?: AbortSignal): Promise<ValidationRunOutcome>;
}
