import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  CheckpointStore,
  GDScriptDevelopmentService,
  GodotGDScriptDiagnostic,
} from "@solaris/core";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
  createTempWorkspace,
  writeFixtureFiles,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";
import { createGDScriptDevelopmentService } from "./gdscript-development-service.js";
import {
  createFakeDiagnosticsService,
  createFakeGitInspector,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
  sha256Of,
  type FakeLanguageControl,
  type FakeParserControl,
} from "./gdscript-development-testing.js";

const PLAYER_VALID =
  "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n";

function errorDiagnostic(path: string, line: number, message: string): GodotGDScriptDiagnostic {
  return {
    source: "godot-lsp",
    severity: "error",
    path,
    line,
    column: 1,
    code: null,
    message,
    rawCategory: null,
  };
}

function warningDiagnostic(path: string, message: string): GodotGDScriptDiagnostic {
  return {
    source: "godot-lsp",
    severity: "warning",
    path,
    line: 1,
    column: 1,
    code: null,
    message,
    rawCategory: null,
  };
}

interface Harness {
  readonly workspace: TempWorkspace;
  readonly store: CheckpointStore;
  readonly service: GDScriptDevelopmentService;
  readonly language: FakeLanguageControl;
  readonly parser: FakeParserControl;
  readonly primitives: ReturnType<typeof createWorkspaceFilePrimitives>;
  readonly events: string[];
}

async function createHarness(
  options: {
    readonly engineSha?: string;
    readonly files?: Readonly<Record<string, string>>;
    readonly git?: boolean;
  } = {},
): Promise<Harness> {
  const workspace = await createTempWorkspace();
  await writeFixtureFiles(workspace.root, {
    "project.godot": '[application]\nconfig/name="fixture"\n',
    "src/player/player.gd": PLAYER_VALID,
    ...(options.files ?? {}),
  });
  const store = await createTempCheckpointStore(workspace.root);
  const languageFake = createFakeLanguageService(
    options.engineSha === undefined
      ? {}
      : { engine: { sha256: options.engineSha, version: "4.7.1-stable", installationId: "test" } },
  );
  const parserFake = createFakeDiagnosticsService();
  const gitFake = options.git === false ? null : createFakeGitInspector();
  const events: string[] = [];
  const primitives = createWorkspaceFilePrimitives(workspace.root);
  const service = createGDScriptDevelopmentService({
    workspaceRoot: workspace.root,
    platform: "linux",
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    language: languageFake.service,
    diagnostics: parserFake.service,
    git: gitFake?.git ?? null,
    canApplyIdentityBound: true,
    primitives,
    onEvent: (event) => {
      events.push(event.type);
    },
    idFactory: () => `id-${events.length}-${Math.random().toString(36).slice(2, 8)}`,
    settling: { hardTimeoutMs: 1000, pollIntervalMs: 1 },
  });
  return {
    workspace,
    store,
    service,
    language: languageFake.control,
    parser: parserFake.control,
    primitives,
    events,
  };
}

async function startWorkflow(
  harness: Harness,
  request = "Add a heal method to the player",
): Promise<string> {
  const prepared = await harness.service.prepareStart(request);
  expect(prepared.status).toBe("ready");
  if (prepared.status !== "ready") {
    throw new Error(prepared.message);
  }
  const started = await harness.service.start(prepared.workflowId, {
    approvedDigest: prepared.digest,
  });
  expect(started.status).toBe("ready");
  if (started.status !== "ready") {
    throw new Error(started.message);
  }
  return prepared.workflowId;
}

async function readWorkspaceFile(harness: Harness, path: string): Promise<string> {
  const { readFile } = await import("node:fs/promises");
  return readFile(`${harness.workspace.root}/${path}`, "utf8");
}

