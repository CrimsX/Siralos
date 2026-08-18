# Stage 3R R7 — Provider, Tool Loop, Projection, Configuration, and CLI Behavior Extraction

Status: behavior-extraction and acceptance record for Stage 3R R7. R7.3
Projection parity is complete and evidence-backed; R7.4 Configuration parity
is a completed Rust candidate pending independent completion review. The
TypeScript implementation remains the behavioral oracle until R12;
behavioral parity does not require structural parity (ADR 0032).

This document is the R7 acceptance/evidence design. It freezes the observable
TypeScript behavioral contract for the five R7 surfaces so that the Rust R7
implementation is mechanical rather than exploratory. Executable candidate
implementation and acceptance evidence are recorded in
`docs/development/RUST_MIGRATION.md`; this document remains the contract and
review record.

R7A remediation note: this document records one intentional correction to the
behavioral oracle — **reference protocol hardening discovered during R7
behavior extraction**. The runtime provider-event discriminator contract
(§2.1.1, §4 F28-F29) was corrected in the TypeScript reference before porting:
externally supplied provider events are untrusted data, and unknown or
malformed runtime discriminators now fail closed instead of being
reinterpreted by field shape. This is not R7 implementation work.

---

## 1. Baseline

Recorded before any extraction work:

| Item           | Value                                                                                                                                                                                                            |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Branch         | `main`                                                                                                                                                                                                           |
| HEAD           | `0d02db4a3189517c1c85433e455d39906c27bf4f`                                                                                                                                                                       |
| Upstream       | `0d02db4a3189517c1c85433e455d39906c27bf4f` (origin/main, up to date)                                                                                                                                             |
| Worktree       | Clean (`git status --short` empty)                                                                                                                                                                               |
| Baseline check | `npm run check` — **PASS**, exit code 0 (full gate: format, lint, typecheck, unit/integration, architecture, identity ratchet, Rust architecture, full Rust gate incl. 159 core unit tests + domain conformance) |

No repository files were modified during extraction except this document.

## 2. R7 behavior map

### 2.1 Provider

| Aspect                | Evidence                                                                                                                                                                                                                                                                                 |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript source     | `packages/core/src/ports/provider.ts` (port), `packages/core/src/application/provider-turn.ts` (turn collection), `packages/adapters/src/providers/bounded-model-turn.ts` (strict collector + result detach), `packages/adapters/src/providers/deterministic-fake-provider.ts` (adapter) |
| Tests                 | `packages/core/src/application/provider-protocol.test.ts` (turn protocol), `packages/adapters/src/providers/bounded-model-turn.test.ts`, `packages/adapters/src/providers/deterministic-fake-provider.test.ts`, `packages/core/src/application/tool-loop.test.ts` (loop interaction)     |
| Callers               | `packages/core/src/application/application.ts` (`sendPrompt`), `apps/cli/src/interactive-session.ts`, `apps/cli/src/bootstrap/create-application.ts` (composition), planner/reviewer executor factories                                                                                  |
| Observable contract   | See §2.1.1 and the bounding contract in §5.1                                                                                                                                                                                                                                             |
| Authority owner       | Provider output is untrusted data; the Host validates, bounds, detaches, and commits it                                                                                                                                                                                                  |
| Rust owner            | `siralos-core`: provider-neutral request/event/result types and the strict turn-collection state machine; `siralos-adapters`: deterministic fake provider and bounded stream consumption; `siralos-cli`: none                                                                            |
| Differential evidence | New `provider-turn` subject (§8)                                                                                                                                                                                                                                                         |

#### 2.1.1 Provider contract (exact)

- `ModelRequest` = `{ messages: readonly ConversationItem[]; tools: readonly ToolDefinition[]; system?: string; signal?: AbortSignal }` (provider.ts:4-10). The Host builds every field; the provider only receives.
- `ModelProvider` = `{ id: string; toolCalling?: boolean; stream(request): AsyncIterable<ModelEvent> }` (provider.ts:27-37). `toolCalling` absent means supported; a `false` route must fail clearly up front in tool-requiring modes — never a silent text-only session (projection-service.ts:674-699).
- `ModelEvent` union is closed and exactly three variants (provider.ts:12-25): `text_delta {text}`, `tool_call {callId, toolName, input}`, `completed`. **Runtime contract (R7A hardening):** the TypeScript discriminated union is not a runtime trust boundary — provider events are externally supplied data, so the runtime discriminator is authoritative and validated explicitly in both collectors (`collectProviderTurn`, provider-turn.ts, and `collectBoundedModelTurn`, bounded-model-turn.ts). Unknown discriminators fail the turn closed; an event is never reinterpreted as `tool_call`, `text_delta`, or `completed` from its field shape, and malformed values of known variants (non-object events, non-string `text`, non-string `callId`/`toolName`) fail deterministically without TypeErrors or coercion. Extra fields on a `completed` event are ignored (only the discriminator is authoritative). Exact messages: §4 F28-F29.
- **Transcript validation before every request** (`validateConversationItems`, conversation.ts:33-77): every `assistant_tool_call` must be followed by exactly one `tool_result` for the same call id before the next `user_message`; no orphan results; no duplicate call ids; no empty call ids. A structurally invalid history fails the turn closed ("The conversation transcript is structurally invalid; the provider request was blocked: …", provider-turn.ts:79-85).
- **Iterator EOF is not completion** ("The provider stream ended without a completion event; the response was rejected.", provider-turn.ts:254-258).
- **Any event after `completed` rejects the turn** ("an event after completion", provider-turn.ts:144-147); the iterator is closed best-effort via `iterator.return()` (provider-turn.ts:240-244, 310-320) and the already-chosen outcome is authoritative.
- **Provider-thrown errors**: cancellation (signal aborted or `AbortError` named error, domain/cancellation.ts:1-3) → cancelled; everything else → failed with the error message (provider-turn.ts:235-239).
- **Assistant text**: deltas streamed to the caller as they arrive; text is committed to history only as one `assistant_message` when the turn completes (application.ts:192-198, 218-221); a rejected turn never commits partial text (provider-protocol.test.ts:147-161, 338-353).
- **Tool calls**: JSON round-tripped (`JSON.parse(JSON.stringify(input))`) before retention/execution so caller references and unknown fields never cross (provider-turn.ts:175-232; provider-protocol.test.ts:241-264). Empty call id or empty tool name → an `invalid` call with synthetic id `invalid-call-N` (provider-turn.ts:209-216); duplicate call id → `invalid` call "Duplicate tool call id: …" while the first occurrence still executes (provider-turn.ts:217-224; tool-loop.test.ts:179-198). **Layer boundary (R7A):** the provider-turn layer produces only the `TurnToolCall` outcome (execute/invalid) — it never creates a `tool_result`. The paired failed `tool_result` for an invalid call is created later by the Tool Round (`executeToolRound`, tool-round.ts:48-61) and is R7.2 evidence, not R7.1 evidence. The strict adapter collector instead fails the whole turn on duplicates (bounded-model-turn.ts:256-261) — both behaviors are contract and belong to different call sites.
- **Tool-result boundary** (`detachBoundedToolResult`, bounded-model-turn.ts:101-157): result must be a JSON object; success requires `status:"success"` + `output` + string `summary`; failure statuses are exactly `invalid_input, denied, conflict, failed, cancelled, timed_out, output_limit, sandbox_denied, sandbox_unavailable, workspace_violation, unavailable` (bounded-model-turn.ts:64-76) and require a string `message`; non-JSON, non-serializable, oversize (>maxBytes), wrong shape, unknown status, and invalid success/failure shapes each fail with a distinct message; the retained result is the re-parsed round-trip (unknown fields and caller references dropped; bounded-model-turn.test.ts:202-233).
- **Deterministic fake provider** (adapters): id `deterministic-fake`; default echo "Siralos received: <latest user prompt>"; deterministic 16-code-point text chunks; synthetic scenarios gated on tool availability (`list files` → `workspace.list {path:"."}`, `read README.md` → `workspace.read {path:"README.md"}`, `search <text>` → `workspace.search`, plus git/godot/write/command/plan/develop scenario families); fixed call ids (`call-1`, `call-git`, …); results are read only from items after the last user message (previous-turn results never reused); throws `AbortError` when the signal is already aborted and checks the signal before each chunk and before `completed`; deterministic identical input → identical event stream (deterministic-fake-provider.ts:12-14, 445-542, 878-925, 1371-1406; tests :101-109, 141-178).
- **Detachment**: request arrays are copied (`[...requestMessages]`), inputs and results JSON-detached; host-owned history is never exposed mutable to the provider (provider-turn.ts:86-120; application.ts:142).

### 2.2 Tool loop

| Aspect                | Evidence                                                                                                                                                                                                                                                                                 |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript source     | `packages/core/src/tools/tool.ts`, `tool-registry.ts`, `packages/core/src/application/application.ts` (loop), `provider-turn.ts`, `tool-round.ts`, `application-events.ts`, `packages/adapters/src/tools/workspace/*` (example tools), `apps/cli/src/interactive-session.ts` (rendering) |
| Tests                 | `packages/core/src/application/tool-loop.test.ts`, `provider-protocol.test.ts`, `packages/core/src/tools/tool-registry.test.ts`, adapter tool tests, `apps/cli/src/interactive-session.test.ts`                                                                                          |
| Callers               | `createSiralosApplication` (application.ts:139-226) is the only loop owner; CLI consumes events; planner/reviewer use the same provider-turn/round machinery via their own registries                                                                                                    |
| Observable contract   | §2.2.1                                                                                                                                                                                                                                                                                   |
| Authority owner       | Host owns tool availability, execution authorization, budgets, and transcript; the provider only proposes calls                                                                                                                                                                          |
| Rust owner            | `siralos-core`: registry semantics, round execution, transcript pairing, budgets; `siralos-adapters`: concrete tools (workspace read tools already exist from R4); `siralos-cli`: composition and rendering                                                                              |
| Differential evidence | New `tool-loop` subject (§8)                                                                                                                                                                                                                                                             |

#### 2.2.1 Tool loop contract (exact)

- `ToolDefinition` = `{name, description, inputSchema: JsonObject}` (tool.ts:4-8). `ToolExecutionResult` = discriminated union with statuses `success` (output: JsonValue + summary: string) and message-only `invalid_input | denied | conflict | failed | cancelled | timed_out | output_limit | sandbox_denied | sandbox_unavailable | workspace_violation | unavailable` (tool.ts:20-69).
- **Registry** (tool-registry.ts:17-35): duplicate names throw at construction ("Duplicate tool name: <name>"); lookup is exact and case-sensitive; `definitions()` returns a fresh copy in registration order; registry is immutable after construction. Plain tools default their capability to `workspace.read` (prepared-mutation-tool.ts:59-70).
- **Loop** (application.ts:165-226): single-flight (a second `sendPrompt` while responding throws); `user_message` appended; per iteration: abort → `response_cancelled`; turn cancelled → `response_cancelled`; turn failed → `response_failed`; zero tool calls → terminal (`assistant_message` if text non-empty, `onProviderTurnCompleted`, `response_completed`); tool-round budget checked before executing the round; otherwise execute the round and append assistant text + round transcript, then loop.
- **Tool-round bounds**: `DEFAULT_MAX_TOOL_ROUNDS = 8`, hard ceiling `MAX_TOOL_ROUNDS = 32`, normalized via clamp/floor (application.ts:111-112, 132-137); the round at the cap is not executed: "Siralos reached the maximum of <n> tool rounds; the requested tool round was not executed." (application.ts:200-206).
- **Round execution** (tool-round.ts:30-92): transcript pre-seeded with one `assistant_tool_call` per call in emission order; calls execute sequentially in order; every call receives exactly one `tool_result` — invalid calls get `{status:"failed"}` results without lookup/execution; a cancelled call stops the round and every not-yet-started call receives `{status:"cancelled", message:"The tool call was cancelled before it executed."}`; results are stored in call order; a cancelled round still appends its full transcript, but the turn's assistant text is not appended (application.ts:213-221).
- **Tool lookup and authorization** (application.ts:228-263): `tool_started` (displayInput truncated to 200 chars) is emitted before lookup; unknown tool → `tool_failed` + `{status:"failed"}` result, provider gets a next turn; a tool hidden by the last projection is denied before execution ("Tool <name> is not in the projected tool schema for this session and was denied before execution."); every execution re-evaluates the capability policy (deny → `{status:"denied"}`); plain tools under `ask` with no reviewable preparation are denied; prepared tools (mutation/probe/diagnostic/LSP/command) run the prepare → one-time digest-bound approval → execute protocol; non-ready prepare outcomes are stored with their typed status (application.ts:333-346).
- **Tool argument validation**: the loop does not schema-validate; tools self-validate and return `invalid_input`, which is stored as the result and rendered as `tool_failed` (workspace tools use shared parsers in adapters/src/tools/workspace/validation.ts).
- **Result retention**: round transcripts are appended to host history and resent to the provider on the next turn; projection may trim oldest whole tool-call/result pairs only from the disposable request copy (§5.3), never from authoritative history.
- **Events** (application-events.ts:22-50): `tool_started`, `tool_awaiting_approval`, `tool_completed` (success only), `tool_failed` (every other non-cancelled status plus unknown/hidden/denied paths), `tool_cancelled` (cancelled only), plus `approval_requested`/`approval_resolved`, `checkpoint_applied`, `context_pressure`, and command events.
- **Model proposal vs Host authority**: tools are filtered from the request by policy at application construction (application.ts:147-150) and again by the ToolProjector per mode (visible = available + gated; hidden tools are absent from the schema); execution is independently re-checked (projection-schema guard + per-call permission). A provider can never gain Host authority by requesting a tool.

### 2.3 Projection

| Aspect                | Evidence                                                                                                                                                                                                                                                                                                                                                  |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript source     | `packages/core/src/projection/` — `context-capacity.ts`, `context-estimator.ts`, `context-pressure.ts`, `context-projector.ts`, `conversation-trim.ts`, `evidence-projector.ts`, `projection-service.ts`, `tool-projector.ts`, `watermark-cache.ts`, `stale-result.ts`; `packages/core/src/context/` (`projection.ts` phase mapping, `phase-contract.ts`) |
| Tests                 | `projection.test.ts`, `reference-research-projection.test.ts`, `tests/behavior/projection.behavior.test.ts`, `packages/core/src/context/context.test.ts`, `effect-tests.test.ts`                                                                                                                                                                          |
| Callers               | `collectProviderTurn` (provider-turn.ts:89-114) is the sole turn-time consumer; `/context` and `/tools` render `lastProjection()` (output/context.ts); planner/reviewer construct their own projectors                                                                                                                                                    |
| Observable contract   | §2.3.1 and §5.3                                                                                                                                                                                                                                                                                                                                           |
| Authority owner       | Host owns context semantics; projection is derived, disposable context that never grants capability                                                                                                                                                                                                                                                       |
| Rust owner            | `siralos-core`: estimator, pressure, segment model, deterministic sort/serialize, trim, evidence normalization, tool visibility projection                                                                                                                                                                                                                |
| Differential evidence | New `context-projection` subject (§8)                                                                                                                                                                                                                                                                                                                     |

#### 2.3.1 Projection contract (exact)

- **Pipeline per turn**: project → estimate → classify pressure → fit (trim) or block → provider (projection-service.ts:66-78). The projected request IS the provider request (provider-turn.ts:111-120).
- **Capacity** (context-capacity.ts): `workingMaximum` is the authority (`DEFAULT_CONTEXT_WORKING_MAXIMUM = 32_768` tokens); `advertisedMaximum`/`verifiedMaximum` are `null` today and never used in decisions; `DEFAULT_CONTEXT_MAX_OUTPUT_TOKENS = 4_096` carried but unused in math.
- **Estimator** (context-estimator.ts): `estimateTokens = Math.ceil(utf8Bytes / 4)`, empty → 0; `estimateConversationItemTokens` sums UTF-8 bytes of `content/summary/message/toolName/callId` plus JSON of `input`, object `output`, and `result.summary/result.message`; non-serializable payloads contribute 0, never throw. Deterministic.
- **Pressure** (context-pressure.ts): ratios `warn 0.7 / auto 0.85 / hard 1.0` of the working maximum, thresholds inclusive (≥); `workingMaximum ≤ 0` → ratio 1 and hard for any non-negative estimate. `normal` → send unchanged; `warn` → send + `context_pressure` event; `auto`/`hard` → deterministic reduction (message budget = workingMaximum − system tokens − tool tokens, then `trimConversationPreservingPairs`); if still `hard` after reduction the provider call is blocked (`blocked = {type:"hard", reason:"Projected context is N tokens against a working maximum of M; the provider call was blocked.[ (reduction was already applied)]"}`, projection-service.ts:763-769) and `response_failed` is emitted.
- **Trim** (conversation-trim.ts:38-92): drops oldest whole tool-call/result pairs until the budget fits; `user_message` items always survive; `assistant_message` survives while it fits; structurally invalid transcripts fail closed (returned unchanged, never reduced); the trimmed transcript stays valid by construction.
- **Context projector** (context-projector.ts): segments sorted by stability (`stable 0 / contextual 1 / volatile 2`) then id (code-unit order); serialized as `[Title]\ncontent` joined by `\n\n`; system prefix = stable + contextual; `stableFingerprint = sha256(canonicalizeJson({id,title,content}))` over stable segments only — volatile/contextual changes never perturb it (prompt-cache identity); `SIRALOS_SYSTEM_INSTRUCTIONS` is the stable anchor (context-projector.ts:130-146).
- **Segment composition order** (projection-service.ts:636-663): siralos-core-instructions (stable) → project-instructions (contextual) → pinned/retrieved knowledge (contextual) → task-contract/task-state (contextual) → task-plan (contextual, ≤ 4 KiB, current revision only) → executor-brief (contextual, ≤ 4 KiB) → current-task-evidence (volatile) → reference/research/scene evidence (contextual, 4 most recent each, combined 12 KiB with deterministic reduction order research → scene → reference, truncation marker `… [truncated]`). Empty inputs: system is never empty when projection is wired (stable segment always present); evidence sections omitted when empty.
- **Tool projection** (tool-projector.ts): visibility = `available | gated | hidden` from mode allowlists (exact tool names per mode; `generic` uses the capability allowlist) ∩ permission (deny → hidden, ask → gated, allow → available); request tools = visible only; ABI `fingerprint = sha256(canonicalizeJson(visible {name,description,inputSchema}))` — hidden tools never change it; tool-requiring modes (`development/review/inspection/planning`) with a non-tool-calling route block with `{type:"unsupported"}` (no silent fallback).
- **Evidence projection** (evidence-projector.ts): bounded sanitization of tool-result summaries/messages for the model — strip ANSI/control, redact secrets, bound lines, truncate; `DEFAULT_EVIDENCE_MAX_TOTAL_BYTES = 32 KiB`, `DEFAULT_EVIDENCE_MAX_LINE_BYTES = 1 KiB`; deterministic transform order with a "never worse" rule (a size-reducing transform that would inflate is reverted); truncation always applies last and shrinks; explicit `… [truncated]` disclosure. Raw history/evidence is never mutated (disposable model view).
- **Observability**: `lastProjection()` feeds `/context` (stable/contextual/volatile bytes, estimated tokens / working maximum, pressure + ratio %) and `/tools` (ABI counts) (output/context.ts:150-184).

### 2.4 Configuration

| Aspect                | Evidence                                                                                                                                                                                                                                                                                                                                                  |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript source     | `packages/adapters/src/config/user-config.ts`, `config-diagnostics.ts`; consumers `apps/cli/src/bootstrap/create-application.ts`, `sandbox-doctor.ts`, `doctor.ts`; schema `schemas/user-config.schema.json`; self-reference summary `packages/core/src/self/config-schema-summary.ts`                                                                    |
| Tests                 | `packages/adapters/src/config/user-config.test.ts`, `config-schema-summary.test.ts`, behavior/effect tests (self-reference-doctor.behavior.test.ts)                                                                                                                                                                                                       |
| Callers               | `createCliApplication` (create-application.ts:214-216), sandbox doctor, `/doctor configuration`, `/references`                                                                                                                                                                                                                                            |
| Observable contract   | §2.4.1                                                                                                                                                                                                                                                                                                                                                    |
| Authority owner       | User-level trusted input; Host-owned strict validation; config can never broaden policy                                                                                                                                                                                                                                                                   |
| Rust owner            | `siralos-adapters`: external config discovery/loading, bounded read, symlink/filesystem policy, and parsing of the current external config format (including its strict format-specific validation); `siralos-core`: none (no file-format-independent config semantics exist today); `siralos-cli`: path/override composition and user-facing diagnostics |
| Differential evidence | New `user-config` subject (§8)                                                                                                                                                                                                                                                                                                                            |

