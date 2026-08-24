# Decision — R13.2 Entry Review — Workspace Guidance Scenarios

**Wayfinder ticket:** [R13.2 Entry Review](../tickets/24-r13-2-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) (PASS) + R13.1 landed (`e2ee231`, parity held 220/220 at corpus v24)
**Decided:** 2026-08-24 (resolver session, interactive HITL grilling over reads of `packages/core/src/instructions/*` and `knowledge/{model,coordinator}/*`)
**Status:** **PASS — R13.2 scenario contract frozen; implementation authorized**
**Self-loop ledger:** 3 criteria, one implementation pass (verification below)

> Mirrors [R13.1 Entry Review](22-r13-1-entry-review.md). No implementation
> lands in this record.

---

## Summary

R13.2 ports the workspace-guidance backbone: the single instruction
resolver (scope applicability, deterministic precedence, structural
conflict surfacing, normalized identity, authority-framed rendering) and
the knowledge coordinator's deterministic core (proposal validation,
no-churn subject evolution, policy-shaped rejection, secret protection,
provenance gating, bounded scored retrieval with traces, pin/retire
semantics, state revisions). Everything runs under an **injected clock**
and injected provenance ports — no wall clock, no filesystem, no network.

## 1. Frozen scenario set (~14 cases, corpus v25)

### `instructions-resolution` ×7

| #   | Case                                 | What it proves                                                                                                                |
| --- | ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------- |
| 1   | precedence-ordering                  | root (30) outranks directory scopes (40+depth); resolved order is most-authoritative-first                                    |
| 2   | scope-applicability                  | scoped instructions apply beneath their path only; null-scope applies everywhere; trailing-slash/`./` normalization           |
| 3   | conflict-detection                   | same-layer+same-scope differing content surfaces a conflict (sorted ids, exact reason); identical normalized content does not |
| 4   | content-normalization-identity       | CRLF/tab/trailing-space/blank-run differences collapse to one `instr_` id; real content changes re-identify                   |
| 5   | id-and-revision-determinism          | resolved-set revision changes on content or sourceRevision change; stable otherwise                                           |
| 6   | rendering-authority-framing          | rendered output leads with the never-grant-capabilities framing; conflicts are surfaced with reason + ids, never dropped      |
| 7   | inventory-revision-order-insensitive | the inventory digest sorts internally                                                                                         |

### `knowledge-revisions` ×7

| #   | Case                       | What it proves                                                                                                                                         |
| --- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | propose-accept-shape       | accepted fact carries a `kf_` id, ADR-0028 canonical content digest, revision 1, retrieved activation                                                  |
| 2   | subject-evolution-no-churn | normalized-identical reproposals return `unchanged`; changed content creates revision 2 with retained history                                          |
| 3   | policy-shaped-rejection    | permission/sandbox-shaped claims are structurally rejected with the exact reference reason                                                             |
| 4   | secret-protection          | candidates containing an injected known secret are rejected                                                                                            |
| 5   | provenance-gating          | `workspace_file` fails closed when the file port rejects; `research_evidence` requires the host-verified research port; both accept when ports confirm |
| 6   | retrieval-scoring-trace    | deterministic selection over subject/keyword/freshness scoring with match reasons, budget omission counts, pinned/expired exclusion, recorded trace    |
| 7   | pin-limits-and-retire      | pinned-set bounds reject overflow with reasons; retire removes a subject from active/retrieval while history remains; state revision tracks all of it  |

Knowledge-seeding is explicitly deferred to R13.5 (HITL decision
2026-08-24).

## 2. Mechanics

- Corpus bumps **v25** at the R13.2 reconciliation commit; schema stays 3.
- Probes follow the established pattern; the coordinator is constructed
  with an injected fixed clock and stub ports so records are
  byte-stable cross-platform.
- Acceptance mirrors R13.1: all applicable required scenarios at byte
  parity plus focused Rust unit tests and the full local gate on the
  reconciliation tree.

## 3. Boundaries — not in R13.2

- Instructions remain guidance: they can never grant capability or alter
  security/policy surfaces (the resolver has no such inputs).
- Knowledge stays factual context: the structural policy-shape rejection
  is not an AI safety classifier, and facts never enable tools.
- Seeding, CLI projection wiring, and `/instructions` //knowledge`
  rendering belong to later slices.
- No filesystem access inside scenarios: instruction sets are built
  through the reference constructors from fixture data.

## 4. Authorization

Implementation of R13.2 is authorized against this frozen set; landings
are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                               | Status |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | §1 cites `resolveInstructionSet`/`compareInstructions`/`detectConflicts`/normalization/id functions and the coordinator's propose/retrieve/pin/retire surface verified by reading the TS sources this session | pass   |
| Determinism posture preserved                | §2 injects clock + ports; §3 excludes live inputs; scoring constants are reference-frozen                                                                                                                     | pass   |
| Human decided the scope cut                  | HITL answer 2026-08-24: approved as proposed, seeding deferred to R13.5                                                                                                                                       | pass   |