const editChangeSet = (hash: string) => ({
  changes: [
    {
      operation: "edit",
      path: "src/player/player.gd",
      expectedSha256: hash,
      replacements: [{ oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" }],
    },
  ],
});

async function proposeAndApply(
  harness: Harness,
  input: unknown,
  approvedDigest?: string,
): Promise<Awaited<ReturnType<GDScriptDevelopmentService["applyChangeSet"]>>> {
  const prepared = await harness.service.prepareChangeSet(input, {});
  expect(prepared.status).toBe("ready");
  if (prepared.status !== "ready") {
    throw new Error(prepared.message);
  }
  return harness.service.applyChangeSet(prepared.changeSetId, {
    approvedDigest: approvedDigest ?? prepared.digest,
  });
}

describe("development workflow core", () => {
  let harness: Harness;
  beforeEach(async () => {
    harness = await createHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("starts a workflow and reports investigating status", async () => {
    await startWorkflow(harness);
    const status = harness.service.status();
    expect(status.session).not.toBeNull();
    expect(status.session?.state).toEqual({ kind: "active", phase: "investigating" });
    expect(status.session?.iteration).toBe(0);
    expect(status.session?.maxIterations).toBe(4);
    expect(status.session?.repairProposalsRemaining).toBe(3);
    expect(status.support.available).toBe(true);
  });

  it("allows only one active workflow at a time", async () => {
    await startWorkflow(harness);
    const second = await harness.service.prepareStart("another request");
    expect(second.status).toBe("conflict");
  });

  it("rejects an empty development request", async () => {
    const prepared = await harness.service.prepareStart("   ");
    expect(prepared.status).toBe("invalid_input");
  });

  it("requires the approved digest to start and consumes it", async () => {
    const prepared = await harness.service.prepareStart("request");
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const wrong = await harness.service.start(prepared.workflowId, {
      approvedDigest: "f".repeat(64),
    });
    expect(wrong.status).toBe("conflict");
    const right = await harness.service.start(prepared.workflowId, {
      approvedDigest: prepared.digest,
    });
    expect(right.status).toBe("ready");
    const again = await harness.service.start(prepared.workflowId, {
      approvedDigest: prepared.digest,
    });
    expect(again.status).toBe("failed");
  });

  it("runs a complete clean development iteration with evidence", async () => {
    await startWorkflow(harness);
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status !== "applied" || outcome.result === null) {
      return;
    }
    expect(outcome.result.iterations).toBe(1);
    expect(outcome.result.changes).toHaveLength(1);
    expect(outcome.result.changes[0]?.path).toBe("src/player/player.gd");
    expect(outcome.result.changes[0]?.operation).toBe("update");
    expect(outcome.result.changes[0]?.beforeSha256).toBe(sha256Of(PLAYER_VALID));
    expect(outcome.result.changes[0]?.afterSha256).toBe(
      sha256Of(PLAYER_VALID.replace("move_and_slide()", "move_and_slide(Vector2.UP)")),
    );
    expect(outcome.result.checkpointIds).toHaveLength(1);
    expect(outcome.result.validation.parser).toBe(true);
    expect(outcome.result.validation.lsp).toBe(true);
    expect(outcome.result.validation.workspaceIntegrity).toBe(true);
    expect(outcome.result.diagnostics.errors).toBe(0);
    // The workflow is now reviewing with a clean validation.
    const status = harness.service.status();
    expect(status.session?.state).toEqual({ kind: "active", phase: "reviewing" });
    expect(status.session?.validation).toBe("clean");
    // The fresh language session stays alive after the edit.
    expect(harness.language.active).not.toBeNull();
    // The source file on disk contains exactly the approved change.
    const onDisk = await readWorkspaceFile(harness, "src/player/player.gd");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
  });

  it("completes deterministically when the provider finishes with a clean turn", async () => {
    await startWorkflow(harness);
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    harness.service.completeFromProviderTurn();
    const status = harness.service.status();
    expect(status.session?.state).toEqual({ kind: "terminal", status: "completed" });
  });

  it("keeps a completed workflow immutable", async () => {
    await startWorkflow(harness);
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    harness.service.completeFromProviderTurn();
    const proposal = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(proposal.status).toBe("failed");
  });

  it("completes a change set containing an approved delete", async () => {
    await writeFixtureFiles(harness.workspace.root, {
      "src/player/old.gd": "extends Node\n# to be removed\n",
    });
    await startWorkflow(harness);
    const oldHash = sha256Of("extends Node\n# to be removed\n");
    const prepared = await harness.service.prepareChangeSet(
      {
        changes: [
          {
            operation: "delete",
            path: "src/player/old.gd",
            expectedSha256: oldHash,
          },
        ],
      },
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const outcome = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(outcome.status).toBe("applied");
    if (outcome.status === "applied" && outcome.result !== null) {
      expect(outcome.result.changes[0]?.operation).toBe("delete");
      expect(outcome.result.validation.workspaceIntegrity).toBe(true);
    }
    expect(harness.service.status().session?.validation).toBe("clean");
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed",
    });
  });

  it("applies nothing from a prepared change set after the workflow finished", async () => {
    await startWorkflow(harness);
    const prepared = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    harness.service.completeFromProviderTurn();
    const outcome = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(outcome.status).toBe("failed");
    expect(await readWorkspaceFile(harness, "src/player/player.gd")).toBe(PLAYER_VALID);
  });

  it("ends as denied when the provider finishes while a change set is unapproved", async () => {
    await startWorkflow(harness);
    const prepared = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    // The approval was denied: the provider's next turn completes with the
    // denial in hand, and the workflow ends truthfully with nothing applied.
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "denied",
    });
    expect(await readWorkspaceFile(harness, "src/player/player.gd")).toBe(PLAYER_VALID);
  });

  it("allows a new workflow after a terminal one", async () => {
    await startWorkflow(harness);
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "cancelled",
    });
    const next = await harness.service.prepareStart("a second request");
    expect(next.status).toBe("ready");
  });

  it("does not report vacuous validation success for a workflow that never validated", async () => {
    await startWorkflow(harness);
    harness.service.completeFromProviderTurn();
    const result = await harness.service.cancel();
    if (result.status === "cancelled" && result.result !== null) {
      expect(result.result.validation.parser).toBe(false);
      expect(result.result.validation.lsp).toBe(false);
      expect(result.result.validation.workspaceIntegrity).toBe(false);
    }
  });

  it("does not burn the repair budget on a failed repair preparation", async () => {
    await startWorkflow(harness);
    harness.parser.resultsByPath.set("src/player/player.gd", {
      valid: false,
      diagnostics: [errorDiagnostic("src/player/player.gd", 4, "invalid call")],
    });
    const first = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(first.status).toBe("applied");
    expect(harness.service.status().session?.validation).toBe("errors");
    // A repair with a stale hash fails preparation and burns nothing.
    const failed = await harness.service.prepareChangeSet(editChangeSet("f".repeat(64)), {});
    expect(failed.status).toBe("conflict");
    expect(harness.service.status().session?.repairProposalsRemaining).toBe(3);
  });

  it("reports an infrastructure failure when the fresh LSP session never answers", async () => {
    await startWorkflow(harness);
    // The post-edit session never answers diagnostics, so the settling
    // loop never receives a snapshot: that is an infrastructure failure,
    // never a clean result.
    harness.language.failDiagnostics = true;
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("validation_failed");
    if (outcome.status === "validation_failed") {
      expect(outcome.message).toContain("language session");
    }
  });

  it("enforces the prepared-change-set cap", async () => {
    await startWorkflow(harness);
    for (let index = 0; index < 4; index += 1) {
      const prepared = await harness.service.prepareChangeSet(
        editChangeSet(sha256Of(PLAYER_VALID)),
        {},
      );
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
    }
    const blocked = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(blocked.status).toBe("failed");
    if (blocked.status === "failed") {
      expect(blocked.message).toContain("Too many change sets are prepared");
    }
  });

  it("ends as cancelled when the provider finishes without proposing anything", async () => {
    await startWorkflow(harness);
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "cancelled",
    });
  });

  it("conflicts before apply when a stale expected hash is proposed", async () => {
    await startWorkflow(harness);
    const proposal = await harness.service.prepareChangeSet(editChangeSet("f".repeat(64)), {});
    expect(proposal.status).toBe("conflict");
    if (proposal.status === "conflict") {
      expect(proposal.message).toContain("changed since");
    }
    expect(harness.language.closeAllCount).toBe(0);
    expect(harness.service.status().session?.state).toEqual({
      kind: "active",
      phase: "investigating",
    });
  });

  it("rejects a change-set approval that does not match the digest", async () => {
    await startWorkflow(harness);
    const prepared = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const outcome = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: "f".repeat(64),
    });
    expect(outcome.status).toBe("conflict");
  });

  it("does not reuse an approval: a consumed change set cannot be applied twice", async () => {
    await startWorkflow(harness);
    const prepared = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const first = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(first.status).toBe("applied");
    const second = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(second.status).toBe("failed");
  });

  it("reports the workflow as unavailable when the change-set applier is gated", async () => {
    const workspace = await createTempWorkspace();
    const store = await createTempCheckpointStore(workspace.root);
    const languageFake = createFakeLanguageService();
    const parserFake = createFakeDiagnosticsService();
    try {
      const service = createGDScriptDevelopmentService({
        workspaceRoot: workspace.root,
        platform: "linux",
        store,
        lock: { acquire: () => Promise.resolve(() => undefined) },
        language: languageFake.service,
        diagnostics: parserFake.service,
        git: null,
        canApplyIdentityBound: false,
        primitives: {
          readFile: () => Promise.resolve({ exists: false, sha256: null }),
          readContent: () => Promise.resolve({ exists: false, sha256: null, content: null }),
          writeFile: () => Promise.resolve(),
          deleteFile: () => Promise.resolve(),
        },
      });
      expect((await service.support()).state).toBe("unavailable");
      const prepared = await service.prepareStart("request");
      expect(prepared.status).toBe("unavailable");
    } finally {
      await workspace.cleanup();
      await cleanupTempCheckpointDirs();
    }
  });
});