#### 2.4.1 Configuration contract (exact)

- **Location**: exactly one file, `join(os.homedir(), ".siralos", "config.json")` (user-config.ts:83-85); no search chain, no per-project discovery. Callers may override the path (create-application.ts:214).
- **Absence**: `ENOENT` → `DEFAULT_USER_CONFIG` (a success, never an error, nothing created). Non-regular file or symlink → hard error ("Siralos configuration at <p> is not a regular file.").
- **Bounds/encoding**: `MAX_CONFIG_FILE_BYTES = 1 MiB`; declared size pre-check, then a bounded EOF-verified complete read (readFileBounded); UTF-8 decode; JSON parse errors reported as "Siralos configuration at <p> is not valid JSON: …".
- **Schema** (strict, unknown keys rejected everywhere — a credential can never hide): top-level exactly `sandbox | godot | quality | references`; `sandbox.profile` ∈ `inspect` (default) | `develop-offline`; `sandbox.backend` ∈ `auto` (default) | `anthropic-runtime` — **validated but never consumed** (the backend is always the pinned Anthropic Sandbox Runtime); `godot` (activeInstallation, installations ≤ 16 with absolute paths only, discoverOnPath default true, edition hints) — **Godot config scope classification (R7A):** A) generic envelope parsing is R7 (the section exists in the current format and its presence/shape is observable); B) generic structural validation required for current CLI/config behavior is R7 (unknown-key rejection, id length, absolute-path rule, edition-hint enum, installation count); C) Godot-specific _semantics_ are deferred to R8/R9 (installation selection, PATH discovery, edition classification, engine profiles — none of which R7 implements); `quality.reviewProvider` (identifier ≤ 128 chars, must match a registered provider — only `deterministic-fake` exists); `references` (alias → declaration, ≤ 16, kind local-directory|repository, unknown keys rejected at every level, semantic validation deferred to core and non-fatal: a parse failure keeps the registry empty and surfaces via `/references`, never crashes startup).
- **Defaults** are per-key on absence only; a present-but-invalid value is a hard error, never a silent default.
- **Provider/model selection: not configurable.** The active provider is always `deterministic-fake`; there is no model key, no credentials, no endpoints (create-application.ts:593; doctor reports model id null).
- **Environment**: the config loader reads no environment variables; `SIRALOS_CONFIG` exists only on the child-environment deny list; Godot overrides (`SIRALOS_GODOT`, `SIRALOS_GODOT_INSTALLATION`) are separate from config.
- **Diagnostics** (config-diagnostics.ts:22-68): fixed section order `sandbox, godot, quality, references`; `loadUserConfig` is the single validator; `unknownFields` and `credentialRefs` are always `[]`, `overrideInUse` always false; file state `readable | missing | unreadable` (lstat, symlinks unreadable). Doctor: `configuration.validity` fail on any validation error with one detail per error, remediation "Fix the configuration file"; credential check skips with "No credential environment variables are referenced…". Validation messages name fields, never values.
- **Deterministic representation**: `CONFIG_SCHEMA_REVISION` digest over the documented config summary, drift-protected against the JSON schema by tests.
- **Authority**: config selects among two built-in sandbox profiles and cannot craft profiles, cannot set capability rules, cannot broaden policy, cannot change the backend, cannot add executables, and cannot store secrets. "Lower-authority configuration may narrow behavior. It may never broaden Host authority" (ADR 0036 §6).
- **Distinction from the future**: the current format is one user-level JSON file; `siralos.toml`, Profiles files, `siralos.lock`, Skill/Plugin selection are ADR-0036 target concepts and are not implemented; R7 ports current behavior only.

### 2.5 CLI

| Aspect                | Evidence                                                                                                                                                                                                                                                                                |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TypeScript source     | `apps/cli/src/index.ts` (entrypoint), `interactive-session.ts`, `input/parse-input.ts`, `input/input-queue.ts`, `output.ts` + `output/` (sanitize, context, system, …), `session/` (command implementations), `bootstrap/` (composition root, doctors), `approval/approval-reviewer.ts` |
| Tests                 | `interactive-session.test.ts`, `parse-input.test.ts`, `input-queue.test.ts`, `output.test.ts`, `bootstrap/create-application.test.ts`, doctor tests                                                                                                                                     |
| Callers               | The `siralos` bin (package.json `bin` → `dist/index.js`); all terminal behavior                                                                                                                                                                                                         |
| Observable contract   | §2.5.1                                                                                                                                                                                                                                                                                  |
| Authority owner       | CLI owns composition, terminal I/O, and the interactive session boundary; it owns no conversation state, provider behavior, or policy                                                                                                                                                   |
| Rust owner            | `siralos-cli`: composition, session loop, input queue, output rendering/sanitization; `siralos-adapters`: none new                                                                                                                                                                      |
| Differential evidence | New `cli-session` subject (§8)                                                                                                                                                                                                                                                          |

#### 2.5.1 CLI contract (exact)

- **Entrypoint** (index.ts): no `--help`/`--version` flags exist in the TypeScript oracle — unknown flags silently launch the interactive session. Flags: `--sandbox-doctor [--run-probes]`, `--godot-doctor [--godot-path <p>] [--godot-installation <id>] [--recovery-probe]`, `--doctor`/`--self` (with `--json`, `--report-safe`, area args). Startup fatal errors print `Siralos failed to start: <message>` to stderr with exit 1; session exit codes: interactive session 0 (EOF or `/exit`), sandbox-doctor 0/1/3, godot-doctor 0–7, doctor 0/1/2. (The Rust binary's own `--version`/`--help` are Rust-side conveniences; their values come from R2's `version-identity` subject, not from oracle flags.)
- **Startup**: config load → built-in profile → default policy → workspace root resolution → backend → security facade → tools → application (create-application.ts:211-237); config errors are fatal; reference-config semantic errors are non-fatal fail-closed.
- **Header**: `Siralos\nInteractive Godot development harness\nProvider: <providerId>\n` (output.ts:105-110).
- **Session loop** (interactive-session.ts:100-333): prompt `"> "`; input trimmed; empty input ignored; case-sensitive slash parsing against the core-owned `COMMAND_CATALOG_IDS`; invalid command → `formatInvalidCommand`; `/help` prints the fixed help text (output.ts:112-156); `/exit` returns 0; EOF returns 0; Ctrl+C with an active prompt aborts it and writes `[cancelled; Siralos stays active]`, otherwise closes readline (index.ts:60-67).
- **Prompt flow** (interactive-session.ts:405-523): `response_started` → blank line; `text_delta` printed verbatim (through the sanitizer); `response_cancelled` → `[response cancelled]`; `response_failed` → `formatProviderFailure`; tool events rendered with `formatToolStarted/ToolCompleted/ToolFailed/ToolCancelled` (display input/summary/message sanitized), approval lines, context-pressure line (`⚠ context pressure <state>: <tokens> est. tokens / <max> working`); `process.run` events are rendered by the command renderer with busy-input handling; successful/failed tool activity feeds task observation fingerprints.
- **Input queue** (input-queue.ts): single terminal-read owner; askers register queue entries; timeout/abort/discard settle the asker while the typed line reroutes to the next live entry or buffers; EOF resolves every pending entry with null (a denial path); `/cancel` aborts via the session AbortController (session-development-commands.ts).
- **Terminal sanitizer** (output/sanitize.ts): the single output boundary — every byte to the terminal passes through it; neutralizes C0/C1 controls (caret notation), ESC/CSI/OSC sequences (including OSC 8 links, titles, clipboard), CR/BS rewriting, DEL; newlines and tabs survive; cross-chunk escape sequences tracked; dangling sequences dropped on flush; lone/high surrogates handled; `sanitizePathForDisplay` escapes path metacharacters (`\\`, `\n`, `\r`, `\t`, controls) so untrusted paths cannot spoof lines.
- **Determinism**: no color codes; output is deterministic given identical inputs except wall-clock/timing fields (durations, timestamps) and host paths; ANSI clear (`\x1b[2J\x1b[H`) is presentation-only.
- **Differential comparison stance**: compare exit codes, stable literals, normalized numeric/path/digest values, and `--report-safe --json` doctor output; treat ANSI, absolute paths, timestamps, and durations as presentation-only.

## 3. Required vs structural behavior

### MUST PORT (observable contract)

- Provider port types, turn-collection state machine (bounds, completion, cancellation precedence, EOF, event-after-completion, duplicate/empty call ids, JSON detach), tool-result detach boundary, transcript validation, deterministic fake provider behavior, application loop (rounds, terminal condition, transcript ordering, one-call-one-result), registry semantics (duplicate rejection, order, case-sensitive lookup), capability filtering + projection-schema enforcement, per-turn/round budgets, token estimator, pressure classification and thresholds, trim rules, context segment sort/serialize, stable fingerprint, evidence normalization (never-worse + truncation marker), tool visibility projection, config path/bounds/strict-schema/diagnostics/defaults, CLI prompt/parse/EOF/interrupt/exit-code/sanitizer behavior.

### MAY REDESIGN (structural freedom, same observable behavior)

- The strict adapter collector (`collectBoundedModelTurn`) and the application collector (`collectProviderTurn`) are two TypeScript call sites with subtly different duplicate-id behavior; Rust may expose one bounded-turn core with an explicit option for "invalid-call" vs "fail-turn" handling.
- `ProjectionService`'s object composition (three projectors + caches) can flatten into fewer Rust types as long as `ProjectedRequest`-equivalent output, pressure decisions, and event ordering are preserved.
- `WatermarkCache` internals: port the invariant (high/low watermarks 64/32, oldest-first eviction), not the JS Map mechanics.
- Prepared-tool/approval machinery: only the observable protocol (prepare → one-time digest-bound approval → execute; denial/EOF/failure/timeout/cancel prevent execution; non-ready outcomes stored with typed status) is contract; the adapter plumbing is free.
- Terminal rendering: ANSI-free rendering is contract; exact layout/formatting strings are only contract where the tests pin them (help text, status lines, header).

### DO NOT PORT (structural debt)

- `deepFreeze`/defensive-copy machinery — Rust ownership replaces it; the observable invariant (caller-owned mutable data is never retained) is what must hold.
- Stringly error plumbing where a typed Rust error preserves the exact external message.
- Single-implementation interfaces/pass-through wrappers around the loop (e.g. `ProviderTurnContext`, adapter interfaces) unless a real seam exists.
- `context/artifacts.ts`, `context/provenance.ts`, `context/staleness.ts`, `context/source-integrity.ts` (ADR 0030 interpretability bookkeeping) — see §12.
- Node stream/iterator machinery (`AsyncIterable`, `nextProviderEvent` racing) — replace with pull-based collection in Rust; async only where behavior requires it.

### LATER MILESTONE (explicitly not R7)

- All `godot.*` tools and Godot-specific config semantics (installation selection, PATH discovery, edition classification, engine profiles) — R8/R9; the generic configuration envelope and its structural validation remain R7 (§2.4.1). Also not R7: real providers, provider credentials, model routing (R11/ADR 0036 Profile); mutation application, process execution, Git execution (R11 — fail-closed unavailable surfaces already exist in both implementations and only need typed parity, which R4 started); references/research/self-reference tools as effect surfaces; the canonical Context compiler (R10).

## 4. Failure taxonomy

Machine-branchable R7 failure inventory (source → typed representation → external message → retryable/terminal → transcript effect → session usable). F = failure class; all messages are exact external strings.

| #   | Failure                                                                                                                          | Source            | Typed/structured                     | External message                                                                                                                                                                         | Retry                      | Terminal               | Transcript                                               | Session      |
| --- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------- | ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- | ---------------------- | -------------------------------------------------------- | ------------ |
| F1  | Turn limit exceeded (text bytes / text events / tool calls / call-id bytes / tool-name bytes / tool-arg bytes / aggregate bytes) | provider stream   | `failed` + message naming the limit  | "The provider exceeded <what> limit; the response was rejected."                                                                                                                         | yes (new prompt)           | no                     | nothing committed                                        | yes          |
| F2  | EOF without `completed`                                                                                                          | provider stream   | `failed`                             | "The provider stream ended without a completion event; the response was rejected."                                                                                                       | yes                        | no                     | none                                                     | yes          |
| F3  | Event after `completed`                                                                                                          | provider stream   | `failed`                             | "The provider exceeded an event after completion limit; the response was rejected."                                                                                                      | yes                        | no                     | none                                                     | yes          |
| F4  | Provider throw (non-cancellation)                                                                                                | provider stream   | `failed` + error message             | `describeError` text                                                                                                                                                                     | yes                        | no                     | none                                                     | yes          |
| F5  | Non-JSON-serializable tool argument                                                                                              | provider stream   | `failed`                             | "…exceeded the tool-argument JSON validity limit…"                                                                                                                                       | yes                        | no                     | none                                                     | yes          |
| F6  | Empty call id/name                                                                                                               | provider stream   | `invalid` TurnToolCall (not failure) | "Provider emitted a tool call with an empty call id or tool name."                                                                                                                       | n/a                        | no                     | no tool_result at this layer; Tool Round (R7.2) pairs it | yes          |
| F7  | Duplicate call id (application)                                                                                                  | provider stream   | `invalid` TurnToolCall               | "Duplicate tool call id: <id>."                                                                                                                                                          | n/a                        | no                     | no tool_result at this layer; Tool Round (R7.2) pairs it | yes          |
| F8  | Duplicate call id (strict adapter)                                                                                               | provider stream   | `failed`                             | "<actor> emitted duplicate tool call id <id>."                                                                                                                                           | yes                        | no                     | none                                                     | yes          |
| F9  | Transcript structurally invalid                                                                                                  | host history      | `failed` (blocked request)           | "The conversation transcript is structurally invalid; the provider request was blocked: …"                                                                                               | no (host state)            | yes for the prompt     | none                                                     | yes          |
| F10 | Tool-result detach: non-JSON / oversize / wrong shape / unknown status / invalid success / invalid failure                       | host tool adapter | `ok:false` + message                 | "<actor> returned …" (per rule)                                                                                                                                                          | yes                        | no                     | none (result rejected)                                   | yes          |
| F11 | Unknown tool                                                                                                                     | host lookup       | `{status:"failed"}` result           | "Unknown tool: <name>."                                                                                                                                                                  | yes (provider can recover) | no                     | failed result stored                                     | yes          |
| F12 | Tool hidden by projection                                                                                                        | host projection   | `{status:"denied"}` result           | "Tool <name> is not in the projected tool schema for this session and was denied before execution."                                                                                      | yes                        | no                     | denied result stored                                     | yes          |
| F13 | Capability denied                                                                                                                | host policy       | `{status:"denied"}` result           | "Capability <cap> is denied by policy: <reason>"                                                                                                                                         | yes                        | no                     | denied result stored                                     | yes          |
| F14 | Tool self-validation failure                                                                                                     | tool              | `{status:"invalid_input"}` result    | tool message                                                                                                                                                                             | yes                        | no                     | result stored, no execution                              | yes          |
| F15 | Tool throw (non-cancellation)                                                                                                    | tool              | `{status:"failed"}` result           | error message                                                                                                                                                                            | yes                        | no                     | result stored                                            | yes          |
| F16 | Tool cancellation                                                                                                                | tool / signal     | `{status:"cancelled"}` result        | "Tool execution was cancelled." / "The tool call was cancelled before it executed."                                                                                                      | yes                        | no                     | cancelled result(s) stored, round ends                   | yes          |
| F17 | Approval denial / reviewer failure / no reviewer                                                                                 | approval protocol | `{status:"denied"}` result           | adapter denied message or "No approval reviewer is available."                                                                                                                           | yes (new request)          | no                     | denied result stored                                     | yes          |
| F18 | Max tool rounds reached                                                                                                          | host budget       | `response_failed`                    | "Siralos reached the maximum of <n> tool rounds; the requested tool round was not executed."                                                                                             | yes                        | yes for the prompt     | user_message + prior rounds only                         | yes          |
| F19 | Hard context pressure (after reduction)                                                                                          | host projection   | `response_failed` + blocked reason   | "Projected context is N tokens against a working maximum of M; the provider call was blocked.[ (reduction was already applied)]"                                                         | yes                        | yes for the prompt     | none                                                     | yes          |
| F20 | Tool-calling unsupported for tool-requiring mode                                                                                 | host projection   | `response_failed` + blocked reason   | "The selected provider route does not support tool calling, which this task requires; …"                                                                                                 | no (route)                 | yes for the prompt     | none                                                     | yes          |
| F21 | Concurrent prompt                                                                                                                | application       | thrown `Error`                       | "Siralos is already responding to a prompt."                                                                                                                                             | yes                        | n/a                    | none                                                     | yes          |
| F22 | Config: missing file                                                                                                             | config loader     | defaults                             | (none — absence is success)                                                                                                                                                              | n/a                        | n/a                    | n/a                                                      | yes          |
| F23 | Config: unreadable/oversize/symlink                                                                                              | config loader     | thrown `Error`                       | "Cannot read Siralos configuration at <p>: …" / "…exceeds the <n>-byte limit." / "…is not a regular file."                                                                               | yes                        | yes at startup (fatal) | n/a                                                      | no (startup) |
| F24 | Config: invalid JSON / unknown key / invalid value                                                                               | config loader     | thrown `Error`                       | exact per-rule messages (§2.4.1)                                                                                                                                                         | yes                        | yes at startup (fatal) | n/a                                                      | no (startup) |
| F25 | CLI: startup failure                                                                                                             | CLI               | stderr + exit 1                      | "Siralos failed to start: <message>"                                                                                                                                                     | yes                        | yes                    | n/a                                                      | no           |
| F26 | CLI: non-fatal command/doctor error                                                                                              | CLI               | rendered error                       | `formatProviderFailure` etc.                                                                                                                                                             | yes                        | no                     | n/a                                                      | yes          |
| F27 | CLI: EOF / interrupt                                                                                                             | CLI               | exit 0 / live session                | —                                                                                                                                                                                        | n/a                        | yes                    | n/a                                                      | n/a          |
| F28 | Unknown provider event discriminator (any non-validated `type`, including tool-call-shaped)                                      | provider stream   | `failed` (protocol)                  | "The provider emitted an unknown event type; the response was rejected." (app) / "<actor> emitted an unknown event type." (strict)                                                       | yes                        | no                     | nothing committed                                        | yes          |
| F29 | Malformed provider event (non-object / array / non-string `text` / non-string `callId` or `toolName`)                            | provider stream   | `failed` (protocol)                  | "The provider emitted a malformed event / a text event without a string payload / a tool call with a non-string id or name; the response was rejected." (app; actor-qualified in strict) | yes                        | no                     | nothing committed                                        | yes          |

Retryability follows the existing typed-failure discipline (RUST_MIGRATION.md):
typed Host-observed failures stay distinguishable so recovery never depends on
substring matching. F1-F5, F8, F10, F11, F13-F17, F28, F29 are retryable
turn/tool failures; F9, F18-F20, F25 are terminal for the prompt/startup;
F23/F24 are terminal at startup; F22 is not a failure. Every runtime failure
leaves the session usable; only startup failures prevent it. Stringly
TypeScript errors that should become typed Rust errors without changing
observable behavior: turn limit classes (F1), protocol violations
(F2/F3/F5/F8/F28/F29), transcript invalidity (F9), tool-result detach classes
(F10), and config rule violations (F23/F24).

## 5. Resource bounds

Limits are classified explicitly (R7A correction — not all are byte bounds):

- **BYTE BOUNDS** (UTF-8 bytes, not characters; boundary inclusive — a value
  exactly at the limit is accepted, `>` triggers rejection): assistant text
  bytes, call-id bytes, tool-name bytes, tool-argument bytes, aggregate turn
  bytes, config file bytes, evidence/segment byte budgets.
- **COUNT BOUNDS** (number of events/items; boundary inclusive): text event
  count, tool-call count per turn, tool-round count, reference count,
  installation count, evidence-view count, watermark sizes.
- **LENGTH/DOMAIN-SPECIFIC BOUNDS** (explicitly defined values): character
  lengths (display input 200, reviewProvider 128), token estimates (working
  maximum 32,768 / output 4,096 — estimates, not byte accounting), installation
  id length, focus-path count.

### 5.1 Provider turn (per turn, application — PROVIDER_TURN_LIMITS, provider-turn.ts:50-65)

