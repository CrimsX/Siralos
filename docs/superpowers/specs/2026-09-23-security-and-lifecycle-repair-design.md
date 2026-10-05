# Security and Lifecycle Repair Design

Date: 2026-09-23
Status: approved design; implementation in progress and not yet verified complete (see ROADMAP.md §10 for the current evidence)

## Mission

Repair the security, confidentiality, authority, lifecycle, resource-bound, evidence, and documentation defects identified by the repository audit while preserving explicitly accepted product behavior: literal `key:VALUE` credentials, HTTP endpoint support, provider `/models`, and cross-session replay.

The repository threat model treats the workspace/repository and model provider as untrusted. The Host must retain authority over visibility, routing, effects, and state.

## Non-goals and invariants

- Do not reopen typed-unavailable process, command, Git, Godot, or ordinary workspace-mutation surfaces.
- Do not make a profile, skill, model, provider, replay store, or guest domain grant authority.
- Do not treat the advisory `siralos.lock` as provider-route or credential identity.
- Do not expose absolute paths, credentials, raw provider bodies, or untrusted identifiers in report-safe output.
- Do not claim CI or release success without a real external run.
- Do not silently weaken the frozen differential corpus; any intentional behavioral change must be explicit and reviewed.

## Design

### 1. Trust, visibility, and Host authority

1. Add one shared workspace visibility/protected-path policy used by list, read, search, context, skills, and replay surfaces. It covers `.siralos/**`, `siralos.toml`, `siralos.lock`, staging/temp state, `.env*`, key/PEM/SSH material, and platform case/reparse variants.
2. Retain launch-root and parent identity across reads and writes. Reject symlink/reparse parent substitution and use bounded, no-follow/identity-checked operations where the platform permits them.
3. Treat workspace profiles as untrusted input. Preserve their feature set, but require explicit, digest-bound user approval before a workspace profile selects a credential-bearing destination, arbitrary environment credential, replay write, or context opt-in. Trusted user configuration remains separate.
4. Make model enumeration use the effective provider adapter and approved destination. A named provider must not be routed through the generic `/models` endpoint path.
5. Evaluate domain enable/profile gates before component installation or reads. Construct activation authority only from actual Host-owned effective authority. A manifest’s requested capabilities are requests, never grants. Guest bind failures are bounded and redacted.
6. Render selected skills as explicitly untrusted, delimited guidance with conservative identifiers and no authority. Skill text cannot alter Host policy or expand capabilities.

### 2. Provider, replay, and output integrity

1. Introduce a provider-bound secret/URL redactor used before errors, `ProviderEvent::Failed`, headless/stdio output, status/reload text, and replay recording.
2. Make provider terminal states typed and fail closed: malformed success, premature EOF, missing tool IDs, invalid Anthropic blocks, and transport failures cannot become successful completion.
3. Apply dotted tool-name aliases consistently to all adapters and preserve provider-specific tool structure.
4. Bound replay memory from the first response, make flush unconditional and single-owner, verify parent identity, verify each body digest/length, and never persist reflected secrets.
5. Centralize frontend output safety: mask active/stored API keys, sanitize all stderr/headless/reload/domain/parser paths, bound and sanitize model IDs/lists, validate URLs, and reset stateful terminal sanitizers at boundaries.
6. Make provider/model status truthful for public endpoints, unsupported protocol/endpoint changes, fallback providers, and reload results.

### 3. Lifecycle, reload, and resources

1. Resolve credentials once per effective session snapshot; re-resolve environment references on reload or explicit environment revision. Credential removal and rotation must have truthful `Applied`, `Unsupported`, or `RestartRequired` outcomes.
2. Bind profile reads, writes, and reloads to exact bytes/digests. Reject concurrent replacement. Preserve startup authority and intentional live route swaps, but report policy/plugin/context changes as restart-required rather than applying stale state.
3. Move provider request/header/body work behind abortable transport/worker boundaries. Observe live cancellation during initial headers and body. Shutdown cancels before signaling/join and uses a bounded join policy. Evaluation enforces wall deadlines on every event.
4. Fix TUI/stdio key handling, active Ctrl+C, sticky interrupt state, terminal I/O errors, sanitizer lifecycle, and cross-turn state.
5. Bound prompts, history, TUI pending/reasoning buffers, context/skills traversal, and replay recording. Enforce owner-only permissions and root/parent identity for profile/replay/temp writes. Use UTF-8-safe truncation everywhere byte limits apply.

### 4. Evidence, CI, and documentation

1. Verify frozen oracle/input identity before building candidates; bind and report separate oracle/candidate identities. Sanitize child environments, use trusted run-owned temp/output roots, enforce no-follow output paths, and preserve unresolved/extra records.
2. Repair clean-checkout CI ordering and stale workflow commands. Do not claim a release gate until a real workflow succeeds and the license blocker is resolved.
3. Reconcile `SECURITY.md`, README, architecture/status documents, provider/skill/domain contracts, and semantic documentation checks with the accepted behavior and new boundaries.

## Implementation slices and acceptance criteria

### Slice A — containment and authority

- Shared secret/protected classifier covers list/read/search/context/skills/replay.
- Root and parent substitution tests fail closed.
- Untrusted profile cannot silently select arbitrary env credentials or an unapproved remote destination.
- `/models` cannot cross from a named provider to a generic arbitrary endpoint.
- Domain activation cannot self-grant requested capabilities; bind-time `.env` sentinel cannot reach output.
- Skills are untrusted and cannot alter capability policy.

### Slice B — provider, replay, and output

- Reflected credential/URL/body sentinels are absent from errors, headless output, status, reload, and replay artifacts.
- Malformed/premature provider and replay data fail rather than complete.
- Replay memory, count, body, parent, digest, and flush tests pass.
- TUI masks active/completed credentials and hostile model IDs/endpoint URLs.

### Slice C — lifecycle and resources

- Credential rotation/removal and profile-writer races have deterministic tests.
- Blocking provider/header/body and shutdown cancellation are bounded.
- Policy/plugin/context reload behavior is truthful under the preserved restart boundary.
- Prompt/history/TUI/context/skills/replay resource limits and UTF-8 boundaries are tested.

### Slice D — assurance and documentation

- Differential runner verifies frozen inputs before build and binds candidate identity.
- CI definitions are clean-checkout viable and release claims remain gated.
- Documentation and semantic checks match current behavior and accepted decisions.

## Verification protocol

For each criterion, record exact commands and observed output. Use focused regression tests first, then `npm run check`, Rust formatting/clippy/tests, architecture/reachability/differential/supersession gates, and independent acceptance/code review. A check that cannot be rerun after the final edit is `unknown`, not a pass. No live CI, release, fuzz, or network operation is implied by local tests.