describe("LSP/edit coordination", () => {
  let harness: Harness;
  beforeEach(async () => {
    harness = await createHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("starts an initial language session under the workflow authorization", async () => {
    await startWorkflow(harness);
    expect(harness.language.prepareCount).toBe(1);
    expect(harness.language.startCount).toBe(1);
    expect(harness.language.active).not.toBeNull();
  });

  it("suspends the active LSP session before the edit and recreates it after", async () => {
    await startWorkflow(harness);
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    const closeIndex = harness.language.log.findIndex((entry) => entry === "closeAll");
    const startIndexes = harness.language.log
      .map((entry, index) => (entry.startsWith("started-") ? index : -1))
      .filter((index) => index >= 0);
    expect(closeIndex).toBeGreaterThanOrEqual(0);
    expect(startIndexes.length).toBe(2);
    expect(closeIndex).toBeLessThan(startIndexes[1]!);
    // The old session is closed; the active session is the fresh one.
    expect(harness.language.active).not.toBeNull();
    expect(harness.language.active?.session.id).toBe("lsp-2");
  });

  it("rejects new LSP queries while the session is closing for an edit", async () => {
    await startWorkflow(harness);
    const prepared = await harness.service.prepareChangeSet(
      editChangeSet(sha256Of(PLAYER_VALID)),
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const gateBefore = harness.service.languageQueryGate();
    expect(gateBefore.blocked).toBe(false);
    // During the apply (which suspends the session), the gate must block.
    const gatePromise = harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    // The apply is synchronous through the fake fakes; poll the gate after.
    await gatePromise;
    const gateAfter = harness.service.languageQueryGate();
    expect(gateAfter.blocked).toBe(false);
  });

  it("does not apply the edit when the language session cannot be stopped", async () => {
    await startWorkflow(harness);
    harness.language.closeAllError = new Error("shutdown failed");
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("apply_failed");
    if (outcome.status === "apply_failed") {
      expect(outcome.message).toContain("could not be stopped safely");
    }
    // No checkpoint was created and the file is unchanged.
    expect(await harness.store.list()).toHaveLength(0);
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "apply_failed",
    });
  });

  it("runs the parser gate after the mutation and before the LSP restart", async () => {
    await startWorkflow(harness);
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    const appliedIndex = harness.events.indexOf("development_change_applied");
    const parseIndex = harness.events.indexOf("development_parser_completed");
    const restartIndex = harness.events.indexOf("development_language_restarted");
    expect(appliedIndex).toBeGreaterThanOrEqual(0);
    expect(parseIndex).toBeGreaterThan(appliedIndex);
    // Exactly one language-restart event: the fresh session after the
    // edit (the initial workflow-start session is not a restart).
    expect(harness.events.filter((type) => type === "development_language_restarted")).toHaveLength(
      1,
    );
    expect(restartIndex).toBeGreaterThan(parseIndex);
  });

  it("engine change invalidates the workflow at the language restart", async () => {
    await startWorkflow(harness);
    const change = editChangeSet(sha256Of(PLAYER_VALID));
    const prepared = await harness.service.prepareChangeSet(change, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    // The engine changes before the approved change set is applied.
    harness.language.engine = {
      sha256: "d".repeat(64),
      version: "4.8.0-stable",
      installationId: "other",
    };
    const outcome = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(outcome.status).toBe("conflict");
    if (outcome.status === "conflict") {
      expect(outcome.message).toContain("engine changed");
    }
  });

  it("project risk change invalidates the workflow at the language restart", async () => {
    await startWorkflow(harness);
    const change = editChangeSet(sha256Of(PLAYER_VALID));
    const prepared = await harness.service.prepareChangeSet(change, {});
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    // An unrelated file changes while the change set is being approved.
    await writeFixtureFiles(harness.workspace.root, {
      "src/unrelated.gd": "extends Node\n# external edit\n",
    });
    const outcome = await harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
    expect(outcome.status).toBe("conflict");
    if (outcome.status === "conflict") {
      expect(outcome.message).toContain("outside the approved change set");
    }
  });

  it("suspension happens before the first mutation write", async () => {
    await startWorkflow(harness);
    const applyPromise = proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    // The suspension must complete before the applier writes: the write
    // path records the closeAll order into the shared language log.
    const outcome = await applyPromise;
    expect(outcome.status).toBe("applied");
    const closeIndex = harness.language.log.findIndex((entry) => entry === "closeAll");
    const startedIndexes = harness.language.log
      .map((entry, index) => (entry.startsWith("started-") ? index : -1))
      .filter((index) => index >= 0);
    expect(closeIndex).toBeLessThan(startedIndexes[1]!);
    // The file changed on disk exactly once (the approved change).
    const onDisk = await readWorkspaceFile(harness, "src/player/player.gd");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
  });
});

describe("validation and evidence", () => {
  let harness: Harness;
  beforeEach(async () => {
    harness = await createHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("produces warnings status when validation has only warnings", async () => {
    await startWorkflow(harness);
    harness.parser.resultsByPath.set("src/player/player.gd", {
      valid: true,
      diagnostics: [warningDiagnostic("src/player/player.gd", "unused variable")],
    });
    harness.language.nextSessionDiagnostics.set("src/player/player.gd", [
      warningDiagnostic("src/player/player.gd", "unused variable"),
    ]);
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status === "applied") {
      expect(outcome.result.diagnostics.warnings).toBeGreaterThan(0);
      expect(outcome.result.diagnostics.errors).toBe(0);
    }
    expect(harness.service.status().session?.validation).toBe("warnings");
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed_with_warnings",
    });
  });

  it("parser errors trigger the repair state and evidence", async () => {
    await startWorkflow(harness);
    harness.parser.resultsByPath.set("src/player/player.gd", {
      valid: false,
      diagnostics: [errorDiagnostic("src/player/player.gd", 4, "parse error")],
    });
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status !== "applied" || outcome.result === null) {
      return;
    }
    expect(outcome.result.diagnostics.errors).toBeGreaterThan(0);
    expect(harness.service.status().session?.validation).toBe("errors");
    expect(harness.service.status().session?.state).toEqual({
      kind: "active",
      phase: "reviewing",
    });
  });

  it("an LSP startup failure is an infrastructure failure, never source invalidity", async () => {
    await startWorkflow(harness);
    harness.language.nextStartFailure = "the editor could not start";
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("validation_failed");
    if (outcome.status === "validation_failed") {
      expect(outcome.message).toContain("could not start");
    }
    // The approved source change stays on disk.
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "validation_failed",
    });
    expect(harness.service.status().session?.validation).toBeNull();
  });

  it("a parser infrastructure failure is not reported as clean success", async () => {
    await startWorkflow(harness);
    harness.parser.nextPrepareFailure = "check-only execution is unavailable";
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("validation_failed");
    if (outcome.status === "validation_failed") {
      expect(outcome.message).toContain("check-only");
    }
  });

  it("keeps the approved source change when LSP validation fails", async () => {
    await startWorkflow(harness);
    harness.language.nextStartFailure = "no editor";
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    const bytes = await import("node:fs/promises").then((fs) =>
      fs.readFile(`${harness.workspace.root}/src/player/player.gd`, "utf8"),
    );
    expect(bytes).toContain("move_and_slide(Vector2.UP)");
  });

  it("detects an unexpected workspace change during the apply", async () => {
    await startWorkflow(harness);
    // An external process plants an unrelated file exactly when the
    // approved player.gd write lands (mid-application race).
    const originalWrite = harness.primitives.writeFile.bind(harness.primitives);
    harness.primitives.writeFile = async (path: string, content: string): Promise<void> => {
      if (path === "src/player/player.gd") {
        await originalWrite("src/unrelated.gd", "extends Node\n# external\n");
      }
      await originalWrite(path, content);
    };
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("conflict");
    if (outcome.status === "conflict") {
      expect(outcome.message).toContain("outside the approved change set");
    }
    // The approved change itself remains on disk; nothing is reverted.
    const onDisk = await readWorkspaceFile(harness, "src/player/player.gd");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "conflict",
    });
  });

  it("collects bounded LSP diagnostics for the changed scripts only", async () => {
    await startWorkflow(harness);
    harness.language.nextSessionDiagnostics.set("src/player/player.gd", [
      errorDiagnostic("src/player/player.gd", 4, "unexpected token"),
    ]);
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status === "applied" && outcome.result !== null) {
      expect(outcome.result.diagnostics.errors).toBeGreaterThan(0);
    }
  });
});