| Dimension                                        | Limit             | Violation message fragment      |
| ------------------------------------------------ | ----------------- | ------------------------------- |
| Assistant text bytes (cumulative)                | 64 KiB (65,536)   | "the assistant-text byte limit" |
| text_delta events                                | 4,096             | "the text-event count"          |
| Tool calls per turn                              | 32                | "the tool-call count"           |
| Call-id bytes (per call)                         | 256               | "the tool-call id byte limit"   |
| Tool-name bytes (per call)                       | 256               | "the tool-name byte limit"      |
| Tool-argument bytes (per call, serialized)       | 128 KiB (131,072) | "the tool-argument byte limit"  |
| Aggregate turn bytes (text + ids + names + args) | 256 KiB (262,144) | "the aggregate turn byte limit" |

Of the seven turn dimensions, five are byte bounds (assistant text,
call id, tool name, tool argument, aggregate turn) and two are count bounds
(text events, tool calls). The strict adapter collector accepts the same seven
dimensions caller-supplied and normalizes numeric budgets via clamp/floor with
fallback (bounded-model-turn.ts:82-93); "callers may choose a smaller
assistant-text limit, but every other dimension remains explicitly bounded".

### 5.2 Loop budgets

- Tool rounds: default 8, hard max 32, clamp [0,32] — **count bound** (application.ts:111-112, 132-137).
- Display input truncation: 200 chars + "…" — **length bound** (application.ts:130, 1027-1035).

### 5.3 Projection

- Working maximum 32,768 tokens; output tokens 4,096 — **token estimates**, not byte accounting (carried, unused in math).
- Pressure ratios 0.7 / 0.85 / 1.0, inclusive thresholds.
- Evidence: 32 KiB total, 1 KiB per line, truncation marker "
  … [truncated]".
- Plan segment ≤ 4 KiB; executor-brief segment ≤ 4 KiB; reference+research+scene evidence combined ≤ 12 KiB, 4 views each, per-view research 4 KiB.
- Evidence cache watermarks 64/32; task focus paths ≤ 8.
- Trim: whole tool-call/result pairs only; user messages always survive.

### 5.4 Configuration

- File ≤ 1 MiB — **byte bound** (declared-size pre-check + bounded EOF-verified read); references ≤ 16 and godot installations ≤ 16 — **count bounds**; installation id ≤ `GODOT_LIMITS.maxInstallationIdLength` and reviewProvider ≤ 128 chars — **length bounds**.

### 5.5 Tool-result detach boundary

- Result JSON ≤ caller-supplied `maxBytes` (adapter); the application path applies no separate result bound at storage (bounds apply to provider output and to display).

### 5.6 Boundary semantics (pinned by tests)

- Exactly-at-limit accepted (provider-protocol.test.ts:307-318); cumulative across deltas (…:275-305); multi-byte characters counted byte-wise (…:266-273; bounded-model-turn.test.ts:83-94). Partial text never survives a failed turn; partial tool calls never execute; cancellation outranks provider completion; iterator EOF is not completion; after an explicit `completed` any further event fails the turn.

## 6. Determinism

### OBSERVABLE CONTRACT (encode explicitly in Rust)

- Tool definitions: registration order preserved end-to-end (registry → policy filter → projection → request); no sorting anywhere in the tool path.
- Tool calls/results: emission order; sequential execution; results in call order.
- Provider events: consumed in stream order; text deltas forwarded in order.
- Transcript: ordered array; validation pairing rules; round transcripts appended after assistant text.
- Context segments: stability rank (0/1/2) then id code-unit order (context-projector.ts:86-97).
- Estimator: `ceil(utf8Bytes/4)` — Rust `(bytes+3)/4` with `str::len` matches.
- Fingerprints: `sha256(canonicalizeJson(…))` with sorted object keys — Rust must reproduce JS `JSON.stringify` escaping exactly for parity (real parity risk; the R2 harness already proved canonical-JSON equality).
- Pressure classification and trim drop order (oldest-first whole pairs).
- Config diagnostics: fixed section order; single-validator errors; deterministic messages.
- Fake provider: identical inputs → identical event streams; 16-codepoint chunks; scenario precedence order (planning → develop → godot → git → write → command → generic → echo).

### SOURCE-LANGUAGE ACCIDENT (do not port)

- Object key order in `JSON.stringify(input)` for token _estimation_ (context-estimator.ts:55,63) — an internal estimate with headroom; may be normalized.
- Map/Set iteration details of `WatermarkCache` (port the invariant).
- `snapshot.evidence[length-1]` "latest" and `slice(-4)` most-recent selection are deterministic by array position — reproduce as "most recent", not by timestamp.
- Float ratio display (`Math.round(ratio*100)`); state decisions use integer comparisons.
- Promise/stream timing (chunk interleaving with `await Promise.resolve()`) — no observable ordering requirement beyond event order.

## 7. Cancellation

End-to-end path: caller signal → `sendPrompt` → `collectProviderTurn` → provider `ModelRequest.signal` → tool round → every tool execution → CLI input queue/approval reviewer.

- **Who owns cancellation**: the caller (CLI session controller for prompts; `/cancel`; Ctrl+C aborts the active prompt; input-queue timeouts for approvals).
- **When checked**: before each provider event read (nextProviderEvent races the abort against `iterator.next()`, provider-turn.ts:267-308), before each tool call in a round (tool-round.ts:43-47), and inside cooperative tools; a provider that ignores its signal cannot hold the application open — the abandoned `next()` is handled and never consumed; the iterator is best-effort closed via `return()`.
- **Representation**: `AbortSignal` + `AbortError`-named errors (`isCancellationError`, domain/cancellation.ts); tool results use status `cancelled`.
- **Precedence**: cancellation outranks provider completion and terminal conditions.
- **Scope**: cancellation ends the whole attempt (prompt) — `response_cancelled`; prior transcript (user message + completed rounds) is retained; the session remains usable.
- **Partial output**: displayed deltas may have been streamed, but nothing of the cancelled turn is committed to history except the round transcript of a cancelled round (with cancelled results for not-yet-started calls).
- **Terminal behavior**: CLI writes `[response cancelled]` / `[cancelled; Siralos stays active]`; exit codes unchanged; EOF resolves pending approval reads as denial (null).
- **Rust note**: async is only needed where behavior requires it; the deterministic in-process fake provider admits a synchronous pull-based iterator with explicit cancellation checks between events, preserving observable semantics (the harness never needs real concurrency).

## 8. Differential plan

No corpus changes are made in this extraction task (the R7 Rust subjects do
not exist yet; required scenarios would leave the gate red). The plan below is
the expansion to implement with R7.1+; all subjects follow corpus schema 3,
protocol schema 1, and the existing manifest/digest mechanics
(`regenerate-corpus-manifest.mjs` + bumping `CORPUS_VERSION` in
`tests/differential/shared/contract.mjs` and
`crates/siralos-cli/src/harness.rs` in the same change that adds the
scenarios). Expected-output fixtures are never invented — the harness compares
the two implementations' canonical outcome records.

### Current corpus inventory (corpus version 11, schema 3, 86 scenario files)

| Subject                                                                                     | Scenarios         | Status                                                                       |
| ------------------------------------------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------- |
| state-dir                                                                                   | 6                 | R2 (2 required-skipped per host platform; `state-dir.unset.*` informational) |
| version-identity                                                                            | 1                 | R2                                                                           |
| task-contract                                                                               | 17                | R3                                                                           |
| workspace-read / workspace-list / workspace-search / workspace-revision / workspace-prepare | 6 / 2 / 3 / 3 / 2 | R4                                                                           |
| checkpoint / git-inspection                                                                 | 5 / 2             | R4                                                                           |
| language-diagnostics / language-structure / language-definition                             | 8 / 4 / 4         | R5                                                                           |
| domain-lifecycle / domain-capability                                                        | 19 / 4            | R6                                                                           |
| provider-turn / tool-loop / context-projection / user-config / cli-session                  | planned           | R7 (this plan)                                                               |

Current-host audit shape (windows, `parityHeld: true`): 86 total, 83
applicable, 83 required, 82 required-applicable matched, 0 deviations; the
three posix-only `state-dir` scenarios are explicit
`UNSUPPORTED:PLATFORM_NOT_APPLICABLE` records, and `state-dir.unset.windows`
is informational. R7 subjects are all `["*"]` platform with empty `env` and a
controlled `input` (new `providerInputBytes`-style bound per subject in
`CONTRACT_LIMITS`), so they are applicable and required on every host, exactly
like `task-contract`/language/domain subjects. Intentional unavailability is
still a gated parity surface in this corpus (e.g. `workspace-prepare.unavailable`
and `git-inspection.unavailable` are `required` and must be matched typed, not
left `UNIMPLEMENTED`) — R7 inherits the same discipline for any unavailable
surface it exposes.

### Subject naming

Consistent with existing kebab-case subject ids: `provider-turn`,
`tool-loop`, `context-projection`, `user-config`, `cli-session`.

### 8.1 Scenario plan

| Scenario id                                                               | Subject            | Input (controlled)                                                  | Observable outcome                                                                                                  | Parity   | Platform | Boundary covered                             |
| ------------------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | -------- | -------- | -------------------------------------------- |
| provider-turn.basic                                                       | provider-turn      | conversation items, tool definitions, limits, fake-provider prompt  | COMPLETED record: event sequence, assistant text, tool calls, completion                                            | required | *        | request echo, chunking, completion semantics |
| provider-turn.text-bytes-boundary                                         | provider-turn      | prompt engineered to produce exactly/over 64 KiB text               | failed-turn record at the exact boundary                                                                            | required | *        | inclusive byte boundary, UTF-8 accounting    |
| provider-turn.multibyte                                                   | provider-turn      | multi-byte text payload                                             | byte-accurate accounting outcome                                                                                    | required | *        | UTF-8 rather than char count                 |
| provider-turn.text-events                                                 | provider-turn      | > 4096 deltas                                                       | failed-turn record                                                                                                  | required | *        | text-event bound                             |
| provider-turn.tool-call-bounds                                            | provider-turn      | > 32 calls, oversized ids/names/args                                | failed-turn records per dimension                                                                                   | required | *        | per-dimension tool bounds                    |
| provider-turn.aggregate                                                   | provider-turn      | mixed text+args over 256 KiB                                        | failed-turn record                                                                                                  | required | *        | aggregate bound                              |
| provider-turn.duplicate-call-id                                           | provider-turn      | duplicate call id in one turn                                       | first occurrence executable; later duplicate → deterministic invalid TurnToolCall; **no tool_result at this layer** | required | *        | duplicate id handling (R7.1)                 |
| provider-turn.empty-id-name                                               | provider-turn      | empty call id / tool name                                           | invalid TurnToolCall records only — **no Tool Round output asserted**                                               | required | *        | empty-id/name handling (R7.1)                |
| provider-turn.event-after-completed                                       | provider-turn      | event after `completed`                                             | failed-turn record                                                                                                  | required | *        | post-completion rejection                    |
| provider-turn.eof-no-completion                                           | provider-turn      | stream ends without `completed`                                     | failed-turn record                                                                                                  | required | *        | EOF ≠ completion                             |
| provider-turn.cancelled                                                   | provider-turn      | abort mid-stream                                                    | aborted record, no completion                                                                                       | required | *        | cancellation precedence                      |
| provider-turn.unknown-event                                               | provider-turn      | unknown discriminator with valid-looking callId/toolName/input      | failed turn, no Tool proposal retained, no successful turn, deterministic message                                   | required | *        | runtime discriminator contract (F28)         |
| provider-turn.malformed-known-variant                                     | provider-turn      | non-object event / non-string text / non-string callId or toolName  | failed turn, deterministic message, no TypeError/coercion                                                           | required | *        | malformed known variant contract (F29)       |
| provider-turn.result-detach.success/failure/invalid/oversize/status/shape | provider-turn      | success/failure/invalid/oversize/unknown-status/wrong-shape results | ok/ok:false records with exact messages                                                                             | required | *        | tool-result boundary                         |
| tool-loop.terminal                                                        | tool-loop          | prompt producing text-only turn                                     | response_completed, one assistant_message                                                                           | required | *        | terminal completion                          |
| tool-loop.tool-rounds                                                     | tool-loop          | maxToolRounds 0/1/8/32                                              | rounds executed, cap message at bound                                                                               | required | *        | round budget                                 |
| tool-loop.unknown-tool                                                    | tool-loop          | provider calls unregistered tool                                    | failed result + provider recovery turn                                                                              | required | *        | unknown tool                                 |
| tool-loop.hidden-tool-denied                                              | tool-loop          | provider calls a projected-hidden tool                              | denied result before execution                                                                                      | required | *        | projection-schema enforcement                |
| tool-loop.one-call-one-result                                             | tool-loop          | multi-call turn incl. invalid + cancelled                           | exact pairing, ordering, retained results                                                                           | required | *        | pairing/ordering/retention                   |
| tool-loop.cancelled-round                                                 | tool-loop          | abort during round                                                  | cancelled results for skipped calls, no false completion                                                            | required | *        | round cancellation                           |
| tool-loop.assistant-text-with-tools                                       | tool-loop          | text + calls in one turn                                            | transcript ordering (assistant then round)                                                                          | required | *        | mixed turn                                   |
| tool-loop.invalid-call-pairing                                            | tool-loop          | invalid TurnToolCall from an empty/duplicate call                   | every retained assistant_tool_call ↔ exactly one failed tool_result                                                 | required | *        | invalid-call → failed-result pairing (R7.2)  |
| tool-loop.duplicate-call-result-pairing                                   | tool-loop          | duplicate call id turn then round                                   | first call executes with its result; later duplicate gets its own failed result; results in original call order     | required | *        | duplicate-call pairing (R7.2)                |
| tool-loop.empty-call-result-pairing                                       | tool-loop          | empty id/name call turn then round                                  | invalid call receives exactly one failed result in call order                                                       | required | *        | empty-call pairing (R7.2)                    |
| context-projection.estimate                                               | context-projection | text/json/conversation inputs                                       | token/byte estimates                                                                                                | required | *        | estimator determinism                        |
| context-projection.pressure                                               | context-projection | estimates at 0.69/0.7/0.85/1.0 boundaries                           | state classification (inclusive)                                                                                    | required | *        | thresholds                                   |
| context-projection.trim                                                   | context-projection | over-budget conversations                                           | dropped pairs, user-message survival, validity                                                                      | required | *        | trim rules                                   |
| context-projection.hard-block                                             | context-projection | irreducible hard pressure                                           | blocked request record                                                                                              | required | *        | hard block                                   |
| context-projection.segments                                               | context-projection | segment inputs                                                      | sorted serialization, stable fingerprint stability                                                                  | required | *        | ordering + fingerprint                       |
| context-projection.tool-visibility                                        | context-projection | tools × modes × policy                                              | available/gated/hidden counts + ABI fingerprint                                                                     | required | *        | visibility projection                        |
| user-config.absent                                                        | user-config        | nonexistent path                                                    | defaults outcome                                                                                                    | required | *        | absence                                      |
| user-config.valid                                                         | user-config        | full valid config                                                   | parsed config outcome                                                                                               | required | *        | accepted keys/defaults                       |
| user-config.unknown-keys                                                  | user-config        | unknown section/key at each level                                   | exact rejection messages                                                                                            | required | *        | unknown keys                                 |
| user-config.invalid-values                                                | user-config        | bad profile/backend/edition/path/ref                                | exact rejection messages                                                                                            | required | *        | invalid values                               |
| user-config.bounds                                                        | user-config        | > 1 MiB file / > 16 references / > 128 id                           | exact rejection messages                                                                                            | required | *        | size/count bounds                            |
| user-config.not-json                                                      | user-config        | malformed JSON file                                                 | parse rejection message                                                                                             | required | *        | parsing                                      |
| user-config.symlink                                                       | user-config        | symlinked config path                                               | regular-file rejection                                                                                              | required | *        | symlink rejection                            |
| user-config.diagnostics                                                   | user-config        | broken config via diagnostics reader                                | loaded=false + validationErrors record                                                                              | required | *        | diagnostics                                  |
| cli-session.prompt-parse                                                  | cli-session        | scripted stdin lines                                                | parsed input outcomes (prompt/command/empty/invalid)                                                                | required | *        | input normalization                          |
| cli-session.eof-exit                                                      | cli-session        | EOF                                                                 | exit code 0                                                                                                         | required | *        | EOF                                          |
| cli-session.help-status                                                   | cli-session        | `/help`, `/status`                                                  | stable literal output                                                                                               | required | *        | deterministic output                         |
| cli-session.prompt-flow                                                   | cli-session        | fake-provider prompt                                                | rendered deltas + completion, sanitized                                                                             | required | *        | event rendering                              |
| cli-session.interrupt                                                     | cli-session        | Ctrl+C on active prompt                                             | cancel report, session stays alive                                                                                  | required | *        | interruption                                 |
| cli-session.invalid-command                                               | cli-session        | unknown slash command                                               | invalid-command output, session continues                                                                           | required | *        | non-fatal errors                             |

Minimal set required before R7.1 implementation begins:
`provider-turn.basic`, `provider-turn.text-bytes-boundary`,
`provider-turn.multibyte`, `provider-turn.tool-call-bounds`,
`provider-turn.duplicate-call-id`, `provider-turn.empty-id-name`,
`provider-turn.eof-no-completion`, `provider-turn.event-after-completed`,
`provider-turn.cancelled`, `provider-turn.unknown-event`,
`provider-turn.malformed-known-variant`, `provider-turn.result-detach.*`
(≈ 13 scenarios; `result-detach` is a family), plus
`provider-turn.invalid-transcript` for the frozen transcript-before-use
acceptance contract.

**R7.1 implementation status:** the minimal set is complete — 18
`provider-turn` scenarios (including the `result-detach` family and the
invalid-transcript case) hold differential parity (corpus schema 3, corpus
version 12, 104 scenario files; verified executable baseline
3a08a86605f0395244a55eaab1b8db84de22d7f7). Later scenarios for complete
R7: the rest of `provider-turn`, all `tool-loop` (including the R7.2
invalid-call pairing scenarios), `context-projection`, `user-config`, and
`cli-session` (planned ≈ 44 scenario rows; exact counts finalized when each
subject is implemented). Authority assumptions:
every scenario input is Host-controlled; the provider under test is the
deterministic fake; policy is the default `inspect` policy unless the
scenario declares otherwise; no scenario grants or exercises mutation,
process, Git, network, or Godot authority.

### 8.2 Harness changes per subject (mechanical, with implementation)

1. `tests/differential/shared/contract.mjs`: add subject to `ALLOWED_SUBJECTS`, add a subject-specific input bound (e.g. `providerInputBytes: 64 KiB`) and a `validateSubjectInputs` branch; bump `CORPUS_VERSION`.
2. `tests/differential/shared/protocol.mjs`: add a `validate*Result` for the subject's canonical outcome record.
3. Oracle probe: new `tests/differential/probes/<subject>-oracle.mjs` exercising the real reference modules (e.g. `createDeterministicFakeProvider` + `collectBoundedModelTurn`/application for `provider-turn`; `parseUserConfig` for `user-config`; `runInteractiveSession` with scripted `SessionIO` for `cli-session`).
4. Candidate: `crates/siralos-cli/src/harness.rs` subject constant, validator, and `run_scenario` dispatch against the real `siralos-core`/-`adapters` modules.
5. Corpus files + `regenerate-corpus-manifest.mjs`; run `npm run check:differential` (exit 0 required) and `npm run check`.

## 9. Additional adversarial evidence (beyond the differential corpus)

- **Rust unit tests (core)**: bounded-turn state machine at every limit boundary (inclusive/exclusive), UTF-8 multi-byte accounting, duplicate/empty call ids, event-after-completion, EOF-without-completion, cancellation precedence, result-detach each failure class, transcript validation pairing, registry duplicate rejection/order, round pairing with cancelled tails, estimator/pressure/trim unit boundaries, segment sort/serialize/fingerprint stability, tool visibility projection, config schema validation per rule (reuse the TS test matrix).
- **Adapter tests**: deterministic fake provider determinism and scenario precedence; bounded stream consumption; config file loading (absent/symlink/oversize/not-JSON) over real temporary files.
- **CLI integration tests**: scripted interactive sessions (help/status/prompt/EOF/interrupt/invalid command/exit codes), input-queue arbitration (timeout/abort/discard/EOF-denial, type-ahead rerouting), sanitizer adversarial corpus (C0/C1, CSI, OSC 8 links, split sequences, lone surrogates, path line-spoofing).
- **Adversarial tests**: provider streams that ignore the signal; streams that yield after `completed`; giant ids/args across event boundaries; cyclic/non-serializable arguments and results; duplicate call ids across rounds; result-object mutation after submission (detachment); config files that are directories/symlinks/grown-after-stat; terminal injection strings through tool names/summaries/paths (GT-010 discipline).
- **Authority separation tests**: a provider calling a hidden tool is denied before execution; a denied capability never executes; approval denial/EOF/timeout/cancel prevent execution; prepared plans bind to the exact digest.

