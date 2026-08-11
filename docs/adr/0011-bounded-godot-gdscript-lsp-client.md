---
id: ADR-0011
status: accepted
domains: [godot, lsp]
paths: [packages/adapters/src/godot/**]
supersedes: []
---
# ADR 0011: Bounded Godot GDScript LSP client (fail-closed at this stage)

Status: accepted

## Context

GDScript intelligence in Solaris is currently per-invocation: `--check-only`
parser runs (ADR 0010) validate one script at a time against a disposable
mirror. Real development needs persistent, project-aware language
intelligence — diagnostics as the project changes, hover, completion, and
go-to-definition — which only a running language server can provide. Godot
4.x ships a GDScript LSP server inside its editor, selectable with
`--lsp-port`.

Running a language server is the most privileged Godot operation Solaris
performs so far: a long-lived editor process that reads project source and
answers protocol requests. The source workspace is untrusted, so the LSP
session must never run against it. The security boundary is a stack, not a
single feature: explicit capability policy + one-time session approval +
disposable project mirror + Godot recovery mode + OS sandbox +
loopback-only LSP + external network denied + credential filtering + source
workspace integrity checking + bounded session lifetime. Recovery mode
alone is never described as the security boundary.

## Decision

### Session architecture

- **The source workspace is never the LSP root.** A disposable project
  mirror (the established ADR 0009 architecture — no second copy
  implementation) is prepared and verified, the recovery-mode editor runs
  against it, and `rootUri` is the mirror URI. Source-workspace file URIs
  never enter LSP initialization; provider/application APIs use
  workspace-relative paths only, and the adapter converts them to mirror
  `file://` URIs with explicit Windows-drive, space, Unicode, and percent-
  encoding handling.
- **The Godot invocation is fixed and recovery-mode mandatory**:
  `<godot> --headless --editor --recovery-mode --path <disposable-mirror>
--lsp-port <allocated-loopback-port>`. `--scene`, `--script`,
  `--import`, `--dap-port`, `--debug-server`, `--build-solutions`, export,
  and quit options are prohibited; the source workspace never becomes the
  project path; no user-supplied Godot arguments exist. The architecture
  check enforces the pairing structurally.
- **The LSP port is Solaris-generated, loopback-only, and dynamic.** The
  allocator binds `127.0.0.1:0`, records the OS-assigned ephemeral port,
  and closes; the OS allocation is the race answer. No fixed shared port,
  no large-range scanning, no UPNP/port forwarding, no provider-controlled
  host or port.
- **One active session.** Only one Godot LSP session exists initially; a
  project or engine fingerprint change terminates/invalidates it, and
  restart requires a fresh approval. Sessions are bounded: 30 s startup
  timeout, 15 s request timeout, 10 min idle timeout, 30 min maximum
  lifetime, 5 s shutdown timeout.
- **LSP mutations are rejected.** `workspace/applyEdit` and
  `workspace/executeCommand` are never implemented; the server-request
  boundary returns MethodNotFound for unsupported requests, and completion
  `additionalTextEdits`/`command` attachments are dropped as data.
- **Capability model**: `godot.lsp` is `ask` in every user-facing profile
  (no public unconditional `allow`). The session-start tool
  (`godot.lsp_session`) is the one-time approval point; one approval
  covers exactly one bounded session. Query tools (`godot.hover`,
  `godot.complete`, `godot.definition`, `godot.lsp_diagnostics`) require
  an active session and return typed `session_required` failures
  otherwise.

### Protocol and transport

- Standard LSP framing (`Content-Length` headers) with an incremental
  parser: fragmented headers/bodies, multiple messages per read,
  malformed/missing/negative/absurd Content-Length rejected, header bound
  32 KiB, body bound 16 MiB, valid UTF-8 JSON required, deterministic
  protocol-error failure.
- Bounded JSON-RPC client: numeric deterministic ids never reused while
  pending, 128 pending-request bound, timeouts, cancellation (with
  `$/cancelRequest`), late/duplicate responses ignored safely, malformed
  responses reported as protocol errors without crashing, shutdown drains
  pending state, close rejects new requests.
- Initialization advertises only what Solaris uses: text-document
  synchronization, diagnostics, hover, completion, definition. Mutation
  capabilities are never advertised.
- Diagnostics, hover, completion, and definition payloads are normalized
  conservatively into the existing provider-neutral models: mirror URIs →
  workspace-relative paths (out-of-mirror URIs rejected), 0-based LSP
  positions → 1-based Solaris positions, bounded counts and sizes,
  control characters sanitized, markup treated as data (never executed or
  rendered), malformed items skipped safely, insertText returned but never
  applied, external definition locations represented conservatively
  without absolute paths.

### Staleness and integrity

- The risk-manifest digest and executable identity are recorded at
  approval time and revalidated before every query; a changed script,
  project file, plugin, or executable marks the session stale, shuts it
  down, cleans the mirror, and requires a fresh approval. No stale mirror
  result is ever returned. Live mirror synchronization is deferred to the
  edit/repair milestone.
- During a session, `.godot` generation inside the mirror is expected;
  authored mirror source files must remain unchanged; mirror changes never
  propagate back to source; on shutdown the source workspace is verified
  unchanged and the mirror is destroyed.
- While a session is active, provider-accessible workspace mutations are
  designed to refuse (`language_session_active`) rather than live-sync;
  Git read-only inspection and API knowledge lookup remain allowed.

### Fail-closed at this stage

Every execution path inherits the ADR 0009 platform blocker: Node and the
pinned sandbox runtime offer no exec-by-handle, no directory-relative
create, and no delete-by-handle primitive. Therefore the LSP server runner
never spawns the editor; the session service refuses with a typed
`unavailable` outcome before a mirror is created, a port is opened, or an
editor is launched; no approval is requested while execution is
unavailable; `/gdscript-lsp` refuses before approval; the live isolation
probe is reported skipped, never passed; and the network-isolation scope
is reported truthfully (`loopback-only` only when the backend enforces a
port-specific loopback rule, `unverified` otherwise — never claimed). The
designed pipeline (prepare → approve → mirror → verify → allocate port →
start editor → connect → initialize → initialized → ready) is implemented
as tested contracts, transport, normalizers, and static preparation, and
becomes operational only after identity-bound execution primitives exist.

### Limits

Message body 16 MiB · header 32 KiB · pending requests 128 · open
documents 256 · diagnostics per document 2 000 · completion items 500 ·
hover content 512 KiB · definition locations 100 · startup 30 s · request
15 s · idle 10 min · lifetime 30 min · shutdown 5 s. Providers cannot
increase any limit; truncation is explicit.

## Consequences

- GDScript intelligence becomes persistent and project-aware in design:
  diagnostics, hover, completion, and definition served by the exact
  selected engine through one bounded loopback session.
- The provider can request only the high-level tools; it cannot approve
  sessions, select ports/hosts, send raw LSP methods, or mutate anything
  through LSP.
- DAP, debugging, scene/gameplay execution, editor integration, and the
  edit/repair loop are explicitly out of scope for this milestone; the
  next narrow task is the LSP-assisted edit/diagnose/repair loop.
- The architecture check now enforces: sockets only inside the approved
  LSP and sandbox adapters, the fixed recovery LSP tuple with no
  DAP/debug/scene/import options, mirror-only paths, Solaris-owned port
  values, and the mutation-method prohibition.