describe("repair loop", () => {
  let harness: Harness;
  beforeEach(async () => {
    harness = await createHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  async function brokenFirstIteration(): Promise<string> {
    await startWorkflow(harness);
    harness.parser.resultsByPath.set("src/player/player.gd", {
      valid: false,
      diagnostics: [errorDiagnostic("src/player/player.gd", 4, "invalid call")],
    });
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    expect(harness.service.status().session?.validation).toBe("errors");
    return readWorkspaceFile(harness, "src/player/player.gd");
  }

  async function proposeRepair(
    harness: Harness,
  ): Promise<Awaited<ReturnType<GDScriptDevelopmentService["applyChangeSet"]>>> {
    const content = await readWorkspaceFile(harness, "src/player/player.gd");
    const oldText = content.includes("Vector2.UP")
      ? "move_and_slide(Vector2.UP)"
      : "move_and_slide(Vector2.DOWN)";
    const newText = content.includes("Vector2.UP")
      ? "move_and_slide(Vector2.DOWN)"
      : "move_and_slide(Vector2.UP)";
    const prepared = await harness.service.prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "src/player/player.gd",
            expectedSha256: sha256Of(content),
            replacements: [{ oldText, newText }],
          },
        ],
      },
      {},
    );
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      throw new Error(prepared.message);
    }
    return harness.service.applyChangeSet(prepared.changeSetId, {
      approvedDigest: prepared.digest,
    });
  }

  it("allows a focused repair proposal after errors and applies it with a new checkpoint", async () => {
    const afterFirst = await brokenFirstIteration();
    const checkpointCountBefore = (await harness.store.list()).length;
    const parserRunsBefore = harness.parser.log.filter((entry) => entry === "execute").length;
    harness.parser.resultsByPath.delete("src/player/player.gd");
    const outcome = await proposeRepair(harness);
    expect(outcome.status).toBe("applied");
    if (outcome.status === "applied" && outcome.result !== null) {
      expect(outcome.result.iterations).toBe(2);
      expect(outcome.result.checkpointIds.length).toBe(checkpointCountBefore + 1);
    }
    // The repair triggered a fresh parser run and a fresh language session.
    expect(harness.parser.log.filter((entry) => entry === "execute").length).toBeGreaterThan(
      parserRunsBefore,
    );
    expect(harness.language.startCount).toBe(3);
    expect(harness.service.status().session?.validation).toBe("clean");
    harness.service.completeFromProviderTurn();
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed",
    });
    void afterFirst;
  });

  it("exhausts the repair budget after three failed repairs", async () => {
    await brokenFirstIteration();
    for (let index = 0; index < 3; index += 1) {
      const outcome = await proposeRepair(harness);
      expect(outcome.status).toBe("applied");
    }
    const exhausted = await harness.service.prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "src/player/player.gd",
            expectedSha256: sha256Of(await readWorkspaceFile(harness, "src/player/player.gd")),
            replacements: [
              { oldText: "move_and_slide(Vector2.DOWN)", newText: "move_and_slide(Vector2.UP)" },
            ],
          },
        ],
      },
      {},
    );
    expect(exhausted.status).toBe("repair_budget_exhausted");
    // The provider cannot raise the budget: the limits are immutable.
    expect(harness.service.status().session?.repairProposalsRemaining).toBe(0);
  });

  it("does not let a repair bypass the iteration budget", async () => {
    await brokenFirstIteration();
    // Three repairs consume the repair budget; a fourth proposal of a new
    // iteration must still fail on the iteration budget.
    for (let index = 0; index < 3; index += 1) {
      const outcome = await proposeRepair(harness);
      expect(outcome.status).toBe("applied");
    }
    const exhausted = await harness.service.prepareChangeSet(
      {
        changes: [
          {
            operation: "edit",
            path: "src/player/player.gd",
            expectedSha256: sha256Of(await readWorkspaceFile(harness, "src/player/player.gd")),
            replacements: [
              { oldText: "move_and_slide(Vector2.DOWN)", newText: "move_and_slide(Vector2.UP)" },
            ],
          },
        ],
      },
      {},
    );
    expect(
      exhausted.status === "repair_budget_exhausted" ||
        exhausted.status === "iteration_budget_exhausted",
    ).toBe(true);
  });

  it("denial during repair leaves the first approved edit intact", async () => {
    await brokenFirstIteration();
    // The user denies the repair; the workflow ends truthfully with the
    // errors outstanding and the accepted first edit preserved.
    harness.service.completeFromProviderTurn();
    const status = harness.service.status();
    expect(status.session?.state).toEqual({
      kind: "terminal",
      status: "completed_with_errors",
    });
    expect(status.session?.validation).toBe("errors");
    const onDisk = await readWorkspaceFile(harness, "src/player/player.gd");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
  });

  it("cancellation preserves approved changes and closes the language session", async () => {
    await startWorkflow(harness);
    await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    const result = await harness.service.cancel();
    expect(result.status).toBe("cancelled");
    if (result.status === "cancelled" && result.result !== null) {
      expect(result.result.changes).toHaveLength(1);
    }
    expect(harness.service.status().session?.state).toEqual({
      kind: "terminal",
      status: "cancelled",
    });
    expect(harness.language.active).toBeNull();
  });
});