## 10. Rust ownership design

```text
siralos-cli        composition only: session loop, input queue, output
    |              rendering, terminal sanitizer, doctors
    v
siralos-adapters   deterministic fake provider, bounded provider stream
    |              collection, tool implementations (workspace read tools
    |              from R4), user-config loading/validation
    v
siralos-core       provider-neutral contracts and semantics only:
                   ModelRequest/ModelEvent/ConversationItem/ToolDefinition/
                   ToolExecutionResult types, bounded-turn collection state
                   machine, transcript validation, tool registry semantics,
                   round execution, loop budgets, token estimator, pressure
                   classification, context segment model + sort/serialize +
                   stable fingerprint, conversation trim, evidence
                   normalization, tool visibility projection, failure
                   types
```

- **Provider trait**: justified — a real behavioral seam exists (core must stay
  provider-neutral; the deterministic fake adapter implements the seam; future
  real provider adapters plug in). Smallest interface: one bounded turn with
  cancellation and event consumption — a pull-based iterator over typed events
  plus a cancellation check; no provider SDK types in core, no factories,
  registries, or managers.
- **Bounded turn**: core owns the collection state machine (bounds, ordering,
  completion/EOF/cancellation precedence, detach); adapters own how a concrete
  provider's stream is consumed (for the fake: in-process deterministic
  iterator; async only where a real adapter requires it).
- **Tool loop**: core owns registry, round, pairing, budgets, permission
  re-check, projection-schema guard; adapters own concrete tools; CLI owns
  composition and rendering.
- **Projection**: core owns estimator/pressure/trim/segment/fingerprint/
  evidence/visibility; the seam where a future compiled Context (R10) enters
  provider requests is exactly `projectRequest`-equivalent output feeding
  `ModelRequest` — providers receive projected requests and never own Context
  selection (compatibility requirement, not an implementation task).
