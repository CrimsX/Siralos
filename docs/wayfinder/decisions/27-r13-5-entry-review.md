# Decision — R13.5 Entry Review — CLI Product Composition Contract

**Wayfinder ticket:** [R13 Execution Register](../tickets/22-r13-execution-register.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) (PASS) + R13.4 landed (corpus v27, parity held 226/226 locally)
**Decided:** 2026-08-25 (interactive HITL grilling)
**Status:** **PASS — R13.5 contract frozen as four ordered sub-slices; R13.5a authorized**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors [R10 Entry Review](14-r10-entry-review.md) (one milestone, ordered
> entry-reviewed sub-slices). No implementation lands in this record.

---

## Summary

R13.5 closes the migration's last slice: full slash-command interactive
session composition at **full behavioral parity**, over every seam ported in
R13.1–R13.4. Surface surveyed 2026-08-25: ~75 command dispatch arms in
`apps/cli/src/interactive-session.ts`, ~188 symbols across
`apps/cli/src/{session,input,output}`, briefing-service composition
(`packages/core/src/executor/briefing-service.ts`, 231 lines), four real S3M
manifest constants, plus three seam groups explicitly deferred into R13.5 by
[decision 23](23-r13-2-entry-review.md) (knowledge seeding),
[decision 24](24-r13-3-entry-review.md) (reference access tools, research
access port/tools), and [decision 26](26-r13-4-entry-review.md)
(briefing-service wiring, real manifests' CLI use).

## 1. HITL decisions (2026-08-25)

1. **Split into ordered sub-slices** (mirrors R7.x / R10a–c discipline).
2. **Probe shape: scripted stdin sessions** — the oracle drives the REAL
   TypeScript session composition with scripted command sequences,
   injected clock/ports, and TTY-free deterministic output capture; the Rust
   candidate mirrors behind the same subject name.
3. **Deferred seams land INSIDE the R13.5 contract** as scenario groups in
   the subjects where they belong — no new subject name.
4. **All four real manifests ported as fixture constants** so brief
   compilation is exercised against real milestone content.

## 2. Frozen sub-slice structure

| Slice  | Scope                                                                                                                                                                                                                  | Subjects (scenario groups) | Corpus |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- | ------ |
| R13.5a | Slash-dispatch core: input parsing, command routing, help/status/system/clear/commands surfaces over the real composition                                                                                              | `cli-session` (begins)     | v28    |
| R13.5b | Briefing-service composition + s3m8/s3m9/s3m10/s3m11 real manifest fixture constants                                                                                                                                   | `executor-brief` extension | v29    |
| R13.5c | Deferred seams: knowledge seeding (`knowledge-revisions` group), reference access tools (`reference-identity` group), research access port/tools (`research-policy` group)                                             | existing subjects extended | v30    |
| R13.5d | Full `cli-session` parity closure: remaining ~75-command surface incl. godot/develop/gdscript commands reporting their typed `unavailable` posture, session ordering, sanitizer output boundary, input-queue ownership | `cli-session` closure      | v31    |

Each sub-slice receives its own short entry review freezing its
scenario-level fixtures before implementation (subject names are frozen
here; fixture detail is not).

## 3. Mechanics

- Schema stays `3`; each sub-slice bumps the corpus version at its own
  reconciliation commit.
- Determinism posture: one injected clock per session; ports faked at the
  existing harness seams; no TTY, no ambient environment, no real network;
  sanitizer state machine exercised through `push`+`flush` exactly as the
  R7.5 rubric froze it.
- Fail-closed posture unchanged: mutation/execution/checkpoint/git-status
  style commands report their truthful typed availability; nothing flips
  operational.

## 4. Boundaries — not in R13.5

- No new capability, no operational effect, no Stage 4 work.
- No product feature beyond TypeScript behavioral parity; lean-porting
  discipline applies (behavior, not structure).
- Godot-domain commands render their read-only/unavailable reporting only;
  no engine probes become live beyond what R8/R9 already froze.

## 5. Authorization

**R13.5a is authorized as the next implementation slice** against this
contract; its scenario-level fixture freeze lands in the R13.5a entry
review. R13.5b–d follow in order, each with its own mini entry review.

---

## Self-loop verification

| Criterion                        | Direct evidence                                                                                                                                                                | Status |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| Scope claims grounded in sources | interactive-session.ts dispatch arms counted (~75); session/input/output symbol scan (~188); briefing-service.ts line count; manifest files enumerated — all read this session | pass   |
| Human decided the material cuts  | Four HITL answers recorded verbatim in §1 (split / scripted stdin / inside contract / all manifests)                                                                           | pass   |
| Consistent with prior contracts  | Sub-slice + mini-entry-review pattern mirrors decisions 14 (R10) and 21 (R13); fail-closed boundaries restated from decision 21 §3                                             | pass   |
| No double port                   | Deferred seams assigned to their owning slices' subjects (23/24/26 deferral notes cross-checked); briefing-service wiring assigned to R13.5b per decision 26                   | pass   |
