---
id: ADR-0019
status: accepted
domains: [self-reference, capability, doctor]
paths: [packages/core/src/self/**, packages/core/src/doctor/**]
supersedes: []
---

# ADR 0019 — Siralos Self-Reference and Capability Diagnostics

- Status: accepted (Stage 3 milestone 6)
- Date: current milestone
- Related: ADR 0014 (task runtime), ADR 0015 (projection boundaries),
  ADR 0016 (workspace revision and structural reads), ADR 0017 (project
  instructions and knowledge), ADR 0018 (references and research sources)

## Context

Siralos now has many moving parts: providers, sandboxing, Godot
discovery, the Task Runtime, projection, knowledge, references, and
research. Answering "what does the installed Siralos actually support?"
from model training memory is wrong by construction: the model has no
reliable knowledge of the exact installed build, its version, its
registered tools, or its current configuration. Scattered CLI status
commands (`/status`, `/sandbox`, `/permissions`, `/godot-doctor`,
`/references`, `/research-status`) are not a coherent diagnostic surface,
and ad hoc environment probing by the model cannot be trusted to be
read-only, offline, or honest. As Siralos grows more capabilities, it
needs a host-owned way to explain and diagnose its actual installed
capabilities.

## Decision

Introduce three host-owned surfaces plus safe reporting:

1. **Built-in SelfReference (`@siralos`)** — a host-generated, read-only
   documentation surface describing the EXACT installed runtime: version
   and platform identity, the interactive command catalog, the
   configuration surface, capability names, sandbox profiles, the
   registered tool surface, Godot capability status, references/research
   configuration, and Task Runtime concepts. Content is derived from
   authoritative runtime metadata wherever practical (command catalog,
   capability ids, profile ids, tool registry) and is retrieved on
   demand (`self.read` / `self.search`); full documentation is never
   injected into prompts. A stable runtime revision fingerprints the
   installed surface (version + command catalog revision + config schema
   revision + capability schema revision + tool ABI revision). The
   self-reference is NOT an external Reference (untrusted supporting
   material) and NOT model training memory; it contains no secrets and
   has no mutation tool.

2. **Host-owned CapabilitySnapshot** — a structured snapshot of what the
   current runtime can actually do, using an explicit state vocabulary
   (available / configured / unavailable / unsupported / degraded /
   blocked_by_policy / requires_approval / unknown) so that "provider
   supported", "provider configured", "tool registered", "tool
   projected", "backend installed", and "backend usable" are never
   conflated. The snapshot is OBSERVATION, not policy: it grants
   nothing; SandboxBackend, ToolProjector, and the security layer stay
   authoritative.

3. **Deterministic read-only CapabilityDoctor** — an application-owned
   diagnostic service with typed checks over ten areas (runtime,
   configuration, providers, sandbox, workspace, godot, project,
   references, research, capabilities). It orchestrates the EXISTING
   subsystem owners through a `DoctorSources` port (sandbox backend
   inspect, Godot inspector doctor, reference registry, research
   service, ToolProjector, task runtime, config loader, Git, checkpoint
   store) and never re-implements their logic. Default operation is
   offline and non-paid; every probe is bounded by a per-check timeout;
   failures are classified honestly (required sandbox enforcement
   failures are `fail`, never "warn but usable"); recovery/LSP/check-only
   operations are reported separately as approval-required (policy
   rule) and are never triggered by the doctor, with their execution
   state reported truthfully — currently `unavailable` at this stage
   because execution cannot be identity-bound; current task runtime
   snapshots are compared against the current global configuration as
   diagnostic facts and are never mutated. Safe reports (`--report-safe`) and JSON output
   (`--json`, schema-versioned) are deterministic, bounded, and
   sanitized: absolute user paths, credential values, and source content
   are excluded; the safe report is NOT anonymous (OS family, Node
   major, Siralos version remain).

The CLI exposes one implementation behind both surfaces: `siralos
--doctor [area] [--json] [--report-safe]` and the interactive `/doctor
[area]` command, plus `--self` and `/siralos` for the self-reference.
Exit codes are documented: 0 = no failures, 1 = one or more failures,
2 = doctor invocation/config failure; warnings never fail.

## Alternatives rejected

- **Model answers about Siralos from training knowledge**: wrong by
  construction; the installed build is authoritative.
- **Inject the entire Siralos documentation into every system prompt**:
  destroys prompt-cache stability; self-reference is retrieved on
  demand.
- **Let each subsystem (provider adapters, sandbox, Godot, CLI,
  ContextProjector, ToolProjector) diagnose itself independently**:
  creates competing capability inventories; the doctor queries
  authoritative owners through one port.
- **Doctor automatically fixing the environment**: the doctor is
  read-only; remediation is instructions only. No auto-fix in this
  milestone.
- **Default live provider/network probing**: default doctor is offline;
  live probes are not implemented at this milestone.
- **Diagnostic output containing raw config/secrets/absolute paths**:
  reports are sanitized and schema-bounded.
- **Separate duplicate capability-resolution logic**: the doctor reuses
  ToolProjector/policy state via sources; it cannot re-derive
  projection (architecture-enforced).

## Consequences

Benefits:

- reproducibility (installed-version identity + self-reference revision),
- easier support and debugging (safe reports, deterministic JSON,
  documented exit codes),
- better model grounding ("what commands does Siralos support?" is
  answered from the installed runtime, not memory),
- a single authoritative capability-inventory path,
- foundations for future Agent Hub/runtime compatibility and `/evolve`
  diagnostics,
- reduced hallucination about Siralos itself.

Costs:

- diagnostic models (DoctorReport/checks/snapshot/safe report) must be
  maintained and kept synchronized with the authoritative registries
  (command catalog, capability ids, config schema summary — conformance
  tests prevent drift),
- additional tests (unit, behavior, final-boundary effect tests,
  architecture rules),
- the configuration-surface summary is authored once and conformance
  tested against the schema file.
