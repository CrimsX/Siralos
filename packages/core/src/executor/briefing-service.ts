import type { TaskContract } from "../tasks/task-contract.js";
import { createTaskContract } from "../tasks/task-contract.js";
import type { TaskPlan } from "../planning/planning-model.js";
import type { TaskState } from "../tasks/task-model.js";
import type { ResolvedInstructionSet } from "../instructions/instruction-model.js";
import type { CapabilitySnapshot } from "../doctor/doctor-model.js";
import type { ExecutionContract } from "./execution-contract.js";
import type { MilestoneManifest } from "./milestone-manifest.js";
import type { ArchitectureContextEntry } from "./architecture-context.js";
import type { DocumentationEntry } from "./documentation-context.js";
import type { WorkspaceScope, ActiveWorkingSet } from "./workspace-scope.js";
import type { NewFileRationale } from "./new-file-discipline.js";
import type { ExecutorContextPack, ScopeSignalRef } from "./context-pack.js";
import { buildExecutorContextPack } from "./context-pack.js";
import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import type { ExecutorBrief } from "./brief-compiler.js";
import { compileExecutorBrief, computeExecutorBriefFingerprint } from "./brief-compiler.js";

/**
 * ExecutorBriefing (executor briefing foundation).
 *
 * The host-owned session-level briefing service. It compiles the
 * ExecutorBrief for the CURRENT task on demand and memoizes it: the memo
 * key covers only task-stable identity (task id, contract revision, plan
 * id/revision/approval, milestone version), so unrelated volatile state
 * (new findings, evidence, timestamps) never rewrites a compiled brief —
 * stable content stays stable and the fingerprint stays reproducible.
 *
 * The service is a composition convenience: the compiler itself is pure
 * and deterministic, and the service never grants capability, never
 * mutates TaskState, and never calls a provider.
 */

export interface ExecutorBriefingOptions {
  readonly executionContract: ExecutionContract;
  /** Current milestone manifest; null when none is selected. */
  readonly milestone?: MilestoneManifest | null;
  /** Deterministic milestone selection by task request (host-owned). */
  readonly selectMilestone?: (request: string) => MilestoneManifest | null;
  readonly getTaskContract: () => TaskContract | null;
  readonly getTaskSnapshot: () => TaskState | null;
  readonly getCurrentPlan?: () => TaskPlan | null;
  /** Resolve path-scoped instructions for the task's focus paths. */
  readonly resolveInstructions?: (focusPaths: readonly string[]) => ResolvedInstructionSet | null;
  readonly getCapabilitySnapshot?: () => CapabilitySnapshot | null;
  /** Deterministic architecture index (defaults to the built-in index). */
  readonly architectureIndex?: readonly ArchitectureContextEntry[];
  /** Derived task workspace scope (verified/candidate files, budgets). */
  readonly workspaceScope?: WorkspaceScope | null;
  /** Current plan-step working set. */
  readonly activeWorkingSet?: ActiveWorkingSet | null;
  /** Documentation index override (behavior fixtures inject doc trees). */
  readonly documentationIndex?: readonly DocumentationEntry[];
  /** Deterministic review signals (proliferation / scope expansion). */
  readonly scopeSignals?: readonly ScopeSignalRef[];
  /** Recorded new-production-file rationales. */
  readonly newFiles?: readonly NewFileRationale[];
  /** Restrict capability guidance to these areas (capability-aware). */
  readonly capabilityAreas?: readonly string[];
}

export interface ExecutorBriefing {
  /** Compile (or return the memoized) brief for the current task. */
  latestOrCompile(): ExecutorBrief | null;
  /**
   * Compile a brief for a task that does not exist yet (for example at
   * development-task start, when the snapshot must record the initial
   * fingerprint). Deterministic; independent of the current-task memo.
   */
  compileForRequest(taskId: string, request: string): ExecutorBrief | null;
  /** The last compiled brief, without recompiling. */
  latest(): ExecutorBrief | null;
  /** Deterministic fingerprint of the last compiled brief; null when none. */
  fingerprint(): string | null;
}

interface MemoKey {
  readonly taskId: string;
  readonly contractRevision: number;
  readonly planId: string | null;
  readonly planRevision: number;
  readonly planApproval: string;
  readonly milestoneVersion: number;
  readonly executionContractRevision: number;
  /** Digest of the evolving context inputs (scope, working set, docs, signals). */
  readonly dynamicContextDigest: string;
}

function memoKeyEquals(a: MemoKey, b: MemoKey): boolean {
  return (
    a.taskId === b.taskId &&
    a.contractRevision === b.contractRevision &&
    a.planId === b.planId &&
    a.planRevision === b.planRevision &&
    a.planApproval === b.planApproval &&
    a.milestoneVersion === b.milestoneVersion &&
    a.executionContractRevision === b.executionContractRevision &&
    a.dynamicContextDigest === b.dynamicContextDigest
  );
}