- **Configuration (R7A-consistent)**: `siralos-adapters` owns external config
  file discovery/loading, the bounded EOF-verified read, symlink/filesystem
  policy, and parsing of the current external config format — including its
  strict format-specific validation rules (unknown-key rejection, enum
  checks, bounds). `siralos-core` owns only configuration semantics that are
  genuinely Host/domain-neutral and required independently of the file format
  (today: none — the doctor/self-reference diagnostic models consume
  adapters' results). `siralos-cli` owns path/override composition and
  user-facing diagnostics. Validation being "semantic" does not move parsing
  into Core; no generic configuration framework is created.
- **Determinism**: encode required ordering with typed ordered collections;
  reproduce JS `JSON.stringify` escaping for canonical digests exactly.
- **Cancellation**: cooperative signal checks between events; no async in core
  unless a concrete adapter needs it.
- Ownership naturally replaces TypeScript defensive-copy machinery (deep-freeze,
  JSON round-trips at call sites): values are moved into host-owned structures
  at the boundary; the observable invariant (caller mutation can never affect
  retained state) is preserved by construction.

## 11. First implementation slice

**R7.1 — Provider contract + deterministic fake provider + bounded single
model turn** (exactly one task):

- `siralos-core`: `ModelRequest`/`ModelEvent`/`ConversationItem`/
  `ToolDefinition`/`ToolExecutionResult` types; the bounded turn-collection
  state machine with all §5.1 limits, completion/EOF/cancellation precedence,
  duplicate/empty call-id rules, JSON detach, and the tool-result detach
  boundary; transcript validation; failure types with exact messages.
- `siralos-adapters`: deterministic fake provider (echo, 16-codepoint
  chunks, tool-call scenarios, cancellation, determinism).
- `siralos-cli`: the harness subject dispatch for `provider-turn`.
- Differential: the minimal `provider-turn` scenario set defined in §8.1
  (the authoritative enumeration), corpus version bump + manifest
  regeneration; `npm run check:differential` green.
- Rust unit/adapter tests for the adversarial matrix in §9 that concerns the
  turn only.
- Excluded from R7.1: the application tool loop (R7.2+), projection (R7.3),
  configuration (R7.4), CLI session (R7.5) — the loop is separable: the turn
  collector and the round executor have distinct contracts and tests.

## 12. Deferred scope (explicitly not R7)

- Real OpenAI/Anthropic providers, provider auto-discovery, marketplaces,
  dynamic provider loading, credentials, model routing.
- Profiles, `siralos.toml` target composition model, `siralos.lock`,
  Skills, Plugins, Views, hot Plugin lifecycle (ADR 0036 targets).
- The R10 canonical Context compiler, Context controls (Live/Pinned/Frozen),
  generic context/provenance dependency engines (`context/artifacts.ts`,
  `provenance.ts`, `staleness.ts`, `source-integrity.ts`).
- Godot domain behavior — all `godot.*` tools and Godot-specific config
  _semantics_ (installation selection, PATH discovery, edition classification,
  engine profiles) — R8/R9. The generic configuration envelope (loading,
  bounded read, strict structural validation of the current reference format,
  including the `godot` section's shape per §2.4.1) remains R7
  parse-compatible because it is observable current reference behavior.
- Mutation application, process execution, Git execution, run directories,
  checkpoints/undo effects, references materialization, research fetch —
  R11 effect/security work; the existing fail-closed typed unavailable
  surfaces remain the parity baseline.
- Hooks, middleware, workflow engines, multi-agent frameworks, service
  containers, DI frameworks, schedulers, event buses, telemetry frameworks.
- Native sandbox redesign, recovery orchestration, hot reload.
- Prompt caching (only the ordering/stable-prefix properties in §6 are R7
  requirements; cache identity is derived behavior, never authoritative
  state).

## 13. R7.2 entry review — Application Tool Loop contract freeze

Recorded before any R7.2 implementation. Status: **PASS** — the R7.2
Application Tool Loop contract is frozen and R7.2 may begin. This review
ports no Rust code, adds no corpus files, and modifies no executable source.

### 13.1 Review baseline

| Item           | Value                                                                                                                                                                                      |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Branch         | `main`                                                                                                                                                                                     |
| HEAD           | `4dd8aea3c6394be3d751890ccb30c8c33a185364` (`docs: record R7.1 measurement and final acceptance`)                                                                                          |
| Upstream       | `origin/main` (`4dd8aea`, up to date)                                                                                                                                                      |
| Worktree       | Clean (`git status --short` empty)                                                                                                                                                         |
| Verified R7.1  | `bd335190696f3662e242407c89d7821483d7fa24` — present in local history; the only commits after it are documentation-only (`a9c7233`, `4dd8aea`)                                             |
| Baseline check | `npm run check` — **PASS**, exit code 0 (full gate: format, lint, typecheck, unit/integration, architecture, identity ratchet, Rust architecture, full Rust gate) on the starting worktree |

### 13.2 Scope decision

R7.2 owns the **generic Application Tool Loop** only. The TypeScript
`application.ts` co-locates provider-turn iteration, registry, round
execution, transcript updates, budgets, unknown/hidden-tool handling,
per-call capability evaluation, approved-surface defense, the approval
workflow, prepared workspace mutation, prepared command execution, three
Godot prepared kinds, checkpoint events, and command audit state. R7.2
must not mechanically port that file. The smallest owned behavior is:

```text
Tool registry (one immutable concept)
    ↓
provider Tool proposal (R7.1 TurnToolCall)
    ↓
Host lookup
    ↓
Host authorization decision (approved surface + per-call capability recheck)
    ↓
generic Tool execution seam (Tool trait, execute)
    ↓
typed ToolExecutionResult (R7.1 value, reused)
    ↓
exactly one ToolResult appended to the authoritative transcript
    ↓
next provider turn
```

plus: single-flight prompt ownership, tool-round budget, round sequencing,
round cancellation, invalid-call pairing, terminal provider answer,
provider failure/cancellation propagation, and the application event
semantics of this layer (closed generic event set, §13.10).

### 13.3 TypeScript co-location audit (runToolCall branch classification)

Every branch in `application.ts` `runToolCall` plus the loop branches in
`sendPrompt`, classified into the four frozen categories.

| Branch (application.ts)                                                                                                          | Classification                 | Owning milestone / note                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------ |
| `tool_started` before lookup with `toDisplayInput` (200-char truncation)                                                         | R7.2 MUST PORT                 | Deterministic loop-emitted event value (bound §5.2); rendering the event is CLI-only.            |
| Registry lookup; unknown → `tool_failed` + `Failed` result                                                                       | R7.2 MUST PORT                 | Exact message `Unknown tool: <name>.`; recoverable.                                              |
| Projection-schema guard (`lastProjection` name set)                                                                              | R7.2 GENERIC SEAM ONLY         | Narrow approved-tool-surface seam (§13.8); no projection import (§13.12).                        |
| Capability re-evaluation (`evaluatePermission` deny/ask)                                                                         | R7.2 MUST PORT                 | Per-call Host authorization recheck (§13.7).                                                     |
| Plain `Tool.execute` under `ask` (no reviewable preparation)                                                                     | R7.2 MUST PORT                 | `deny without execution` with the frozen message (§13.14); not the approval framework.           |
| Plain `Tool.execute` success/failure/cancellation catch                                                                          | R7.2 MUST PORT                 | `cancelled` (described) vs `failed` (describeError).                                             |
| `emitToolOutcome` status → event mapping                                                                                         | R7.2 MUST PORT                 | success→`tool_completed`; cancelled→`tool_cancelled`; every other status→`tool_failed`.          |
| Invalid TurnToolCall emission in the round (empty/duplicate ids)                                                                 | R7.2 MUST PORT                 | Failed result paired, no lookup, no execution (§13.5).                                           |
| Budget check before round, round counter, cap failure message                                                                    | R7.2 MUST PORT                 | §13.6.                                                                                           |
| Round transcript append + mixed-turn assistant text rule                                                                         | R7.2 MUST PORT                 | §13.5, §13.9, §13.14.                                                                            |
| Single-flight state; `response_started`/cancelled/failed/completed                                                               | R7.2 MUST PORT                 | §13.6, §13.14.                                                                                   |
| `text_delta` passthrough from the provider turn                                                                                  | R7.2 MUST PORT                 | Replayed in order from the R7.1 turn outcome.                                                    |
| `onProviderTurnCompleted` hook                                                                                                   | R7.2 GENERIC SEAM ONLY         | The loop exposes the terminal turn outcome; the hook itself is composition glue (CLI), not core. |
| Prepared mutation (`isPreparedMutationTool` prepare/approval/apply, `checkpoint_applied`)                                        | LATER MILESTONE                | R11 mutation/effect work; R4/R5 typed prepared availability already exists.                      |
| Prepared command (`runPreparedCommandTool`, `streamPreparedCommand`, audit, `command_*` events)                                  | LATER MILESTONE                | R11 process execution + its owning approval/audit work.                                          |
| Godot project probe (`runPreparedProbeTool`)                                                                                     | LATER MILESTONE                | R8/R9.                                                                                           |
| Godot GDScript diagnostics (`runPreparedDiagnosticTool`)                                                                         | LATER MILESTONE                | R8/R9.                                                                                           |
| Godot LSP session (`runPreparedLSPSessionTool`)                                                                                  | LATER MILESTONE                | R8/R9.                                                                                           |
| `runApprovedTool` generic one-time approval protocol                                                                             | LATER MILESTONE                | The approval framework belongs to its owning effect milestone (§13.14).                          |
| `approval_requested` / `tool_awaiting_approval` / `approval_resolved`                                                            | LATER MILESTONE                | Owning effect milestones only.                                                                   |
| `checkpoint_applied`                                                                                                             | LATER MILESTONE                | R11 checkpoint effects.                                                                          |
| `context_pressure`                                                                                                               | LATER MILESTONE                | R7.3 projection.                                                                                 |
| `pendingApproval` / `activeCommandId` / `lastCommandExitCode` / `commandHistory`; `getCommandHistory` / `getLastCommandExitCode` | SOURCE STRUCTURE — DO NOT PORT | Co-located TypeScript state; not generic R7.2 requirements.                                      |
| `SessionStatus` beyond `state` + message count                                                                                   | SOURCE STRUCTURE — DO NOT PORT | CLI presentation surface; R7.2 keeps `Idle                                                       | Responding` + transcript length only. |

### 13.4 Registry contract

Freeze (R7.2, one registry concept in `siralos-core::tool`):

- duplicate name → construction failure with the exact message
  `Duplicate tool name: <name>`;
- lookup → exact, case-sensitive match; unknown name → `None` (never a
  fuzzy match, never partial);
- `definitions()` → a fresh, ordered representation (caller mutation
  cannot affect the registry); registration order retained end-to-end;
- the registry is immutable after construction;
- each definition carries its capability metadata (registered info =
  definition + capability; a plain tool defaults to `workspace.read`);
- the registry holds one concept only: the ordered immutable tool set with
  deterministic lookup. No `ToolManager`/ToolService/ToolFactory/plugin
  loader or dynamic loading is created.

Rust shape: an immutable tool table owning the concrete tools in
registration order plus an index for O(1) exact lookup; observable
ordering stays Vec-ordered (no unordered-map iteration is observable).

### 13.5 Tool execution and round contract

- One generic `Tool` execution seam (ADR 0036 §26 — Tool is the one
  callable operation abstraction): `definition()`, `capability()`, and
  `execute(input, cancellation) -> ToolExecutionResult`.
- The result value is the R7.1 `ToolExecutionResult` (reused — no second
  result enum; §13.15).
- Round execution (`executeToolRound` contract, exact):
  - input: ordered `TurnToolCall` values from R7.1;
  - transcript pre-seeded with one `assistant_tool_call` per retained
    call in emission order (an invalid retained call is recorded without an
    input payload — the reference writes `input: undefined`, which
    serializes as an omitted field; the Rust `ConversationItem` needs an
    input-presence representation for this case);
  - execution is strictly sequential in provider emission order;
  - invalid calls: no lookup, no execution; emit `tool_failed` with the
    deterministic invalid-call message and store exactly one `Failed`
    result with the same call id/name; continue to later calls;
  - every retained `assistant_tool_call` receives exactly one
    `tool_result` (same call id, same tool name) before the transcript
    leaves the round — no orphans, no missing, no duplicates, no
    reordering; cancellation never breaks pairing;
  - signal already aborted before call N: call N and every later unstarted
    call receive `Cancelled` with the exact message
    `The tool call was cancelled before it executed.`;
  - a tool returns `cancelled`: the round stops; its own result keeps the
    tool's message; every later call receives the same deterministic
    skipped-call cancelled result;
  - no later tool ever executes after cancellation;
  - round outcome: `completed` or `cancelled`, both carrying the full
    paired transcript.
- One small explicit round-state owner (transcript, current call index,
  cancelled state). No ToolRoundManager / ExecutionScheduler / TaskGraph /
  workflow engine / event bus / worker pool; sequential stays sequential.
- Cancellation authority: a Tool receives only the read-only
  `CancellationSignal` observation view — never the Host
  `CancellationToken`. The R7.1 authority correction is carried to the
  Tool boundary by the type (the signal exposes no mutation operation and
  no accessor that yields the controller). The Tool Round caller (Host)
  holds the controller; the Tool's execution context exposes no
  controller path (§13.7).
- The loop does not JSON-schema-validate tool arguments; Tools own input
  validation and may return `invalid_input` (§13.9). `ToolDefinition.inputSchema`
  remains provider-visible metadata, not a runtime validator.

### 13.6 Application loop, budget, and single-flight

- Loop (from `sendPrompt`, exact): append `user_message` → emit
  `response_started` → repeat: collect one bounded provider turn (R7.1);
  cancelled → `response_cancelled` → stop; failed → `response_failed`
  (exact message) → stop; zero tool calls → append `assistant_message`
  only when text is non-empty → terminal → `response_completed` → stop;
  tool calls → budget check **before** the round → execute the round →
  append transcript → continue.
- Budget (`normalizeMaxToolRounds` + cap check, exact):
  default 8; hard maximum 32; undefined/non-finite → 8; else
  `clamp(floor(v), 0, 32)`. Negative → 0; fractional → floor; 0/1/8/32
  exact; >32 → 32. The requested round that **exceeds** the budget is NOT
  executed. Cap failure message (exact):
  `Siralos reached the maximum of <n> tool rounds; the requested tool round was not executed.`
  — a `response_failed`; history keeps the user message and all prior
  rounds. Final-answer boundary: with maxToolRounds = 1, round 1 may
  execute and then a text-only next turn completes; a tool-calling next
  turn fails and round 2 never executes.
- Single-flight: one prompt may be actively responding at a time; a second
  prompt while responding fails deterministically with
  `Siralos is already responding to a prompt.` A typed
  AlreadyResponding-style start failure is required. No Mutex/RwLock/async
  is needed for theoretical concurrency: the observable requires only a
  typed `Idle|Responding` state plus an event surface that can be
  consumed stepwise/lazily (the reference is an async generator whose
  state flips on first pull, which is how the already-responding rejection
  is observable mid-stream). The Rust loop should expose a pull/stepwise
  event session so the deterministic harness and unit tests can interleave
  without threads (§13.16).

### 13.7 Authority model

```text
Provider proposes a Tool call (proposal only — never authority)
    ↓
Host Tool surface (registry definitions + policy filter; optional
approved-tool-surface seam)
    ↓
Host per-call authorization, immediately before execution:
    registered? (unknown → failed result)
    approved surface contains name? (hidden → denied result)
    capability decision rechecked now? (deny → denied result)
    ↓
Tool executes only when the Host invokes it
```

Permanent rules: a Tool can never register itself, never broaden its
capability, never mutate Host cancellation state (signal only), and never
mutate Host history directly (inputs move into the loop; history is Host
owned). Visible/proposed does not imply authorized: the per-call capability
recheck is part of the R7.2 contract, and the Rust implementation must
represent the observable decision rule (allow/ask/deny with the exact
reference reasons) without porting the TypeScript policy/profile object
graph and without creating a second permission system. The small Rust
capability/rule/evaluator (§13.17) is Host-loop-owned; the R6
`domain::capability` types remain the Domain host-boundary vocabulary and
are not reused here (different identifier vocabulary and boundary).

**Capability representation — domain-neutral by construction (R7.2
independent-review remediation).** `siralos-core` MUST NOT contain
Godot/domain-specific capability semantic types: no `Capability` enum with
Godot (or any optional-domain) variants, no optional-domain semantic
capability types, and no requirement to modify Core merely because a future
Domain adds capability identifiers. The selected representation is a small
validated domain-neutral capability identifier: an opaque `CapabilityId`
whose grammar accepts the reference identifier format (non-empty, bounded
length, lowercase ASCII letters/digits and `.`/`_`/`-` separators) and whose
Core semantics know nothing about any optional domain — `godot.inspect` is
an identifier to Core, never a semantic type. Core may compare and evaluate
identifiers; it must not understand domain semantics. A type materially
like `enum Capability { WorkspaceRead, GodotInspect, GodotLsp, ... }` is
explicitly prohibited in Core. The R6 `domain::capability` types (dash-only
identifier grammar, Domain host-boundary vocabulary) are deliberately not
reused for the Tool Loop because their grammar rejects the reference
`dot`/`underscore` identifier format and they are bound to the Domain
boundary. Future optional-domain Tool capabilities enter through the same
generic `CapabilityId` + policy path with zero Core changes.

### 13.8 Projection seam (R7.3 boundary)

R7.2 implements no projection. The loop enforces the model-visible schema
through one narrow, future-compatible seam:

```text
HostAuthorizedToolSurface (approved model-visible tool names)
    = Option<ordered set of tool names>, supplied by the composition root

provided → a provider call whose name is outside the set is denied BEFORE
           execution: "Tool <name> is not in the projected tool schema for
           this session and was denied before execution." (message frozen)
absent   → the R7.1 behavior: the request surface is the policy-filtered
           registry (no additional guard) — the loop stays unchanged
```

R7.3 later projects real visible tool names into this exact seam (the
reference guard reads `lastProjection().tools` names; absent projection
means no guard). Nothing of the projection service, estimator, pressure
model, fingerprints, trim, or Context segments is imported by R7.2.

### 13.9 Tool input validation

The R7.2 loop performs no generic JSON-Schema validation of Tool
arguments; `ToolDefinition.inputSchema` is provider-visible metadata only,
and the Tool implementation owns authoritative runtime validation. Exact
`invalid_input` boundary (independent-review remediation):

```text
Host (after registry / visible-surface / capability gates):
    invokes Tool.execute(input, cancellation_signal)
        ↓
Tool:
    validates its own input at the execution boundary
    invalid input → returns ToolExecutionResult::InvalidInput
        BEFORE any substantive/effectful Tool work
        ↓
Tool Loop:
    emits tool_failed
    stores the paired invalid_input result
    provider may recover on the next turn
```

`Tool.execute` IS invoked for an invalid input — "no execution" is never a
valid paraphrase; only the substantive/effectful Tool work body is not
entered after the validation failure. No generic schema runtime is
invented and no loop-side validator is added.

### 13.10 Application event surface

One closed generic event enum for R7.2 only:

```text
response_started, text_delta, response_completed, response_cancelled,
response_failed,
tool_started (callId, toolName, displayInput-truncated),
tool_completed (summary), tool_failed (message), tool_cancelled
```

Not ported into the R7.2 type: `tool_awaiting_approval`,
`approval_requested`/`approval_resolved`, `checkpoint_applied`,
`context_pressure`, and all `command_*` events (each owned by its later
milestone, §13.13). Structural copying of the full TypeScript union is not
required; behavioral parity is. `displayInput` truncation is a
deterministic loop-emitted value and is part of the R7.2 event contract,
not merely presentation. Exact algorithm (independent-review remediation):

```text
text = JSON.stringify(input)

if serialization returns undefined:
    displayInput = "<unprintable>"
else if text.length <= 200:
    displayInput = text
else:
    displayInput = text.slice(0, 200) + "..."
```

The truncation unit is the JavaScript UTF-16 code unit: `String.length`
counts UTF-16 code units and `String.slice(0, 200)` cuts at UTF-16 code
unit boundaries. Rust must reproduce exactly this semantics for the
serialized JSON string — it must NOT silently substitute 200 UTF-8 bytes,
200 Unicode scalar values, or 200 grapheme clusters unless executable
differential evidence proves equivalence over the accepted input domain.
When the boundary splits a UTF-16 surrogate pair, the observable canonical
record carries the reference's serialized form (JSON escape), which the
Rust formatter must reproduce at the boundary.

Reachability of `<unprintable>`: through the accepted R7.2 input protocol
the fallback is structurally impossible on BOTH implementations — the R7.1
application collector retains only `JSON.parse(JSON.stringify(input))`
values (always JSON-serializable), and the Rust typed input is
`serde_json::Value` (always serializable). The fallback stays in the
contract for algorithmic completeness; no fixture is required, and Rust
must not weaken its typed input merely to create an impossible internal
state.

### 13.11 Failure taxonomy and recovery

Typed internal loop failures (`siralos-core::tool`), classified as
terminal-for-the-prompt or recoverable-result:

| Failure                     | Terminal?            | Observable effect                                                                                           |
| --------------------------- | -------------------- | ----------------------------------------------------------------------------------------------------------- |
| AlreadyResponding           | n/a (start rejected) | `Siralos is already responding to a prompt.` thrown start failure.                                          |
| ProviderFailed              | terminal for prompt  | `response_failed` with the R7.1 message; nothing of the failed turn committed.                              |
| ToolRoundLimitExceeded      | terminal for prompt  | `response_failed` with the frozen cap message; user message + prior rounds retained.                        |
| UnknownTool                 | recoverable result   | `tool_failed` + `Failed` result paired; next provider turn.                                                 |
| ToolDenied (capability)     | recoverable result   | `tool_failed` + `Denied` result paired; next provider turn.                                                 |
| ToolDenied (hidden surface) | recoverable result   | `tool_failed` + `Denied` result paired; next provider turn.                                                 |
| ToolFailed                  | recoverable result   | `tool_failed` + `Failed` result paired; next provider turn.                                                 |
| ToolCancelled               | terminal for prompt  | `tool_cancelled` + `Cancelled` result; round stops; tail calls get skipped-cancelled; `response_cancelled`. |
| InvalidToolCall             | recoverable result   | `tool_failed` + `Failed` result paired (no execution); next provider turn.                                  |

The distinction is typed, never parsed from prose. Tool failure / unknown
Tool → paired failure result → next provider turn → provider may recover is
**normal Tool-loop continuation**, not R11 recovery orchestration. No
retries, no backoff, no fallback routing, no self-healing: one provider
call → at most one execution attempt; the provider decides what to propose
next.

### 13.12 Projection/prepared-effects boundaries (re-asserted)

- Projection (R7.3): only the approved-tool-surface seam above; no
  projection implementation, no ToolProjector, no context estimation.
- Prepared effects (deferred): prepared mutation (R11), prepared commands
  (R11), Godot probe/diagnostic/LSP (R8/R9), approval workflows (owning
  effect milestone), checkpoint application (R11), command auditing (R11).
- `hidden-tool-denied` is modeled through the seam, never through R7.3.
- No scenario grants or exercises mutation, process, Git, network, or
  Godot authority.

### 13.13 Deferred effect work (owning milestone)

```text
workspace mutation application  -> R11 (fail-closed typed unavailable
                                    already exists from R4/R5)
process execution               -> R11
approval workflow               -> its owning effect milestone
Godot project probes            -> R8/R9
Godot GDScript diagnostics      -> R8/R9
Godot LSP sessions              -> R8/R9
checkpoint application          -> R11
command auditing                -> R11
context projection/pressure     -> R7.3
configuration                   -> R7.4
interactive CLI session         -> R7.5
```

### 13.14 Approval boundary

Default expectation confirmed: **no approval behavior is required by the
generic R7.2 acceptance scenarios.** The loop proves itself with allow,
deny (capability), unknown tool, invalid input, success, failure, and
cancelled. The one generic rule that IS R7.2: an `ask` decision for a
Tool with no reviewable preparation is **denied without execution** (exact
message frozen: `Capability <cap> requires approval, but this tool does not support a reviewable preparation protocol; the call was denied without execution.`).
No approval reviewers or digest-bound prepared-effect execution are ported.

### 13.15 Tool result detachment

The R7.1 typed owned `ToolExecutionResult` and its detach boundary
(`detach_bounded_tool_result`) are reused; R7.2 adds no second parse
path. The loop stores the owned result in the transcript and resends it to
the next provider turn from authoritative history; Rust ownership makes
detachment natural and the boundary already exists for size accounting.
For an in-process Rust Tool returning an owned valid result, no additional
serialization is needed beyond that R7.1 boundary (no JSON churn inside
the loop).

### 13.16 Determinism

Required ordering: registry definitions in registration order; provider
proposals in provider order; sequential execution in that order; results
in call order; transcript appends explicit (user message → [assistant
text] → round transcript); invalid-call synthetic ids `invalid-call-N` in
deterministic order; round counter a deterministic integer. No unordered
map iteration is observable; no wall clock, randomness, threads, or
environment ordering. Skip-call messages are fixed literals (observable
parity).

### 13.17 Rust ownership (proposed)

```text
siralos-core::tool (new module, closed over generic R7.2 only)
    - ToolDefinition / ToolExecutionResult (reuse from provider module)
    - ToolRegistry (immutable; duplicate rejection; ordered definitions;
      exact case-sensitive lookup; capability metadata)
    - Tool trait (definition, capability, execute(input, CancellationSignal))
    - Tool round state + execution (pairing, invalid calls, cancelled tail)
    - Application tool loop (single-flight, round budget, transcript rules)
    - RoundBudget normalization (exact clamp/floor/default)
    - CapabilityId (validated opaque identifier, generic grammar, no
      optional-domain semantics — never an enum with domain variants)
    - PermissionRule / PermissionDecision + evaluator (reference decision
      order and exact reasons; no second permission system; no domain
      semantic types)
    - Approved-tool-surface seam (optional ordered name set)
    - ToolLoopEvent / AppOutcome (closed generic event set)
    - typed failures (AlreadyResponding ... InvalidToolCall)

siralos-adapters::tool (new module)
    - concrete workspace.list / workspace.read / workspace.search Tool
      adapters over the R4 adapter functions (typed input validation →
      invalid_input; detached results)
    - no loop policy, no Host authorization decisions

siralos-cli
    - differential harness composition only: tool-loop subject dispatch,
      canonical records, and harness-local deterministic stub Tools
      (success / invalid_input / denied / failed / cancelled) exercised
      through the real production registry/round/loop
```

No new crate. No loop semantics in the CLI. Adapters never decide Host
authorization. Dynamic dispatch only where a real seam requires it
(static-generic preferred, matching the R7.1 provider seam).

### 13.18 Differential plan (tool-loop subject)

Canonical result record fields (frozen): application event sequence;
terminal application outcome (completed/cancelled/failed + exact message);
provider request count / turn count; final authoritative transcript
(canonical conversation items); Tool execution order (from events and
transcript); Tool inputs; Tool results; completed round count; whether
execution occurred for each call. Excluded: wall-clock timings, absolute
paths, implementation-specific Rust enum names, debug output, pointer
identities.

Frozen scenario set (added atomically with the implementation). Concrete R7.2
Tool set (frozen): generic read-only adapter Tools `workspace.list`,
`workspace.read`, `workspace.search` (over the R4 adapter functions) plus
harness-local deterministic stub Tools for `success` / `invalid_input` /
`denied` / `failed` / `cancelled` — all entering through the real future
Tool Registry / Tool Round / Application Loop. No scenario grants or
exercises mutation, process, Git, network, or Godot authority.

Per-scenario detail (frozen; inputs are Host-controlled, parity required on
every host):

| Scenario id                               | Input / Tool registry                                                                                                                                                     | Host authority / visible surface                                                                                   | Provider script                                                                                          | Observable outcome (required evidence)                                                                                                                                                                                                                                         | Why R7.2                                                                                                                                |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `tool-loop.terminal`                      | prompt without any tool scenario; registry empty or workspace.read tools                                                                                                  | default `inspect`; surface = policy-filtered registry                                                              | deterministic fake echo text + `completed`                                                               | `response_started`, `text_delta`s, `response_completed`; transcript = user_message + one non-empty assistant_message                                                                                                                                                           | terminal completion contract                                                                                                            |
| `tool-loop.tool-rounds`                   | `maxToolRounds` 0/1/8/32 (+ normalization cases: missing/non-finite/negative/fractional/>32); registry = one stub tool                                                    | default `inspect`; surface = registry                                                                              | scripted: tool_call turn(s) then a final text turn (or another tool_call turn at the cap)                | executed-round count; at/over budget the requested round executes nothing; exact cap message `Siralos reached the maximum of <n> tool rounds; ...`; `response_failed`                                                                                                          | round-budget normalization and cap boundary                                                                                             |
| `tool-loop.unknown-tool`                  | prompt; registry without the requested name                                                                                                                               | default `inspect`; surface = registry                                                                              | scripted turn 1: `tool_call("c1", "mystery.tool")` + `completed`; turn 2: text `recovered` + `completed` | `tool_started` then `tool_failed` with `Unknown tool: mystery.tool.`; paired `Failed` result; turn 2 completes the prompt                                                                                                                                                      | unknown-Tool recovery = paired result + next provider turn                                                                              |
| `tool-loop.hidden-tool-denied`            | registry registers `b.tool`; approved visible surface excludes it                                                                                                         | default `inspect`; surface = approved-name set without `b.tool`                                                    | scripted: `tool_call("c1", "b.tool")` + `completed`, then recovery text turn                             | denied BEFORE execution with the exact message `Tool b.tool is not in the projected tool schema for this session and was denied before execution.`; paired `Denied` result; no execute call; provider recovers                                                                 | approved-surface execution guard (projection seam)                                                                                      |
| `tool-loop.one-call-one-result`           | one turn mixing execute + invalid + cancelled-tail calls; registry = stubs                                                                                                | default `inspect`; surface = registry                                                                              | scripted: `tool_call` c1, empty/duplicate call (invalid), tool returning `cancelled`, plus later calls   | exact pairing/order/retention; every retained call ↔ exactly one result (same call id and tool name); no orphans/missing/duplicates/reorder                                                                                                                                    | one-call/one-result invariant                                                                                                           |
| `tool-loop.cancelled-round`               | multi-call turn; Host cancellation during the round                                                                                                                       | default `inspect`; surface = registry                                                                              | scripted: two tool calls; cancellation after the first call completes                                    | first call executed with its result; every unstarted retained call gets `Cancelled` with `The tool call was cancelled before it executed.`; no later execution; no `response_completed`; `response_cancelled`                                                                  | round cancellation + cancelled-tail pairing                                                                                             |
| `tool-loop.assistant-text-with-tools`     | mixed turn (assistant text + tool calls)                                                                                                                                  | default `inspect`; surface = registry                                                                              | scripted: text deltas, `tool_call`, `completed`; next turn terminal text                                 | completed round: assistant_message precedes the round transcript; cancelled round: full paired transcript retained, mixed-turn text NOT committed                                                                                                                              | mixed-turn history commit order                                                                                                         |
| `tool-loop.invalid-call-pairing`          | turn containing an empty id/name call (R7.1 `TurnToolCall::Invalid`)                                                                                                      | default `inspect`; surface = registry                                                                              | scripted: invalid call + `completed`, then recovery turn                                                 | exactly one `Failed` result per retained invalid call (deterministic message), no lookup, no authorization, no execution; later calls continue                                                                                                                                 | invalid-call → failed-result pairing is R7.2 (R7.1 emits the proposal only)                                                             |
| `tool-loop.duplicate-call-result-pairing` | one turn with a duplicated call id                                                                                                                                        | default `inspect`; surface = registry                                                                              | scripted: `tool_call("c1")` twice + `completed`                                                          | first occurrence executes with its result; the duplicate becomes invalid-call and receives its own `Failed` result (`Duplicate tool call id: c1.`); results in original call order                                                                                             | duplicate-call pairing                                                                                                                  |
| `tool-loop.empty-call-result-pairing`     | one turn with an empty id or empty tool name                                                                                                                              | default `inspect`; surface = registry                                                                              | scripted: empty call + `completed`                                                                       | invalid call receives exactly one failed result in call order; no execution                                                                                                                                                                                                    | empty-call pairing                                                                                                                      |
| `tool-loop.final-answer-after-last-round` | `maxToolRounds` = 1                                                                                                                                                       | default `inspect`; surface = registry                                                                              | scripted turn 1: `tool_call` + `completed`; turn 2: final text + `completed`                             | round 1 executes; turn 2 is terminal text → `response_completed` success                                                                                                                                                                                                       | budget boundary success side                                                                                                            |
| `tool-loop.over-budget-round`             | `maxToolRounds` = 1                                                                                                                                                       | default `inspect`; surface = registry                                                                              | scripted turn 1: `tool_call` + `completed`; turn 2: another `tool_call` + `completed`                    | round 1 executes; turn 2's requested round is NOT executed (zero tools); `response_failed` with the exact cap message                                                                                                                                                          | over-budget round executes nothing                                                                                                      |
| `tool-loop.provider-fails-after-round`    | one completed tool round then a failing provider turn                                                                                                                     | default `inspect`; surface = registry                                                                              | scripted turn 1: `tool_call` + `completed`; turn 2: provider throws                                      | completed round transcript retained (call + result); subsequent provider failure → `response_failed` with the R7.1 message; no `response_completed`                                                                                                                            | provider-failure propagation after a committed round                                                                                    |
| `tool-loop.authorization`                 | deterministic subcases A allow / B deny / C ask-plain; registry registers one plain Tool (`workspace.read`)                                                               | A/B: default `inspect` (allow / explicit deny rule); C: policy `ask` for the Tool's capability; surface = registry | scripted: `tool_call` + `completed`, then recovery text turn per subcase                                 | A: executes, `tool_completed`; B: zero execute calls, `tool_failed` with the exact policy-denial message, paired `Denied` result, provider recovers; C: zero execute calls, `tool_failed` with the exact ask-no-preparation message, paired `Denied` result, provider recovers | capability deny and plain-Tool ask are distinct Host authorization gates needing direct TS↔Rust parity (independent-review remediation) |
| `tool-loop.display-input`                 | one matrix case family over `tool_started.displayInput`: exactly 200 units, 201 units, supplementary/multibyte characters crossing the boundary; registry = one stub tool | default `inspect`; surface = registry                                                                              | scripted: `tool_call` whose serialized input hits each boundary case + `completed`                       | `tool_started.displayInput` matches the reference's exact UTF-16-code-unit truncation (JSON.stringify, `slice(0,200)` + `...`) in every matrix case; `<unprintable>` classified structurally unreachable (§13.10) and asserted absent                                          | displayInput is an observable loop-emitted value; cross-language unit semantics must be proven (independent-review remediation)         |
| `tool-loop.tool-result-statuses`          | registry = deterministic stub Tools returning success / invalid_input / denied / failed / cancelled                                                                       | default `inspect`; surface = registry                                                                              | scripted: one `tool_call` per stub tool + `completed` per turn                                           | through the real production loop: success → `tool_completed`; invalid_input/denied/failed → `tool_failed` with the paired message; cancelled → `tool_cancelled`; every paired ToolResult status/message retained in order                                                      | ordinary Tool-returned status → event mapping is R7.2 MUST PORT (independent-review remediation)                                        |

Host authorization decision matrix (frozen — the five gates are separate
Host decisions and must never be collapsed; independent-review
remediation):

| Case                     | Registered? | Visible? | Policy decision                 | Tool executes? | Result status | Event                                           | Provider recovery?               |
| ------------------------ | ----------- | -------- | ------------------------------- | -------------- | ------------- | ----------------------------------------------- | -------------------------------- |
| unregistered Tool        | no          | n/a      | n/a (lookup fails)              | no             | `failed`      | `tool_failed`                                   | yes (next turn)                  |
| registered but hidden    | yes         | no       | n/a (surface gate)              | no             | `denied`      | `tool_failed`                                   | yes (next turn)                  |
| visible, policy deny     | yes         | yes      | deny                            | no             | `denied`      | `tool_failed`                                   | yes (next turn)                  |
| visible, plain Tool, ask | yes         | yes      | ask (no reviewable preparation) | no             | `denied`      | `tool_failed`                                   | yes (next turn)                  |
| visible, policy allow    | yes         | yes      | allow                           | yes            | tool result   | `tool_completed`/`tool_failed`/`tool_cancelled` | n/a (terminal turn or next turn) |

Authority assumptions (every scenario): Host-controlled inputs; provider
is the deterministic fake (or a bounded scripted provider for
denied/hidden/cancelled/authorization paths); default `inspect` policy
unless the scenario declares rules (the authorization scenario declares
its capability rules); no mutation/process/Git/network/Godot authority
is granted or exercised. Harness mechanics follow §8.2 (subject added to
`ALLOWED_SUBJECTS`, bounded input validation in `contract.mjs` +
`harness.rs`, oracle probe composes real `createSiralosApplication` +
Tool Round, candidate composes real Core loop + adapters, corpus version
bump + manifest/digest regeneration, `npm run check:differential` exit 0,
complete repository gate). No expected-output fixture fabrication.

### 13.19 Where each invariant is proven

- **Core unit tests (required)**: single-flight rejection via stepwise
  interleave; tool returns `invalid_input` (regression proves the Tool
  execution entry point WAS invoked and the substantive/effectful work
  body was NOT entered after the validation failure — §13.9); capability
  denial recheck; tool throws → `Failed`; tool returns `cancelled`
  (round stops, tail cancelled, pairing intact); cancellation before call
  N; budget normalization boundaries (undefined/non-finite/negative/
  fractional/0/1/8/32/>32); one-call-one-result invariants (orphan,
  missing, duplicate, reorder); transcript commit rules (mixed turn,
  cancelled round keeps transcript, failed turn commits nothing);
  approved-surface denial; ask-on-plain-tool deny-without-execution;
  registry duplicate/order/case-sensitive lookup/fresh-copy detachment;
  definitions registration-ordered.
- **Adapter tests**: workspace list/read/search Tool adapters over real
  temporary fixture workspaces (typed invalid_input, detached results,
  determinism).
- **Differential scenarios**: the frozen `tool-loop` scenario set in
  §13.18 (range/boundary/ordering/recovery/transcript/authorization/
  display-input/status-mapping parity against the real reference),
  including the `tool-loop.authorization` subcases (allow/deny/ask-plain)
  proving capability deny and plain-Tool ask with zero execute calls, the
  `tool-loop.display-input` matrix proving exact UTF-16-code-unit
  truncation at the 200/201 and multibyte boundaries, and the
  `tool-loop.tool-result-statuses` matrix proving the status → event
  mapping through the real production loop.
- **Security/adversarial tests**: hidden tool never executes; unknown tool
  never executes and acquires no capability; capability deny never
  executes; cancelled/unstarted calls never execute; invalid calls never
  execute; a Tool cannot mutate Host cancellation (type boundary: signal
  has no mutation) nor Host transcript (loop owns history; inputs move);
  result detachment before provider reuse (R7.1 boundary reused).

### 13.20 Measurement plan (after implementation)

Production Rust LOC added per crate (git diff --numstat against the
authorization baseline); new direct dependencies; async runtime yes/no;
threads yes/no; Arc/Mutex/RwLock yes/no; unsafe yes/no; dynamic dispatch
yes/no + justification; `tool-loop` differential scenario count; Core
tool-loop unit test count; Adapter tool test count. No benchmarks: R7.2 is
not identified as a performance-sensitive hotspot.

### 13.21 Security review plan (before R7.2 acceptance)

Explicit verification that: provider Tool proposal grants no authority;
the registry grants no capability; visibility does not imply
authorization; authorization is rechecked immediately before execution; a
hidden Tool cannot execute; an unknown Tool cannot execute;
cancelled/unstarted Tools cannot execute; invalid Tool calls cannot
execute; a Tool cannot mutate Host cancellation or Host history directly;
results are detached/bounded before provider reuse; no later
mutation/process/Godot authority is pulled forward. Additionally frozen:
no automatic Tool retry exists (one provider ToolCall → at most one actual
execution attempt; failure becomes a result and the provider decides the
next proposal), and deterministic execution/result ordering is preserved
end-to-end (registry, proposal, execution, result, transcript, and round
counter orderings from §13.16). Domain-neutral capability acceptance
items (independent-review remediation):

- `siralos-core` contains no Godot/domain-specific capability semantic
  types (no domain capability enum variants, no optional-domain semantic
  capability types);
- adding a future optional-domain capability does not require teaching
  Core that domain's semantics (capabilities are validated opaque
  identifiers; policy and reasons are generic);
- R7.2 does not create a second incompatible capability authority system
  (the R6 `domain::capability` vocabulary remains the Domain host-boundary
  vocabulary; the Tool Loop uses its own domain-neutral `CapabilityId`
  with the reference identifier format).

### 13.22 R7.2 acceptance requirements

The implementation may begin only on this frozen contract; R7.2 is
accepted when: the generic loop/subject exists in `siralos-core::tool`,
adapter workspace tools exist in `siralos-adapters::tool`, the
`tool-loop` subject holds differential parity (all required applicable
scenarios match), the security review checklist passes, the Core
capability representation is domain-neutral (no optional-domain semantic
types; a new Domain capability identifier requires no Core change),
proportional measurement is recorded, and the complete local repository
gate passes. R7.2 verification does not authorize R7.3+.

### 13.22.1 R7.2 acceptance evidence (recorded after implementation)

R7.2 PASS. Executable implementation commits (chronological):
`7d378ad` (Core Tool contracts/registry/round/loop), `14dfa5b`
(read-only workspace Tool adapters), `d965c4b` (transcript unresolved-window
pairing remediation), `1396de4` (`tool-loop` differential subject + corpus
promotion), `0e537c6` (harness integrity hardening), `73db8e8` (security and
recovery regression coverage). The `tool-loop` subject holds differential
parity across all 16 required scenarios (corpus schema 3, corpus version
13, 120 scenario files); `npm run check:differential` exits 0 with
116 applicable required scenarios matching. The complete local repository
gate passes (`npm run check`, `cargo deny check`, `git diff --check`).
Security review §13.21 is PASS, proportional measurement is recorded in
`docs/development/RUST_MIGRATION.md`, and R7.3 remains not implemented and
not authorized by this acceptance.

### 13.23 Implementation sequence (frozen plan)

A plan only — nothing here is executed by this review. The later R7.2
implementation change follows this order:

1. Core Tool contract + registry.
2. Core Tool Round (pairing, invalid calls, cancellation).
3. Core Application Tool Loop (single-flight, provider iteration, round
   budget, history, events, terminal outcomes).
4. Minimal generic adapter Tools (workspace.list/read/search).
5. `tool-loop` TypeScript oracle + Rust candidate subjects.
6. Differential corpus promotion (subject, bounded input schema, canonical
   result validator, corpus version bump, manifest/digest regeneration).
7. Focused tests (Core unit, adapter, security/adversarial per §13.19).
8. Full Rust gate (fmt, clippy -D warnings, tests, check:rust).
9. Differential gate (`npm run check:differential` exit 0).
10. Complete repository gate (`npm run check`).
11. Security/architecture review (§13.21 checklist).
12. Proportional measurement (§13.20).
13. Acceptance/evidence reconciliation (status surfaces + evidence record).

### 13.24 R7.3 pre-port projection oracle correction

The independent R7.3 entry review was interrupted before any Rust
implementation because the TypeScript projection oracle had a Unicode-boundary
defect. Commit `4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc` (`fix(core): preserve
Unicode boundaries in evidence projection`), with parent
`da757958bc3cbfd83a37bfb1346ffcbab304a591`, is the complete executable
correction. It changes only
`packages/core/src/projection/evidence-projector.ts` and
`packages/core/src/projection/projection.test.ts`.

The defect was an arbitrary JavaScript UTF-16 code-unit prefix search inside a
UTF-8 byte bound. A search could stop between the high and low surrogates of a
supplementary scalar, producing a lone surrogate that `TextEncoder` then
encoded as a replacement sequence; the retained text and byte accounting were
therefore not a valid original prefix. The corrected contract enumerates valid
Unicode-scalar boundaries and binary-searches only those candidates by exact
UTF-8 byte length. A supplementary pair is never split. An already-lone
surrogate present in source text remains representable as source data; the
correction prevents the projector from creating one.

The same boundary rule governs bounded lines and total truncation. If an
artificial line bound cannot fit the next complete scalar, the scalar is kept
intact rather than split; the production 1 KiB bound can fit every four-byte
scalar. Total truncation retains the longest complete scalar prefix that fits
with the exact `\n… [truncated]` marker, preserving its explicit marker-only
behavior when the marker itself exceeds the budget. The correction affects
only the detached model-visible `ModelEvidenceView`/projected message copy;
raw `ToolExecutionResult` data, Host history, workspace/tool authority,
capabilities, and provider authority are unchanged.

The correction is covered by focused regressions for a supplementary scalar at
a line boundary, a sub-scalar line bound, truncation immediately below and at
the inclusion threshold, no surrogate-pair split, and retained scalar order.
The focused projection command passed 3 test files and 52 tests. The
differential corpus remains unchanged: schema version 3, corpus version 13,
120 scenario files, digest
`6a5be95acb3ff8a714da39aef206770796987ff8910dc9bd8dd58f4b72246490`.

This is a pre-port oracle correction only. No R7.3 Rust implementation,
projection subject, corpus promotion, or R7.3 authorization is included. The
corrected TypeScript oracle is the source for a restarted independent R7.3
entry review; at that closure point R7.3 remained pending review and
authorization. The completed entry review is recorded below.

## 14. R7.3 Entry Review — Projection Contract

Status: **PASS — R7.3 contract frozen; integrated oracle remediation and
terminal-marker precedence pinned; independent review PASS — authorized as
next implementation slice**.
This section records the independent entry review against the corrected
TypeScript oracle and the follow-up independent review of the integrated
remediation lineage. The contract is frozen; R7.3 is authorized as the next
implementation slice. This record does not contain the Rust implementation
itself, promote a differential corpus subject, or mark R7 Verified.

### 14.1 Scope, entry state, and audit result

The review audited every file under `packages/core/src/projection/`, including
capacity, estimation, pressure, context segments, conversation reduction,
evidence projection, Tool projection, projection composition, cache, and stale
result helpers. It also audited `context/projection.ts`,
`context/phase-contract.ts`, the provider-turn/application/event seams,
`tool-registry.ts`, capability/permission/profile policy, CLI context
observability, all projection/context tests, the projection behavior suite,
application/provider protocol tests, Tool-loop tests, instructions/knowledge
behavior tests, and every repository caller of the projection functions.

The verified local entry state was:

| Item                                            | Value                                                              |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| Branch                                          | `main`                                                             |
| Starting HEAD and upstream                      | `1ffc3f6411a0386c270789af67917aa7b57b6f93`                         |
| Starting worktree                               | clean                                                              |
| Historical R7.2 Rust implementation baseline    | `73db8e89c8f670454927ca7ed7554e17d33ea606`                         |
| Entry-review verified executable repository SHA | `4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc`                         |
| Integrated line-bound SHA                       | `461f290b3d3d778a3bef4d25a895338efcdf315c`                         |
| Current verified test SHA                       | `ea145a14a89fb5e6b9e2988eddb97d65d2e37793`                         |
| Entry-review docs closure                       | `1ffc3f6 docs: record R7.3 projection oracle correction`           |
| Corpus                                          | schema 3, version 13, 120 scenario files                           |
| Corpus digest                                   | `6a5be95acb3ff8a714da39aef206770796987ff8910dc9bd8dd58f4b72246490` |
| R7.3 executable Rust                            | absent; no R7.3 source or implementation commit exists             |

At the entry review, the only commit after the verified executable correction
was the documentation closure above. The full starting `npm run check` gate
exited 0: 211 TypeScript test files passed (3,192 tests, 35 skipped), all 116
applicable required differential scenarios matched, and the Rust format,
clippy, and workspace tests passed. The later independent review found the
integrated line-bound defect recorded in §14.4 and it is corrected by the
current executable remediation SHA above.

### 14.2 Co-located classification table

The following table is the complete R7.3 boundary. “MUST PORT” means the
observable, provider-neutral behavior is part of the next Rust parity slice.
“GENERIC SEAM ONLY” means R7.3 consumes a bounded, typed, host-owned input but
does not port the producer or its domain semantics. “LATER” names the owning
milestone. “DO NOT PORT” is TypeScript implementation structure with no
independent Rust contract.

| Behavior                                   | Classification                      | Frozen boundary and owner                                                                                                                                                                                                                      |
| ------------------------------------------ | ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Context capacity                           | **R7.3 MUST PORT**                  | `advertisedMaximum` and `verifiedMaximum` are informational and currently `null`; `workingMaximum` is the authority; `maxOutputTokens` is carried but does not enter projection arithmetic.                                                    |
| UTF-8 token estimator                      | **R7.3 MUST PORT**                  | Empty text is 0; otherwise `ceil(UTF-8 byte length / 4)`.                                                                                                                                                                                      |
| Conversation item estimator                | **R7.3 MUST PORT**                  | Count UTF-8 bytes of string fields `content`, `summary`, `message`, `toolName`, `callId`; JSON bytes of `input`; JSON bytes of object `output`; and nested `result.summary`/`result.message`. Non-serializable structured values contribute 0. |
| Pressure classification                    | **R7.3 MUST PORT**                  | Deterministic ratio/state classification against the working maximum with inclusive `0.70`, `0.85`, and `1.00` thresholds.                                                                                                                     |
| Automatic reduction                        | **R7.3 MUST PORT**                  | On initial `auto` or `hard`, reserve system and projected Tool tokens, trim only the disposable message copy, then recalculate pressure.                                                                                                       |
| Hard blocking                              | **R7.3 MUST PORT**                  | A final `hard` result produces a typed blocked outcome and no provider call.                                                                                                                                                                   |
| Context segment model                      | **R7.3 MUST PORT**                  | Owned value fields are `id`, stability, title, content, content bytes, and content token estimate.                                                                                                                                             |
| Stable/contextual/volatile classes         | **R7.3 MUST PORT**                  | The three generic classes are the only classification needed by R7.3; volatile data is observable but is not in the stable/contextual provider system prefix.                                                                                  |
| Segment ordering                           | **R7.3 MUST PORT**                  | Sort by stability rank `stable`, `contextual`, `volatile`, then segment id using the TypeScript default string/code-unit comparison.                                                                                                           |
| Segment serialization                      | **R7.3 MUST PORT**                  | Each segment is `[Title]\ncontent`; segments join with `\n\n`.                                                                                                                                                                                 |
| Stable fingerprint                         | **R7.3 MUST PORT**                  | SHA-256 of canonical JSON over stable segments only, retaining ordered `{id,title,content}` values and excluding contextual/volatile segments.                                                                                                 |
| Stable-prefix byte accounting              | **R7.3 MUST PORT**                  | `stableBytes` is serialized stable content; `stablePrefixBytes` is serialized stable plus contextual content; both use exact UTF-8 bytes.                                                                                                      |
| System-instruction composition             | **R7.3 GENERIC SEAM ONLY**          | The Host supplies stable instruction text/segments. The historical Godot/GDScript `SIRALOS_SYSTEM_INSTRUCTIONS` body is not a Rust Core constant.                                                                                              |
| Task-contract segment                      | **R7.3 GENERIC SEAM ONLY**          | R7.3 composes a bounded rendered request/contract value supplied by the Task owner; Task identity and contract authority stay with the task runtime.                                                                                           |
| Task-state segment                         | **R7.3 GENERIC SEAM ONLY**          | R7.3 consumes a rendered state snapshot; it does not own TaskState transitions, acceptance, evidence authority, or progress.                                                                                                                   |
| Latest task evidence segment               | **R7.3 GENERIC SEAM ONLY**          | The Task runtime supplies the latest bounded evidence identity/revision reference; R7.3 never becomes the evidence authority or stores raw evidence.                                                                                           |
| Plan segment                               | **R7.3 GENERIC SEAM ONLY**          | The Host supplies only the current immutable plan/rendering; plan validation, approval, and staleness remain planning/task owners. The current rendered plan cap is 4 KiB.                                                                     |
| Executor brief                             | **R7.3 GENERIC SEAM ONLY**          | R7.3 consumes the current bounded compiled brief; briefing/compiler semantics remain the executor owner. The current cap is 4 KiB.                                                                                                             |
| Project instructions                       | **R7.3 GENERIC SEAM ONLY**          | The instruction resolver supplies a selected, rendered set; R7.3 does not discover files or promote instructions to authority.                                                                                                                 |
| Pinned/retrieved project knowledge         | **R7.3 GENERIC SEAM ONLY**          | The knowledge coordinator owns facts, pinning, retrieval, expiry, and secret isolation; R7.3 consumes bounded rendered facts.                                                                                                                  |
| Reference evidence segment                 | **R7.3 GENERIC SEAM ONLY**          | Reference identity/materialization owns reference revisions and evidence; R7.3 consumes safe rendered views, at most four recent entries.                                                                                                      |
| Research evidence segment                  | **R7.3 GENERIC SEAM ONLY**          | Research owns source access, denial, fetch, and normalization; R7.3 consumes bounded rendered evidence, at most four entries and 4 KiB per formatted entry.                                                                                    |
| Scene evidence producer and semantic types | **LATER MILESTONE / SLICE — R8/R9** | Godot scene/resource intelligence, GDScript semantics, and scene evidence production are not R7.3.                                                                                                                                             |
| Bounded scene-evidence segment             | **R7.3 GENERIC SEAM ONLY**          | R7.3 may consume an already-rendered bounded scene view, at most four recent entries; no Godot type enters `siralos-core::projection`.                                                                                                         |
| Tool visibility projection                 | **R7.3 MUST PORT**                  | Registered order plus mode/surface ceiling plus Host permission decision maps to available, gated, or hidden. Visibility never grants authority.                                                                                               |
| ProjectedRequest/provider composition      | **R7.3 MUST PORT**                  | Projection owns the one transformation from authoritative Host values to a provider-neutral request; R7.1 receives it and the provider does not recompute it.                                                                                  |
| R7.2 ApprovedToolSurface coupling          | **R7.3 MUST PORT**                  | Provider-visible definitions and approved names are derived from one visible Tool list; R7.2 remains the execution and per-call recheck owner.                                                                                                 |
| ProjectionMode value                       | **R7.3 MUST PORT**                  | The current route modes are `generic`, `development`, `review`, `inspection`, and `planning`; Rust should use a small validated/opaque mode value, not domain semantics.                                                                       |
| Mode capability rules                      | **R7.3 GENERIC SEAM ONLY**          | R7.3 consumes Host-supplied allowed capabilities or already-evaluated R7.2 permission decisions; current Godot-oriented TypeScript tables are composition policy, not Core domain.                                                             |
| Mode Tool-name rules                       | **R7.3 GENERIC SEAM ONLY**          | R7.3 consumes exact allowed Tool names from the Host surface. It must not hard-code a new generic policy framework or copy Godot name tables into Core.                                                                                        |
| Development native/mixed Godot gating      | **LATER MILESTONE / SLICE — R8/R9** | `script_only`/`none`/undefined fail closed for native prepare names; `native_only`/`mixed` can expose them only in development. R7.3 preserves this as an opaque host surface input and owns no Godot interpretation.                          |
| Provider Tool-calling compatibility        | **R7.3 MUST PORT**                  | The Host passes provider support; tool-requiring modes fail closed when it is false. Generic mode retains its current non-required behavior.                                                                                                   |
| ToolResult evidence projection             | **R7.3 MUST PORT**                  | Only provider-facing success summaries or failure messages are transformed; status and output remain structurally intact; raw results/history remain authoritative.                                                                            |
| ANSI/control sanitization                  | **R7.3 MUST PORT**                  | Exact corrected text sanitizer behavior is part of the model-view contract.                                                                                                                                                                    |
| Secret redaction                           | **R7.3 MUST PORT**                  | Ordered configured secrets are replaced before reduction; security removal can never be restored by a size rule.                                                                                                                               |
| Repeated-line collapse                     | **R7.3 MUST PORT**                  | Three or more consecutive exactly equal `\n`-split lines become one `${line} ×${count}` line. The never-worse comparison for this step is JavaScript UTF-16 `.length`.                                                                         |
| Line bounding                              | **R7.3 MUST PORT**                  | Ordinary evidence lines use the feasible UTF-8 bound; an unfit scalar and an over-bound terminal marker are the only explicit exceptions.                                                                                                      |
| Final truncation                           | **R7.3 MUST PORT**                  | Use the exact marker `\n… [truncated]`, select the largest valid scalar prefix that fits with it, and retain marker-only output when the marker exceeds the configured bound.                                                                  |
| Never-worse reduction                      | **R7.3 MUST PORT**                  | Repeat collapse is the only optional reduction governed by the never-worse check; if discarded, the mandatory line bound is reapplied to post-security text and retained. Security transforms and final truncation are never reverted.         |
| Evidence bounds                            | **R7.3 MUST PORT**                  | Total evidence 32 KiB UTF-8, one line 1 KiB UTF-8, reference/research/scene combined 12 KiB, four recent records per source, and eight task focus paths. Producer-specific budgets remain producer-owned.                                      |
| Watermark cache                            | **R7.3 MUST PORT**                  | The disposable model-evidence cache is bounded, observable through its size, and never owns durable TaskState evidence. A minimal ordered state value is sufficient.                                                                           |
| High/low eviction                          | **R7.3 MUST PORT**                  | High 64/low 32; cleanup occurs only when insertion makes size greater than 64 and removes oldest insertion-order entries down to 32.                                                                                                           |
| Revision-bound cache invalidation          | **R7.3 MUST PORT**                  | A changed Task contract revision clears disposable views before the next view is stored; raw evidence is not changed or deleted.                                                                                                               |
| RevisionGuard API                          | **SOURCE STRUCTURE — DO NOT PORT**  | The generic TypeScript wrapper is not an independent Rust seam; R7.3 owns the concrete revision comparison needed by the cache.                                                                                                                |
| `awaitCurrent`                             | **SOURCE STRUCTURE — DO NOT PORT**  | It has tests but no real production projection consumer. Do not introduce async infrastructure for this helper.                                                                                                                                |
| `lastProjection` observability             | **R7.3 MUST PORT**                  | Expose a detached typed projection snapshot sufficient for later `/context` and `/tools` rendering; it is disposable and never authoritative.                                                                                                  |
| `/context` data                            | **LATER MILESTONE / SLICE — R7.5**  | R7.3 supplies typed fields; R7.5 owns CLI text and command wiring.                                                                                                                                                                             |
| `/tools` data                              | **LATER MILESTONE / SLICE — R7.5**  | R7.3 supplies typed Tool counts/fingerprint/approved names; R7.5 owns CLI text and command wiring.                                                                                                                                             |
| PhaseContract                              | **LATER MILESTONE / SLICE — R10**   | R10 owns full PhaseContract runtime/readiness parity; R7.3 accepts a mode and generic segments rather than executing phase contracts.                                                                                                          |
| ContextClass                               | **LATER MILESTONE / SLICE — R10**   | R10 owns the declarative context-class artifact model and provenance. R7.3 only retains the three projection stability classes.                                                                                                                |
| PhaseContextSources                        | **LATER MILESTONE / SLICE — R10**   | R10 owns the richer source registry/compiler; R7.3 accepts already-selected values.                                                                                                                                                            |
| `projectPhaseContext`                      | **LATER MILESTONE / SLICE — R10**   | The fixed TypeScript helper remains a host routing seam; it is not a Rust R7.3 context compiler.                                                                                                                                               |
| `toolSurfaceForPhase`                      | **R7.3 GENERIC SEAM ONLY**          | The current fixed host table may supply an opaque mode/surface to R7.3; R7.3 never derives authority from a malformed phase contract. Full phase routing remains R10.                                                                          |
| ICM context classes/compiler               | **LATER MILESTONE / SLICE — R10**   | No Live/Pinned/Frozen model, artifact registry, provenance compiler, or readiness graph is pulled into R7.3.                                                                                                                                   |
| Repository-wide-context invariant          | **R7.3 MUST PORT**                  | Every segment is explicit and bounded; no phase or default projection may silently request repository-wide context.                                                                                                                            |
| Canonical JSON and SHA-256                 | **R7.3 MUST PORT**                  | Reuse the existing domain-neutral identity primitive or prove an equivalent. Do not use ordinary unordered `serde_json` output as a fingerprint ABI.                                                                                           |
| Context-pressure event hook                | **R7.3 MUST PORT**                  | Emit the existing typed `context_pressure` event only from the Host application boundary when the final projected pressure is non-normal; R7.5 renders it later.                                                                               |
| CLI context formatting                     | **SOURCE STRUCTURE — DO NOT PORT**  | `apps/cli/src/output/context.ts` is a presentation renderer; its labels and string formatting are R7.5, not Core projection policy.                                                                                                            |

### 14.3 Domain-neutrality and ownership decisions

The Rust Core contract is generic. No `siralos-core::projection` type may own a
Godot Tool name, Godot capability meaning, scene/resource semantic type,
GDScript instruction, or native/mixed task interpretation. The current
TypeScript mode tables and `SIRALOS_SYSTEM_INSTRUCTIONS` are historical
composition data. A future CLI/domain composition supplies stable instruction
segments, opaque mode/surface values, allowed Tool names/capabilities, and
already-rendered source evidence.

R7.2 remains the owner of `CapabilityId`, permission evaluation, registry
membership, per-call authorization, Tool execution, approval, and the
`ApprovedToolSurface` execution guard. R7.3 composes with that evaluator; it
does not create a second policy owner. R7.3 visibility can only remove a Tool
from a provider request. It cannot add capability or change a R7.2 decision.

TypeScript currently imports canonicalization from the historical
`packages/core/src/godot/digest.ts` module. The behavior is generic: sorted
object keys, preserved array order, JSON string escaping, JSON number/null/
boolean representation, and SHA-256. Rust R7.3 must reuse the existing
domain-neutral `siralos-core::identity` primitive where it is exact, and add
parity tests for any JSON value or Unicode-key ordering that the projection
surface actually admits. The Core contract must not conceptually depend on
Godot digest code.

### 14.4 Corrected Unicode oracle

The authoritative correction is `4b805d4ac0a9eac6d6de5a2b90b64bc6146aeafc`,
`fix(core): preserve Unicode boundaries in evidence projection`; the closure
is `1ffc3f6411a0386c270789af67917aa7b57b6f93`,
`docs: record R7.3 projection oracle correction`. The correction replaced
arbitrary UTF-16 prefix candidates with valid Unicode-scalar boundaries for
both per-line bounding and total truncation.

The exact rule is:

- A valid surrogate pair advances as one scalar and is never split. A lone
  surrogate already present in source data remains a source scalar; the
  projector never manufactures a lone surrogate by slicing a valid pair.
- Line bounds are measured in UTF-8 bytes. Lines are split on LF. The largest
  scalar-aligned prefix that fits is selected by exact UTF-8 byte count. If
  even the first complete scalar cannot fit, that complete scalar is retained
  and the impossible configured bound is exceeded for that scalar rather than
  producing malformed text.
- Total truncation uses the exact marker `\n… [truncated]`. The marker costs
  16 UTF-8 bytes. The largest scalar-aligned prefix whose bytes plus 16 fit is
  retained. For example, six `😀` values at a 19-byte limit produce the
  16-byte marker-only result; at 20 bytes one four-byte `😀` plus the marker
  fits. If the marker itself is larger than the bound, the marker remains the
  explicit result. Exact-limit input is not truncated; an over-limit input is.

The focused correction tests cover a scalar at the 1,024-byte line boundary,
a sub-scalar line budget, total truncation below/at the supplementary-scalar
threshold, and marker-only behavior. The correction changes only the detached
model view; it does not change raw evidence, history, Tool authority,
capability, approval, or provider authority.

#### Integrated line-bound remediation

Independent review of `c474d725f1c66ea78030c382e33fc06382b5728b` found that the
helper-level line bound was not necessarily enforced by the integrated
`projectForModel()` path. The root cause was treating the newline-producing
line split as though it had to be a size reduction, allowing the never-worse
guard to restore the pre-reduction text. The corrected contract classifies
line bounding as a mandatory structural model-view bound, while repeated-line
collapse remains the only optional reduction. Security transforms are
non-revertible; the order is strip controls, redact secrets, optionally
collapse repeated lines, enforce the UTF-8 line bound, then apply final total
truncation. Ordinary projected content obeys `maxLineBytes` for every feasible
scalar bound. If the bound is smaller than one complete scalar, that scalar
remains intact as the established Unicode exception. Final truncation is the
terminal operation and preserves the complete explicit marker
`\n… [truncated]`; its LF-delimited marker line is a second, deliberate
exception when that line cannot fit the configured line bound. No second line
bound is applied after the marker.

The executable remediation is `461f290b3d3d778a3bef4d25a895338efcdf315c`
(`fix(core): enforce integrated evidence line bounds`). Eleven integrated
`projectForModel()` regressions cover ASCII, exact and over-limit boundaries,
supplementary scalars, the impossible sub-scalar bound, security transforms,
repeated-line composition, rejected collapse, and total truncation after line
bounding. Follow-up executable/test commit
`ea145a14a89fb5e6b9e2988eddb97d65d2e37793`
(`test(core): pin truncation-marker line-bound precedence`) adds the integrated
terminal-marker precedence case and the normal marker-fits-line case. The
changes affect only detached model-visible evidence; raw `ToolExecutionResult`
values, Host history, Task evidence, workspace state, capability, and Tool
authority are unchanged. No R7.3 Rust implementation or differential corpus
promotion exists at this documentation commit. The lineage including that
precedence pin and its documentation reconciliation
`8e5384c6b188cbaf314f9e72daa8b89368bbd1c8` has been independently reviewed and
returned PASS (see §14.21).

### 14.5 R7.2 ApprovedToolSurface integration

One projection outcome is the only source for both provider-visible Tool
definitions and the R7.2 approved execution surface:

```text
registered Tools + opaque mode/surface + R7.2 permission decisions
                         |
                    R7.3 projection
                         |
          +--------------+--------------+
          |                             |
  provider request definitions       ApprovedToolSurface.names
  (available + gated, registration    (the same non-hidden names)
   order; hidden absent)              (membership, deterministically exposed)
```

`available` and `gated` Tools are visible. `hidden` Tools are absent from the
provider request, visible Tool fingerprint, and approved surface. A gated Tool
is not silently executable: the R7.2 per-call permission check remains
mandatory, and a plain Tool under `ask` is denied without execution because it
has no reviewable preparation protocol. Prepared approval behavior remains
with R7.2 and its owning effect milestone. A provider proposal for an unknown
or hidden Tool is denied before execution even if it names a registered Tool.

The provider definition vector retains registration order. The approved
surface is a membership set; its canonical observation is sorted by Tool name
so no map iteration becomes observable. The implementation must build both
from the same visible list, not independently re-evaluate mode or permission.

### 14.6 Provider-request ownership and application pipeline

The frozen ownership chain is:

```text
authoritative Host task/history/evidence/policy/capacity inputs
                         |
                      Projection
                         |
             pressure classification and reduction
                         |
                  ProjectedRequest
                         |
                  R7.1 ModelRequest
                         |
                      Provider
```

The provider never trims history, selects Tools, chooses limits, sanitizes
evidence, selects segments, decides relevance, or authoritatively chooses a
projection mode. The Host validates the authoritative transcript before
projection through the existing R7.1/R7.2 boundary. An invalid transcript
blocks the request before provider invocation; projection does not replace
that validation.

For a valid request, normal pressure sends the projected request without a
pressure event. Warn sends it and emits `context_pressure` before the provider
stream. Auto and hard first perform the same deterministic reduction attempt;
the final pressure is recalculated. The current oracle/application emits a
pressure event only when that final state is non-normal. Therefore an auto
reduction that reaches normal proceeds without a pressure event, while a final
auto or hard state emits one. A final hard state emits `response_failed` after
the pressure event and never calls the provider. The hard reason is the exact
projected-token/working-maximum message, with ` (reduction was already
applied)` only when at least one item was dropped.

The typed event is:

```text
context_pressure { state, estimatedTokens, workingMaximum }
```

It is UI-neutral. CLI rendering belongs to R7.5.

When `providerToolCalling` is false, `development`, `review`, `inspection`,
and `planning` are tool-requiring modes and return a blocked `unsupported`
outcome with the exact reason:

> The selected provider route does not support tool calling, which this task
> requires; the session cannot proceed with hidden or missing tools.

The Host still builds the current context and records `lastProjection`; the
unsupported Tool projection is empty, the copied input messages are retained,
`estimatedTokens` is 0, and pressure is classified from 0 against the working
maximum. The provider is not called. `generic` is not in the tool-requiring
set and retains the current route behavior. R7.1 owns the actual model stream
and R7.2 owns the loop; R7.3 supplies the already-projected request only.

### 14.7 Capacity, estimator, and pressure contract

The default capacity is:

| Field               | Exact value/meaning                                     |
| ------------------- | ------------------------------------------------------- |
| `workingMaximum`    | 32,768 estimated tokens; authoritative decision bound   |
| `advertisedMaximum` | `null`; informational only                              |
| `verifiedMaximum`   | `null`; informational only                              |
| `maxOutputTokens`   | 4,096; carried but not in current projection arithmetic |

For text, `estimateTokens("") = 0`; otherwise it is
`ceil(utf8Bytes(text) / 4)`. Conversation estimates sum the exact field set
in the classification table, then apply the same ceiling per item. Structured
JSON byte accounting follows the current `JSON.stringify` boundary. Object
insertion order is not a fingerprint ABI; fixtures must not rely on incidental
key order unless a case explicitly tests the runtime boundary. Fingerprint
canonicalization is separate and always sorts object keys.

Pressure is:

```text
ratio = workingMaximum <= 0 ? 1 : estimatedTokens / workingMaximum
hard  if estimatedTokens >= workingMaximum * 1.00
auto  if estimatedTokens >= workingMaximum * 0.85
warn  if estimatedTokens >= workingMaximum * 0.70
normal otherwise
```

Thresholds are inclusive. Thus just below each threshold remains in the
preceding state; exact warn/auto/hard enters that state; above hard remains
hard. When `workingMaximum <= 0`, every non-negative estimate is hard and the
reported ratio is 1. Zero estimated tokens is normal for a positive working
maximum and hard for a non-positive one. Rust may use integer/rational
comparisons, but the observable classifications and values must match.

### 14.8 Conversation reduction and concrete order

Reduction operates on a detached provider message list only. First, if the
whole list already fits, return a copied list with the original estimate and
zero dropped items without validating it. If it exceeds the budget, validate
the transcript. An invalid transcript is returned unchanged with its original
estimate and zero dropped items; it is not silently repaired or reduced.

For a valid over-budget list, scan in input order. A Tool call is held by
`callId` until its matching result arrives. The call and result are then
treated as one pair: keep both only if their pair estimate fits the cumulative
budget, otherwise drop both. User messages always survive, even if that makes
the final estimate exceed the budget. Standalone assistant messages survive
only when they fit at their encounter point. Other valid items are retained.
The implementation appends a kept pair as `call` then `result`; it never emits
one half. In R7.2-generated histories calls/results are adjacent and pair order
is chronological. For a validator-accepted transcript with multiple pending
calls, the current oracle resolves a pair at its result encounter; that exact
completion-order behavior is part of the parity test, while the call/result
order inside each retained pair remains fixed.

Concrete fixture (the labels are literal two-character contents):

```text
U1 = user_message("u1")                         1 token
A1 = assistant_message("a1")                   1 token
TC1 = callId "c1", toolName "t", input {}      2 tokens
TR1 = callId "c1", toolName "t", summary "r1"  2 tokens
A2 = assistant_message("a2")                   1 token
TC2 = callId "c2", toolName "t", input {}      2 tokens
TR2 = callId "c2", toolName "t", summary "r2"  2 tokens
U2 = user_message("u2")                         1 token
```

The source order is `U1 A1 TC1 TR1 A2 TC2 TR2 U2` and the original estimate is
12 tokens.

| Message budget | Kept order            | Dropped first/estimate                                                                            |
| -------------- | --------------------- | ------------------------------------------------------------------------------------------------- |
| 8              | `U1 A1 TC1 TR1 A2 U2` | `TC2+TR2` is the first pair that cannot fit; dropped 2 items; final estimate 8                    |
| 5              | `U1 A1 A2 U2`         | `TC1+TR1` is tested and dropped before `TC2+TR2`; dropped 4 items; final estimate 4               |
| 1              | `U1 U2`               | Both pairs and standalone `A2` are dropped; `U1` and `U2` survive even though final estimate is 2 |

The example makes the selection rule explicit: the oldest complete pair in the
encounter order is considered first, not the largest pair or a semantic
relevance score. No authoritative Host history, TaskState evidence, or raw
Tool result is ever trimmed.

### 14.9 Context segments, source seams, bounds, and fingerprints

R7.3 owns the generic segment value and composition mechanics. The input
segments are copied, sorted by stability rank then id, and measured from
content bytes. Stable and contextual segments serialize to the provider
`system` prefix; volatile segments remain in the typed projection/observability
snapshot and do not enter that prefix. The request messages are the detached
history with only ToolResult model views replaced; context is not appended as
conversation messages.

Current source composition is preserved as a seam: stable instructions,
project instructions, pinned/retrieved knowledge, task contract/state, current
plan, executor brief, latest task evidence, and reference/research/scene
evidence are all already-owned source values rendered into bounded segment
inputs. The latest reference, research, and scene rings select the four most
recent entries each. Their combined UTF-8 budget is 12 KiB and reduction is
deterministically research, then scene, then reference; empty sections are
omitted. Task focus paths are unique workspace-read paths in encounter order,
bounded at eight, and are an input to instruction/knowledge resolution only.

The current segment accounting is exact:

| Quantity                  | Definition                                                     |
| ------------------------- | -------------------------------------------------------------- |
| Segment `bytes`           | UTF-8 bytes of `content` only                                  |
| Segment `estimatedTokens` | `ceil(content UTF-8 bytes / 4)` except empty → 0               |
| `stableBytes`             | UTF-8 bytes of `serializeSegments(stableSegments)`             |
| `stablePrefixBytes`       | UTF-8 bytes of stable plus contextual serialization            |
| `totalBytes`              | Sum of content bytes for all three classes                     |
| `stableFingerprint`       | SHA-256 of canonical JSON array of stable `{id,title,content}` |

The bound matrix is:

| Resource                          |                                                       Byte bound |                            Count bound | Token estimate                       | Exact-limit / limit-plus-one behavior                                                                                                                                                                                                                                                                  |
| --------------------------------- | ---------------------------------------------------------------: | -------------------------------------: | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Model-visible evidence text       |                                               32,768 UTF-8 bytes |                     one view at a time | `ceil(shownBytes / 4)`               | Exact bytes are retained; an overage enters total truncation. The marker-only exception applies only when a caller supplies a bound smaller than the 16-byte marker.                                                                                                                                   |
| One evidence line                 |                                     1,024 UTF-8 bytes by default |                    every LF-split line | `ceil(lineBytes / 4)`                | A 1,024-byte line stays intact; a 1,025-byte ASCII line is split. A bound smaller than one scalar retains it intact; if the final marker line cannot fit, that explicit marker line also remains whole and may exceed the bound.                                                                       |
| Current plan segment              |                                                4,096 UTF-8 bytes |                       one current plan | `ceil(segmentBytes / 4)`             | Exact content is not truncated; content over the bound uses the explicit truncation marker and scalar-aligned prefix.                                                                                                                                                                                  |
| Executor brief segment            |                                                4,096 UTF-8 bytes |                      one current brief | `ceil(segmentBytes / 4)`             | Same exact-limit and over-limit rule as the plan segment.                                                                                                                                                                                                                                              |
| Reference/research/scene evidence |                               12,288 UTF-8 bytes combined target | four recent views/entries per producer | sum of per-segment `ceil(bytes / 4)` | Exact combined bytes are retained; overage reduces research, then scene, then reference. If a remaining allocation is smaller than the 16-byte marker, the oracle's marker-only exception can exceed that tiny allocation; this is intentional observable behavior, not a second truncation algorithm. |
| Reference evidence                |                     source-rendered bytes within combined target |                               4 recent | source view bytes / 4 ceiling        | Four are retained; a fifth source observation makes the oldest fall out before rendering (`slice(-4)`).                                                                                                                                                                                                |
| Research evidence                 | each formatted entry 4,096 UTF-8 bytes before combined reduction |                               4 recent | formatted bytes / 4 ceiling          | Four are retained; a fifth source observation makes the oldest fall out before rendering.                                                                                                                                                                                                              |
| Scene evidence                    |                     source-rendered bytes within combined target |                               4 recent | rendered bytes / 4 ceiling           | Four are retained; a fifth source observation makes the oldest fall out before rendering.                                                                                                                                                                                                              |
| Task focus paths                  |                                                              n/a |                         8 unique paths | not estimated by projection          | The first eight unique workspace-read paths in encounter order are retained; a ninth is not used for instruction/knowledge resolution.                                                                                                                                                                 |
| Disposable evidence cache         |                                                              n/a |               high 64 / low 32 entries | not estimated                        | Size 64 does not evict; insertion number 65 evicts oldest entries down to 32.                                                                                                                                                                                                                          |

Canonical JSON is not ordinary serialization: object keys are sorted, arrays
retain order, strings use the TypeScript JSON escaping behavior, and numbers,
booleans, and null retain their JSON representation. The existing Rust
identity primitive is the starting point, but R7.3 acceptance must include
Unicode-key and numeric edge cases that are admitted by Tool schemas.

### 14.10 Evidence projection and corrected ToolResult behavior

The provider-visible evidence transform is ordered exactly as follows:

1. Strip valid ANSI CSI sequences and selected C0/DEL controls.
2. Replace configured non-empty secrets in configured order.
3. Optionally collapse runs of at least three exactly equal LF-split lines;
   this is the only transform governed by the never-worse reduction check.
4. Enforce the mandatory per-line UTF-8 byte bound at 1,024 by default, using
   scalar-aligned boundaries and the complete-scalar exception.
5. If optional collapse is discarded because it is worse by the reference
   comparison metric, reapply the mandatory line bound to the post-security
   text. Never disable or revert the structural line bound.
6. Apply total truncation last, with the exact marker and scalar boundary rule.

Sanitization details are frozen. A valid CSI starts with ESC + `[`, ends at a
final byte in `0x40..0x7e`, and is removed. C0/DEL inside a candidate makes
the candidate malformed; the ESC is then removed as a control and subsequent
characters are handled normally. ESC not followed by `[` is removed as a
control. The sanitizer removes `0x00..0x08`, `0x0b`, `0x0c`, `0x0e..0x1f`,
and `0x7f`, while preserving tab, LF, CR, and ordinary non-control Unicode
whitespace. Secret replacement is the fixed token
`███[REDACTED]███`; empty secrets are skipped, and overlapping/prefix secrets
follow configured sequential `split/join` order.

Repeated lines use exact JavaScript string equality and UTF-16 `.length` for
the optional reduction-size test. The marker is `${line} ×${run}`. Line and
total limits use exact UTF-8 bytes, never arbitrary UTF-16 offsets. The
never-worse check may discard an applied repeat collapse when the resulting
representation exceeds the raw `originalBytes`; it may never revert security
transforms, the mandatory line bound, or final truncation. If collapse is
discarded, line bounding is reapplied to the post-security text. The
`bound-lines` label means the retained projection pipeline applied a material
line-bound transformation before later terminal truncation; the
optional-reduction rollback did not discard it. Later truncation may remove the
portion where an inserted separator originally occurred, so the label is not
defined by whether removing line bounding would necessarily change the final
returned string. Discarded candidates do not leave a label. Truncation always
runs after that decision and reports `truncated`, `shownBytes`, `originalBytes`,
and ordered transformation labels. Ordinary final provider-visible evidence
lines obey feasible `maxLineBytes`; the complete-scalar exception applies when
the configured bound cannot fit one scalar, and the explicit terminal marker
line may exceed it only when that marker line cannot fit. The marker remains
whole and no second line-bound pass occurs after truncation. If the marker
itself exceeds `maxTotalBytes`, `shownBytes` may exceed that tiny total budget;
`originalBytes` remains the UTF-8 size of the authoritative raw input.

For a successful `ToolExecutionResult`, only `summary` is projected. Its
`output` and status are retained. For every non-success status, only `message`
is projected. The returned result is a detached result object, while raw
result data and authoritative history remain unchanged. Workspace revision
metadata is read from a successful object output when present for the
disposable model view, never converted into authority. The default evidence
limits are 32 KiB total and 1 KiB per line; plan and brief segments are 4 KiB;
reference/research/scene evidence is 12 KiB combined.

### 14.11 Tool visibility, modes, and fingerprints

The exact generic visibility mapping is:

```text
mode/surface ceiling -> hidden
permission deny      -> hidden
permission ask       -> gated (visible, but still Host-gated at execution)
permission allow     -> available
```

Registered order is preserved. The provider request contains available and
gated definitions only. The visible Tool ABI fingerprint is SHA-256 of
canonical JSON over the visible definitions in registration order, with only
`name`, `description`, and `inputSchema`. Hidden Tool additions/changes do not
change it; visible definition/order changes do.

The current TypeScript host tables are frozen as source behavior but are not
Rust Core domain semantics:

| Mode          | Capability ceiling                                                                                                                                     | Exact Tool-name ceiling                                                                                                                                                                                                                                       |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `generic`     | workspace read/write, Git inspect, Godot inspect/probe/API/diagnose/LSP/development, process, network, reference inspect, research fetch, self inspect | No exact-name list; capability evaluation applies                                                                                                                                                                                                             |
| `development` | workspace read/write, Git inspect, Godot inspect/API/development, reference inspect, research fetch, self inspect                                      | `workspace.read`, `workspace.search`, `workspace.apply_text_changeset`, the Godot inspect/API/development names, reference list/read/search, research repository/Godot docs, self read/search; native prepare names additionally require native/mixed surface |
| `review`      | workspace read, Git inspect, Godot inspect/API/LSP, self inspect                                                                                       | workspace list/read/search, Git status/diff, Godot inspect/API/LSP read names, reference list/read/search, self read/search                                                                                                                                   |
| `inspection`  | workspace read, Git inspect, Godot inspect/API, reference inspect, research fetch, self inspect                                                        | workspace list/read/search, Godot inspect/API read names, reference list/read/search, research repository/Godot docs, self read/search                                                                                                                        |
| `planning`    | workspace read, Git inspect, Godot inspect/API, reference inspect, research fetch, self inspect                                                        | The same read-only workspace/Godot/API/reference/research/self names as inspection                                                                                                                                                                            |

The full current exact-name arrays are the executable source of truth in
`packages/core/src/projection/tool-projector.ts`; R7.3 does not copy those
Godot names into `siralos-core`. A Host composition supplies the exact allowed
names/capability decisions. Development native prepare names are visible only
for `native_only` or `mixed`; script-only, none, or undefined fail closed.
Review never exposes them regardless of surface.

### 14.12 Cache, staleness, and disposable state

The evidence cache is disposable model-view optimization, not an evidence
store. Its key is Tool name plus canonical digest of the raw successful
ToolResult. It uses insertion-order FIFO hysteresis: a set that takes the size
from 64 to 65 removes the oldest entries until 32 remain. Updating an existing
Map key does not make it newest under the TypeScript oracle. Failure views are
not cached. The observable cache surface is `evidenceCacheSize()`; no internal
Map shape is an ABI.

The cache is bound to the current Task contract revision. On first observation
or revision change, disposable views are cleared before the new view is stored.
Raw TaskState evidence, raw Tool results, and Host history are untouched. This
concrete revision binding is MUST PORT.

The exported `RevisionGuard` and `awaitCurrent` wrappers are not separately
ported. The only production consumer is the cache's concrete revision change
check; `awaitCurrent` has no real asynchronous projection caller. The Rust
candidate therefore must not add an async runtime, task, thread, channel,
`Arc`, `Mutex`, or `RwLock` merely to reproduce those TypeScript helper names.

### 14.13 R10/ICM and R8/R9 boundaries

R7.3 owns deterministic projection mechanics and accepts explicit generic
segments/mode/surface/policy inputs. R10 owns the complete Interpretable
Context Model: PhaseContract runtime/readiness parity, ContextClass artifact
registry, provenance/staleness compiler, richer `PhaseContextSources`,
repository-wide policy, and any Live/Pinned/Frozen context model. The current
`projectPhaseContext` and `toolSurfaceForPhase` helpers remain Host routing
seams; they are not a license to build the R10 compiler early. No phase may
default to repository-wide context.

R8/R9 own Godot installation/probe/scene/resource semantic extraction,
GDScript structure, diagnostics, LSP, native/mixed task meaning, and scene
evidence production. R7.3 may carry a bounded generic scene-evidence segment
whose source has already been rendered by those owners. No Godot or GDScript
semantic type enters `siralos-core::projection`.

### 14.14 Typed observability for later `/context` and `/tools`

R7.3 exposes a detached, provider-neutral snapshot with:

- mode and blocked type/reason;
- estimated tokens, working maximum, pressure state, ratio, and pressure
  inputs;
- whether reduction occurred and the dropped-item count (the current
  TypeScript request exposes the result indirectly through the projected
  messages; the typed snapshot makes the observation explicit without
  changing Host authority);
- stable, contextual, volatile bytes and segment lists;
- stable bytes, stable-prefix bytes, and stable fingerprint;
- Tool availability/gated/hidden counts, visible definitions/order, Tool ABI
  fingerprint, and ApprovedToolSurface names;
- projected message/evidence metadata where the caller explicitly requests
  it, never raw private paths, timestamps, or pointer identity.

Current CLI `/context` renders stable/contextual/volatile bytes, estimated and
working tokens, pressure, stable fingerprint prefix, and Tool counts/ABI.
Current `/tools` renders Tool counts and ABI. R7.5 owns those strings and
commands; R7.3 freezes the typed data only.

### 14.15 Differential plan

The future `context-projection` subject is the only differential subject
planned for this slice. The corpus is not changed by this review. The final
scenario set is deliberately compact:

| Scenario id                                   | Required canonical observations                                                                                                                      |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `context-projection.estimate`                 | Capacity fields, UTF-8 byte/token estimates, conversation field accounting, and exact structured-value cases                                         |
| `context-projection.pressure`                 | Inputs, ratio, inclusive threshold matrix, zero/non-positive working maximum, and state                                                              |
| `context-projection.trim`                     | Original/projected messages, pair order, dropped count, final estimate, invalid transcript behavior, and concrete budgets                            |
| `context-projection.hard-block`               | Final pressure, blocked type/reason, pressure event, and provider-called `false`                                                                     |
| `context-projection.segments`                 | Stable/contextual/volatile segments, ordering, serialized system prefix, byte/token fields, stable fingerprint                                       |
| `context-projection.tool-visibility`          | Visibility states, visible definitions/order, approved names, counts, and Tool fingerprint                                                           |
| `context-projection.evidence`                 | Projected ToolResult, transformed text, labels, truncation, byte metadata, scalar cases, and tiny-line-bound whole-marker precedence                 |
| `context-projection.pipeline`                 | Normal/warn/auto/hard application integration, final event order, projected request, and provider-called yes/no                                      |
| `context-projection.unsupported-tool-calling` | Mode, unsupported blocked outcome/reason, empty Tool projection, projection-recorded yes, and provider-called `false`                                |
| `context-projection.fingerprints`             | Stable fingerprint invariance under contextual/volatile changes, Tool fingerprint invariance under hidden changes, canonical key/array/Unicode cases |

The canonical record contains only provider-neutral typed values: estimated
tokens, working maximum, pressure state/inputs, projected messages and drop
count, blocked type/reason, provider-called yes/no, segments/system prefix,
byte counts, stable fingerprint, Tool visibility/definitions/order/approved
names/fingerprint, projected evidence, and evidence metadata. It excludes
absolute paths, timestamps, pointer identity, Rust Debug output, and
unordered collection internals. Cache/revision values are included only in a
scenario when the public cache-size/revision outcome is deliberately under
test.

These scenarios belong in differential coverage because they compare the
TypeScript oracle and Rust candidate at the provider/application boundary;
unit tests alone cannot prove request ownership, event order, provider
non-invocation, or ApprovedToolSurface coherence.

### 14.16 Evidence assignment

| Boundary/gap                                                  | Evidence layer                                           | Required reason                                                                                                         |
| ------------------------------------------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Estimator, pressure, trim, segment values/order/serialization | Core unit + differential                                 | Pure deterministic boundaries need exhaustive edge matrices and cross-language outcomes.                                |
| Evidence sanitization and ToolResult projection               | Core unit + differential + integration                   | Text transforms must match byte-for-byte and must reach the provider-facing copy without changing raw results.          |
| Secret redaction and control removal                          | Security/adversarial + Core unit + differential          | Secrets and stripped controls must never be restored by never-worse logic.                                              |
| Corrected Unicode line bounds                                 | Core unit + differential                                 | A valid scalar boundary is the security/correctness property that motivated the oracle correction.                      |
| Corrected Unicode total truncation                            | Core unit + differential                                 | Marker cost, marker-only output, exact boundary, and supplementary scalar cases must match.                             |
| Unsupported Tool-calling                                      | Integration + differential                               | It proves the Host blocks before provider invocation with the exact typed reason.                                       |
| Warn pressure event                                           | Integration + differential                               | It proves event timing and provider continuation at the application boundary.                                           |
| Auto reduction                                                | Core unit + integration + differential                   | Unit proves selection; integration proves projected request and Host history preservation.                              |
| Hard block/provider-called `false`                            | Integration + differential + security/adversarial        | No provider or Tool effect may occur after a final hard state.                                                          |
| Tool visibility/fingerprint                                   | Core unit + differential                                 | Hidden/gated/available states and deterministic registration order are cross-language ABI behavior.                     |
| R7.2 ApprovedToolSurface coherence                            | Integration + differential + security/adversarial        | The same projected visible list must be used for schema and execution guard; per-call recheck remains tested in R7.2.   |
| Cache/revision invalidation                                   | Core unit + integration; not differential unless exposed | Cache size and revision clearing are public enough to test locally, but internal Map shape is not a cross-language ABI. |
| Stable fingerprint invariance                                 | Core unit + differential                                 | Contextual/volatile changes must not change the stable cache identity.                                                  |
| Tool fingerprint invariance                                   | Core unit + differential                                 | Hidden Tool changes must not change visible ABI; visible schema/order changes must.                                     |
| Segment canonicalization                                      | Core unit + differential                                 | Canonical JSON and SHA-256 are shared identity behavior, not a Rust debug representation.                               |
| PhaseContract/ICM and Godot scene semantics                   | Later milestone                                          | R10 and R8/R9 own the producers and richer policy; R7.3 tests only generic seams.                                       |

### 14.17 Minimal Rust ownership proposal

`crates/siralos-core/src/projection/` is the proposed owner of small,
domain-neutral values and pure functions for capacity, estimation, pressure,
trim/reduction, segments, ordering, serialization, fingerprints, evidence
model views, Tool visibility, projected Tool/request outcomes, and the typed
observability snapshot. It may hold one small explicit disposable cache state
where the public cache-size and revision invalidation contract requires it.

`siralos-adapters` owns no pure projection policy. It may later adapt an
external evidence/source producer into the already-rendered generic seam, but
it does not decide context, pressure, visibility, or authority.

`siralos-cli` owns composition and differential harness wiring only. It does
not reimplement projection policy or CLI rendering in Core. The dependency
direction remains `cli → adapters → core`; no new crate is authorized. No
trait is justified for a single projection implementation, and no async
runtime or synchronization primitive is justified by the TypeScript object
graph.

### 14.18 Security acceptance checklist

- [x] Projection is derived, disposable, reconstructable, and model-visible;
      it never grants capability.
- [x] Projection never grants Tool execution authority.
- [x] Provider Tool definitions and R7.2 `ApprovedToolSurface` derive from
      one projection outcome.
- [x] Hidden Tools are absent from the request, visible fingerprint, approved
      names, and execution path.
- [x] R7.2 permission/approval recheck remains mandatory immediately before
      invocation.
- [x] The provider cannot authoritatively choose projection rules, segments,
      relevance, or pressure limits.
- [x] Authoritative Host history is never trimmed; only a detached request
      copy is reduced.
- [x] Authoritative Tool results and Task evidence are never sanitized in
      place.
- [x] Secret redaction and control stripping cannot be reverted by size rules.
- [x] Line bounding is a mandatory structural bound and cannot be disabled by
      never-worse size logic.
- [x] Ordinary evidence content obeys the mandatory feasible per-line UTF-8
      bound.
- [x] The only permitted over-bound final lines are one complete scalar under
      an impossible sub-scalar configuration or the explicit final
      truncation-marker line when that marker cannot fit the configured bound;
      arbitrary evidence content has no bypass.
- [x] `bound-lines` records a retained pre-truncation line-bound transform;
      later truncation may remove its inserted separator.
- [x] Cache eviction cannot delete durable evidence.
- [x] A stale disposable view cannot become authoritative.
- [x] Core contains no Godot-specific projection semantics.
- [x] R10/ICM is not pulled into R7.3 prematurely.
- [x] All ordering, transforms, bounds, fingerprints, and provider decisions
      are deterministic and time/randomness-free.
- [x] Capacity, per-line, total, record-count, focus-path, cache, and provider
      stream bounds remain explicit.
- [x] No absolute workspace/cache/credential path, timestamp, or private
      pointer is emitted in a provider-safe canonical differential record.

### 14.19 Post-implementation measurement plan

After implementation, record exactly:

- production Rust LOC by `siralos-core`, `siralos-adapters`, and `siralos-cli`;
- direct dependency delta;
- canonical JSON/SHA-256 dependency delta;
- async runtime, threads, `Arc`, `Mutex`, `RwLock`, and `unsafe`: each yes/no;
- dynamic dispatch: yes/no, every use, and its concrete justification;
- stateful projection/cache object count and justification;
- `context-projection` differential scenario count;
- Core projection test count;
- integration/security/adversarial test count;
- any benchmark only if implementation proves a real hotspot; no benchmark
  subsystem is authorized by default.

### 14.20 Implementation sequence (frozen, now authorized)

The frozen implementation sequence is now authorized following independent review PASS
of the integrated oracle remediation lineage:

1. Capacity, estimator, and pressure.
2. Conversation reduction.
3. ContextSegment values, ordering, serialization, and fingerprints.
4. Corrected evidence model-view projection.
5. Generic Tool visibility and ApprovedToolSurface derivation.
6. ProjectedRequest composition.
7. R7.2/R7.1 integration.
8. `context_pressure`, auto-reduction, and hard-block integration.
9. TypeScript oracle and Rust candidate `context-projection` subject.
10. Differential corpus promotion with schema/digest regeneration.
11. Focused Core, integration, security, and adversarial tests.
12. Full Rust format/clippy/test/architecture gates.
13. Differential gate.
14. Full repository gate.
15. Security and architecture review.
16. Proportional measurement.
17. Acceptance/status reconciliation.

This sequence is the authorized implementation order for the next slice.

### 14.21 Authorization

All material projection boundaries are classified; the corrected Unicode
oracle is explicitly frozen; the Host/R7.1/R7.2 ownership chain, generic Core
boundary, R10/R8/R9 deferrals, evidence layers, differential record, security
invariants, Rust ownership, measurement, and implementation sequence are
unambiguous. The original entry review changed no executable file or
differential corpus file; the later executable remediation and precedence pin
are recorded in §14.4.

**PASS — R7.3 contract frozen; independent review PASS — authorized as next
implementation slice.**

Independent review of the remediation lineage
(`461f290b3d3d778a3bef4d25a895338efcdf315c` — integrated evidence line bounds,
`ea145a14a89fb5e6b9e2988eddb97d65d2e37793` — terminal truncation-marker
precedence, and `8e5384c6b188cbaf314f9e72daa8b89368bbd1c8` — final
pre-authorization documentation commit and reconciled §14) has returned **PASS**
on the reconciled contract and its remediations. Specifically confirmed:
(1) integrated evidence line bounding is structural and mandatory,
(2) repeat collapse alone is subject to the never-worse rule,
(3) security transforms cannot be reverted by later reduction decisions,
(4) Unicode scalar boundaries and the impossible-sub-scalar exception remain
correct, (5) final truncation occurs after line bounding,
(6) the complete terminal marker `\n… [truncated]` is the narrow deliberate
second line-bound exception, (7) provider-visible Tool definitions and
`ApprovedToolSurface` derive coherently from one projection while R7.2 retains
per-call authorization, (8) projection is disposable model context and grants
no execution authority, (9) R8/R9 retain Godot-specific semantics,
(10) R10 retains PhaseContract/full ICM/context-compiler work, and
(11) no R7.3 Rust implementation existed at that pre-implementation review
point.

R7.3 Projection parity is complete and evidence-backed (13
projection/application integration tests and 11 required
`context-projection` scenarios). R7.4 Configuration parity is a completed
Rust candidate pending independent completion review (corpus version 15, 133
scenario files, 128/128 applicable required Windows scenarios matching, four
explicit platform skips, one accepted informational deviation). R7.5 has not
started; R8-R12 remain not due; R7 remains Active and is not marked Verified.

## Acceptance gates for R7 (evidence design)

1. All five surfaces traced (this document, §2).
2. Observable behavior separated from TypeScript structure (§3).
3. Host authority boundaries explicit (§2.2, §2.4, §10).
4. Resource limits enumerated (§5).
5. Deterministic ordering enumerated (§6).
6. Cancellation semantics explicit (§7).
7. Failure taxonomy explicit (§4).
8. Differential evidence plan concrete (§8).
9. No required corpus changes leave the repository red (none made in
   extraction; new subjects land with their implementations).
10. Rust ownership assigned without speculative architecture (§10).
11. First slice narrowly defined (§11).
12. Future Profile/Plugin/View/Context architecture not pulled forward (§12).

## Final migration state

```text
R1-R6 — Verified
R7 — Active
R7A — Complete (provider protocol contract corrected; evidence boundary frozen)
R7.1 — Complete (provider contract + deterministic fake provider + bounded
        single model turn at differential parity; corpus version 12, 104
        scenario files, 18 provider-turn scenarios; final acceptance
        including cancellation-authority remediation — providers receive
        only a read-only CancellationSignal and cannot mutate Host
        cancellation state — and proportional measurement)
R7.2 — Complete (generic Application Tool Loop parity; corpus version 13,
        120 scenario files, 16 tool-loop scenarios at differential parity;
        domain-neutral CapabilityId; immutable ordered Tool Registry; Tool
        Round one-call/one-result and cancelled-tail pairing; single-flight
        Application loop; closed R7.2 event surface; security review PASS)
R7.3 — Complete and evidence-backed (13 projection/application integration
        tests; 11 required context-projection scenarios; independent review
        PASS on the contract and implementation lineage)
R7.4 — Completed Rust candidate pending independent completion review
        (corpus version 15; 133 scenario files; 128/128 applicable required
        Windows scenarios matching; four explicit platform skips; one accepted
        informational deviation)
R7.5 — Not started
R8-R12 — Not due
```
