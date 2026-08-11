---
id: ADR-0015
status: accepted
domains: [projection, context]
paths: [packages/core/src/projection/**]
supersedes: []
---
# ADR 0015 — Context, Tool, and Evidence Projection Foundation

- Status: accepted (Stage 3 milestone 2)
- Date: current milestone
- Related: ADR 0014 (host-owned structured task runtime)

## Context

The Stage 2 loop forwards the raw conversation history, the policy-filtered
tool registry, and tool outputs directly to the provider. That works for a
single interactive flow, but it does not scale to structured task work:

- **Authoritative state is not model context.** TaskContract/TaskState live
  in the runtime, but nothing projects them into the provider turn; the
  model only sees conversation prose.
- **Registered capabilities are not model-visible tools.** Every tool whose
  capability is not denied is exposed in every session, including tools a
  read-only reviewer must never even discover.
- **Raw evidence is not a model representation.** Noisy ANSI output,
  repeated progress frames, secret values, and oversized results reach the
  provider untouched, and nothing bounds the request against a working
  context budget before the provider is called.
- **No prompt-cache discipline.** Volatile values would invalidate stable
  provider prefixes because context is one flat string.
- **No stale-result discipline.** Nothing binds asynchronous helper results
  to the state revision they were computed for.

## Decision

Introduce three provider-neutral application boundaries in
`packages/core/src/projection/`, composed by a host-owned
`ProjectionService`:

1. **ContextProjector** — projects one model turn from structured
   runtime data into explicit stability classes (`stable | contextual |
volatile`). The system prefix serializes stable + contextual segments
   with a stable-only fingerprint; volatile values never invalidate it.
   Context is disposable and reconstructable from TaskContract/TaskState —
   deleting a projection never loses task knowledge.
2. **ToolProjector** — decides visibility (`available | gated | hidden`)
   per session as a projection of (task mode ∩ capability policy ∩
   provider capability). Hidden tools are absent from the provider schema;
   gated tools remain visible but every invocation still passes the
   runtime capability/approval/sandbox enforcement. The projected tool ABI
   has a stable fingerprint. Modes: `generic`, `development` (only the
   GDScript workflow's exact tool set), `review` (read-only), `inspection`.
3. **EvidenceProjector** — the boundary between authoritative raw evidence
   and the bounded model view: deterministic transforms (strip ANSI,
   collapse repeated lines, redact configured secrets, bound lines,
   truncate with explicit disclosure + raw-evidence reference). The
   never-worse rule keeps security transforms and never inflates the
   representation; raw evidence is never modified.

The `ProjectionService` runs the pre-flight pipeline before every provider
call: `project -> estimate (deterministic) -> classify pressure
(normal|warn|auto|hard) -> fit or reduce -> provider`. `auto` performs
deterministic pair-preserving conversation reduction; `hard` blocks the
provider call entirely (no provider rejection as flow control). Tool
calling is a provider capability: task modes that require tools fail
clearly when the route does not support them. The route working budget
(`ContextCapacity.workingMaximum`) is authoritative over advertised
maximums. The `/develop` loop and the independent reviewer are integrated:
the reviewer request is projected through `review` mode (write tools absent
from the actual provider request) and never receives implementer private
state, secrets, approval capability, or the implementer transcript.

Async results are revision-bound (`RevisionGuard`/`awaitCurrent`); stale
results are discarded, never injected into a newer turn. Disposable
model-evidence views live in a high/low-watermark cache that never evicts
durable task evidence. Provider streaming remains bounded by the existing
cumulative per-turn limits (text events, bytes, tool calls), so a
drip-feeding stream cannot live forever; idle-timeout/hard-lifetime timers
are deferred until a real provider requires them (documented invariant).

## Alternatives rejected

- **Provider adapter owns context trimming** — each adapter would make
  relevance/visibility/redaction decisions independently; application
  semantics stay upstream, adapters only encode provider-neutral inputs.
- **Every registered tool always visible** — leaks mutating tools into
  read-only contexts; hidden must mean absent, not "permission denied".
- **Permission denial as the only tool filter** — denial happens at
  invocation; hiding happens at discovery, a strictly earlier boundary.
- **Destructive truncation of authoritative evidence** — raw evidence is
  never modified to save tokens; only disposable model views are bounded.
- **One giant prompt builder** — a single string loses stability classes,
  budgets, and reconstruction; structure is preserved until provider
  conversion.
- **Provider-specific application semantics** — projectors are
  provider-neutral; only encoding differs per adapter.
- **Single hard context threshold only** — normal/warn/auto/hard gives
  staged deterministic responses instead of one cliff.
- **Sleep-based stale-result tests** — deterministic fake scheduling only.

## Consequences

Positive: cache stability (volatile changes do not rewrite the stable
prefix), smaller prompts (working budgets, reduction, bounded evidence),
provider neutrality, a clearer security boundary (projection is not
enforcement), easier future roles/agents and compaction, and behavior
fixtures that test the final provider request rather than projector
internals.

Costs: projection complexity, additional explicit data models, and the
need for effect tests at the final observable boundary (the actual
fake-provider request, the absence of a provider invocation at hard
pressure, task-state bit-identity after projection).
