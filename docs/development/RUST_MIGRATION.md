# Siralos Rust migration track

Status: authoritative migration sequence.

Stage 3R is an internal migration track. It does not replace Siralos's six
public product stages. The TypeScript implementation remains the behavioral
reference until R12's retirement gate is satisfied.

External master-handoff identifiers remain reserved and explicitly
unverified until their normative source text is checked in; see the
[handoff traceability register](HANDOFF_TRACEABILITY.md). Similar repository
concepts never satisfy those identifiers by implication.

## Status vocabulary

- **Verified**: implemented, wired, and supported by executable evidence.
- **Remediation**: implemented surface exists, but a blocking acceptance or
  verification defect remains.
- **Not due**: sequenced after the active milestone.

## R1-R12 sequence

| Milestone | Scope                                                                                                                           | Current status |
| --------- | ------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| R1        | Siralos rename, Rust workspace, and engineering standards                                                                       | Verified       |
| R2        | Differential Behavioral Harness                                                                                                 | Verified       |
| R3        | Domain-neutral core: authoritative task, state, acceptance, evidence, identity, and transition semantics                        | Not due        |
| R4        | Generic workspace and project foundation: reads, revisions, search, prepared effects, checkpoints, and optional Git integration | Not due        |
| R5        | Generic language intelligence: diagnostics, symbols, definitions, references, structural parsing, and validation                | Not due        |
| R6        | Minimal domain capability architecture and synthetic conformance domain                                                         | Not due        |
| R7        | Provider, tool-loop, projection, configuration, and CLI parity                                                                  | Not due        |
| R8        | Optional Godot Stage-2 parity                                                                                                   | Not due        |
| R9        | Optional Godot Stage-3 parity                                                                                                   | Not due        |
| R10       | H1 content identity, H2 determinism/replay, ICM context, and H3 runtime-readiness parity                                        | Not due        |
| R11       | Full differential, effect-boundary, security, recovery, and cross-platform parity                                               | Not due        |
| R12       | TypeScript reference retirement or an explicit evidence-backed retention disposition                                            | Not due        |

R2 is verified: its versioned corpus is mechanically bound by scenario and
overall digests; its reference and candidate runners have symmetric bounded
lifecycle supervision; and its semantic comparator reports typed JSON-path
differences. The audit records source, corpus, and exact protocol-document
provenance plus explicit scenario and per-subject coverage, and the complete
local repository gate passes. The Tier-1 CI matrix reruns parity on Linux,
Windows, and macOS and retains the exact reference, candidate, audit, and
failure records; a failed matrix run blocks integration. R2 verification does
not authorize R3 work or satisfy any later migration or Stage-4 entry gate.

## Porting gate

Every R3-R11 subsystem follows the same sequence:

```text
behavior extraction
-> idiomatic Rust design
-> differential and effect parity
-> security and architecture review
-> measurement
-> milestone acceptance
```

Matching names or independently passing tests are not parity. The real Rust
execution path, failure semantics, authority boundaries, deterministic
decisions, and final effects must be compared with the TypeScript reference.

Refactoring during a port is expected when it removes TypeScript/Node
accidental complexity. Broad refactoring of the reference ahead of its port is
discouraged because it changes the migration oracle without advancing the
candidate.

## Stage 4 entry

Stage 4 begins only after R1-R11, the Stage 1-3 migration audit, R12's
retirement/retention disposition, and
[the Stage-4 entry gate](stage4-entry-gate.md) all pass.

The first Stage 4 capability is generic **Controlled Runtime Execution**. It
supervises a host-authorized process and produces structured runtime evidence;
it is not a Godot-specific launcher. The intended Stage 4 sequence is:

1. Controlled Runtime Execution
2. Runtime Evidence
3. Godot Runtime Adapter
4. Visual Evidence
5. Controlled Interaction
6. QA Workflows
7. Profiling

Godot is the first specialization to consume the generic runtime boundary.
It does not define that boundary in `siralos-core`.