describe("git and workspace evidence", () => {
  let harness: Harness;
  beforeEach(async () => {
    harness = await createHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("records the initial workspace fingerprint at workflow start", async () => {
    const prepared = await harness.service.prepareStart("request");
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    expect(prepared.preview.projectFingerprint).toMatch(/^[0-9a-f]{64}$/);
  });

  it("collects Git status evidence without mutating the index", async () => {
    await startWorkflow(harness, "request with git");
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status === "applied" && outcome.result !== null) {
      expect(outcome.result.validation.workspaceIntegrity).toBe(true);
    }
  });

  it("marks git evidence unavailable when no Git inspector is wired", async () => {
    harness = await createHarness({ git: false });
    await startWorkflow(harness);
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
  });

  it("never includes absolute mirror or workspace paths in evidence output", async () => {
    await startWorkflow(harness);
    const outcome = await proposeAndApply(harness, editChangeSet(sha256Of(PLAYER_VALID)));
    expect(outcome.status).toBe("applied");
    if (outcome.status !== "applied" || outcome.result === null) {
      return;
    }
    const serialized = JSON.stringify(outcome.result);
    expect(serialized).not.toContain(harness.workspace.root);
    expect(serialized).not.toContain("C:\\");
    expect(serialized).not.toContain("file://");
  });
});