/**
 * Deterministic digest of the evolving context inputs. The workspace
 * scope, working set, documentation index, signals, new-file rationales,
 * and capability areas may change while a task's plan identity stays
 * stable (scope promotion, budget eviction, step transitions) — the memo
 * must not serve a stale brief for the same plan revision.
 */
function dynamicContextDigest(options: ExecutorBriefingOptions): string {
  const dynamic = {
    ...(options.workspaceScope == null ? {} : { workspaceScope: options.workspaceScope }),
    ...(options.activeWorkingSet == null ? {} : { activeWorkingSet: options.activeWorkingSet }),
    ...(options.documentationIndex === undefined
      ? {}
      : { documentationIndex: options.documentationIndex }),
    ...(options.scopeSignals === undefined ? {} : { scopeSignals: options.scopeSignals }),
    ...(options.newFiles === undefined ? {} : { newFiles: options.newFiles }),
    ...(options.capabilityAreas === undefined ? {} : { capabilityAreas: options.capabilityAreas }),
  };
  return sha256Hex(canonicalizeJson(dynamic));
}

export function createExecutorBriefing(options: ExecutorBriefingOptions): ExecutorBriefing {
  const milestone = options.milestone ?? null;
  let memoKey: MemoKey | null = null;
  let memoized: ExecutorBrief | null = null;

  function currentKey(): MemoKey | null {
    const contract = options.getTaskContract();
    if (contract === null) {
      return null;
    }
    const snapshot = options.getTaskSnapshot();
    const plan = options.getCurrentPlan?.() ?? null;
    const selected =
      options.selectMilestone === undefined ? milestone : options.selectMilestone(contract.request);
    return {
      taskId: contract.id,
      contractRevision: contract.revision,
      planId: plan === null ? null : plan.id,
      planRevision: plan === null ? 0 : plan.revision,
      planApproval: snapshot?.plan.approval ?? "none",
      milestoneVersion: selected === null ? 0 : selected.version,
      executionContractRevision: options.executionContract.revision,
      dynamicContextDigest: dynamicContextDigest(options),
    };
  }

  function compile(): ExecutorBrief | null {
    return compileForContract(options.getTaskContract());
  }

  function compileForContract(contract: TaskContract | null): ExecutorBrief | null {
    if (contract === null) {
      return null;
    }
    const snapshot = options.getTaskSnapshot();
    const plan = options.getCurrentPlan?.() ?? null;
    const selected =
      options.selectMilestone === undefined ? milestone : options.selectMilestone(contract.request);
    const focusPaths = plan === null ? [] : plan.touchpoints.map((touchpoint) => touchpoint.path);
    const instructions =
      options.resolveInstructions === undefined || focusPaths.length === 0
        ? null
        : options.resolveInstructions(focusPaths);
    const pack: ExecutorContextPack = buildExecutorContextPack({
      contract,
      plan,
      executionContract: {
        id: options.executionContract.id,
        revision: options.executionContract.revision,
      },
      milestone: selected,
      instructions,
      ...(selected === null ? {} : { architectureConcerns: selected.architectureConcerns }),
      ...(options.architectureIndex === undefined
        ? {}
        : { architectureIndex: options.architectureIndex }),
      capabilitySnapshot: options.getCapabilitySnapshot?.() ?? null,
      findings: snapshot?.currentFindings ?? [],
      ...(snapshot === null ? {} : { planApproval: snapshot.plan.approval }),
      ...(options.workspaceScope === undefined ? {} : { workspaceScope: options.workspaceScope }),
      ...(options.activeWorkingSet === undefined
        ? {}
        : { activeWorkingSet: options.activeWorkingSet }),
      ...(options.documentationIndex === undefined
        ? {}
        : { documentationIndex: options.documentationIndex }),
      ...(options.scopeSignals === undefined ? {} : { scopeSignals: options.scopeSignals }),
      ...(options.newFiles === undefined ? {} : { newFiles: options.newFiles }),
      ...(options.capabilityAreas === undefined
        ? {}
        : { capabilityAreas: options.capabilityAreas }),
    });
    return compileExecutorBrief({
      contract,
      executionContract: options.executionContract,
      pack,
      milestone: selected,
    });
  }

  return {
    latestOrCompile(): ExecutorBrief | null {
      const key = currentKey();
      if (key === null) {
        memoKey = null;
        memoized = null;
        return null;
      }
      if (memoKey !== null && memoKeyEquals(memoKey, key) && memoized !== null) {
        return memoized;
      }
      memoKey = key;
      memoized = compile();
      return memoized;
    },
    compileForRequest(taskId: string, request: string): ExecutorBrief | null {
      if (taskId.length === 0 || request.trim().length === 0) {
        return null;
      }
      const contract = createTaskContract({
        id: taskId,
        request,
        acceptanceCriteria: [
          {
            id: "host-verified",
            description: "The requested work is complete and verified by the host.",
            verificationKind: "user",
          },
        ],
      });
      return compileForContract(contract);
    },
    latest(): ExecutorBrief | null {
      return memoized;
    },
    fingerprint(): string | null {
      return memoized === null ? null : computeExecutorBriefFingerprint(memoized);
    },
  };
}
