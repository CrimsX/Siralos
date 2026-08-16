# Stage 3R R7 — Provider, Tool Loop, Projection, Configuration, and CLI Behavior Extraction

Status: behavior-extraction record for Stage 3R R7 (implementation not yet
started). The TypeScript implementation remains the behavioral oracle until
R12; behavioral parity does not require structural parity (ADR 0032).

This document is the R7 acceptance/evidence design. It freezes the observable
TypeScript behavioral contract for the five R7 surfaces so that the Rust R7
implementation is mechanical rather than exploratory. It ports no Rust code.

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

- All Godot tools and Godot config semantics (R8/R9); real providers, provider credentials, model routing (R11/ADR 0036 Profile); mutation application, process execution, Git execution (R11 — fail-closed unavailable surfaces already exist in both implementations and only need typed parity, which R4 started); references/research/self-reference tools as effect surfaces; the canonical Context compiler (R10).

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
(≈ 13 scenarios; `result-detach` is a family). Later scenarios for complete
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
                   normalization, tool visibility projection, config
                   validation semantics (Host-owned), failure types
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
- Differential: the ≈ 11 minimal `provider-turn` scenarios (§8.1), corpus
  version bump + manifest regeneration; `npm run check:differential` green.
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
R7A — Complete (provider protocol contract corrected; evidence boundary frozen)
R7.1 — Ready to begin
R8-R12 — Not due
```
