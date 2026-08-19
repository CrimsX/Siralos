# Decision — Stage 4 Entry — Generic Controlled Runtime vs Godot Adapter Layering

**Wayfinder ticket:** [Stage 4 Entry — Generic Controlled Runtime vs Godot Adapter Layering](../tickets/08-stage4-entry-sequence.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R12 Disposition](../decisions/07-r12-disposition.md) (closed — 8 guardrails, retirement vs retention 5-field template)
**Decided:** 2026-08-18 (resolver session, reads of `docs/development/stage4-entry-gate.md` 17 criteria, `docs/development/RUST_MIGRATION.md` § Stage 4 entry + milestones R8-R12, `docs/adr/0036-lean-product-composition-and-extension-model.md` ADR 0036, `docs/development/PROJECT_CONTEXT.md` §15-§16, `ARCHITECTURE.md` sandbox/process/Godot sections, and the R10-R12 decision artifacts at file:line)
**Status:** Layering frozen — generic-first order, exact entry-gate checks, lean sentence, and Stage 4-6 not-guaranteed confirmation all named; no Stage 4 code.
**Self-loop ledger:** 3 criteria, one implementation pass (verification below)

> Wayfinder **Plan, don't do** — this is a decision, not Stage 4 work. No runtime process is supervised, no Godot adapter is built, no visual evidence is captured. The map's Destination is reached when this decision is closed.

---

## Summary

Stage 4 is **generic Controlled Runtime Execution first, Godot specializations second**. The generic host boundary — bounded process supervision producing structured runtime evidence without unrestricted desktop or network access — is the only capability that may land before any Godot runtime adapter. Godot is the first consumer of that boundary, never its author.

Stage 4 execution begins only after **four things** (in order): **R1-R11 Verified** + **Stage 1-3 migration audit PASS** + **R12 retirement/retention disposition Verified** (this map's R12 template) + **`stage4-entry-gate.md` 17 criteria all PASS** → `PRE-STAGE-4 ASSURANCE: PASSED`. Until that sentence flips, Stage 4 remains `NOT PASSED (deferred)` regardless of how far R10-R11 implementation has progressed on a branch.

Stages 4-6 themselves remain **staged product direction subject to evidence, not guaranteed implementation commitments** — the map does not promise delivery, only the way.

---

## 1. Generic Controlled Runtime Execution must come before Godot specializations

### Freeze: the intended Stage 4 sequence is ordered and generic-first

Per `RUST_MIGRATION.md:719-732` (frozen since the current milestone):

```text
1. Controlled Runtime Execution   — generic host-authorized bounded process supervision + structured runtime evidence
2. Runtime Evidence               — generic evidence lifecycle (budgets, retention, failure taxonomy, reconciliation)
3. Godot Runtime Adapter          — first specialization consuming the generic boundary (not the boundary itself)
4. Visual Evidence
5. Controlled Interaction
6. QA Workflows
7. Profiling
```

And per `RUST_MIGRATION.md:731-732`: "Godot is the first specialization to consume the generic runtime boundary. It does not define that boundary in `siralos-core`."

And per `PROJECT_CONTEXT.md:698-706` (§15): "Stage 4.1 is generic runtime execution, never shorthand for 'run Godot.' It is not due until the Stage 3R migration and entry gates pass."

### What "generic" means (and why it must be first)

| Capability                                          | What it provides                                                                                                                                                                                                                                                                                                                                                     | What it does not provide                                                                                                                                          | Where it lives if it were code                                                                                                                                                                                                                                                   |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Controlled Runtime Execution** (Stage 4.1-4.2)    | Host authorizes a bounded process, supervises it (timeout, output-limit, process-tree termination, sandboxed run-directory), and produces **structured runtime evidence** (exit code, stdout/stderr with truncation, duration, violations, diff against expected outcome) — without granting unrestricted desktop or network access, without opening the game window | No Godot edition knowledge, no scene graph, no input-action mapping, no Godot-specific approval copy                                                              | `siralos-core::runtime` (generic) + `siralos-adapters::process` / sandbox adapter (the same fail-closed primitive from R11 §2.1, now made mechanically enforceable with a directory-relative `mkdirat`-style + delete-by-handle primitive that R4 hardening intentionally lacks) |
| **Godot Runtime Adapter and after** (Stage 4.3-4.7) | Godot-specific specialization of the generic boundary: engine selection rationale consumed as runtime input, Godot project launch via the generic supervisor, visual evidence (screenshots, viewport capture), controlled interaction (bounded input injection), QA workflows (validation + quality gates with runtime evidence), profiling                          | Does not define the process boundary, the run-directory identity, or the failure taxonomy — those are generic and already at parity before the adapter is written | `siralos-adapters::godot` — discovery/profile/selection become runtime inputs, not the boundary's authors                                                                                                                                                                        |

### What would break if the order were reversed

Writing the Godot adapter before the generic boundary would:

- **Enshrine a Godot-specific process model as the Host** — violating `ADR 0036 §1/§4`: "The Host is deliberately **not a plugin**. Users cannot replace these" + "Complexity grows primarily through user configuration, Skills, and explicitly installed Plugins — not by expanding Siralos Core."
- **Duplicate the fail-closed primitive** — the generic run-directory identity primitive is host-level; a Godot-first adapter would re-implement it Godot-specifically and later need de-duplication (the exact rework R4 hardening was designed to prevent — `SECURITY.md:210-224`).
- **Skip the measurement step** — `R11 Gate` measurement discipline (`RUST_STYLE.md:568-589`) says benchmarks appear only where a hot spot is measured; a Godot-first runtime would lack the generic evidence lifecycle that makes hot spots observable.

**Therefore:** the R10-R11 map's generic seams (`siralos-core::runtime`, `siralos-core::identity`) are not optional pre-work for a Godot runtime — they **are** the runtime's foundation. The Godot adapter is the first specialization to consume that boundary, the boundary itself is `Stage 4.1-4.2` and is Godot-free.

---

## 2. Exact entry gate that must pass after R12 (and the lean product sentence that governs it)

### 2.1 The Stage-4 entry gate — 17 criteria in `stage4-entry-gate.md`

This decision does not restate the gate verbatim — the **authoritative gate is the file itself** (`docs/development/stage4-entry-gate.md`, Part 27, re-evaluated at each milestone, `PRE-STAGE-4 ASSURANCE: NOT PASSED (deferred)` at HEAD `3814e1f`). The ordered check an executor must run is:

```text
R1-R11 Verified
    ↓
Stage 1-3 migration audit PASS  (stage4-entry-gate.md criterion 1 — the differential audit at full corpus)
    ↓
R12 disposition Verified         (this map — decisions/07-r12-disposition.md: retirement or retention, 8 evidences + 8 guardrails)
    ↓
stage4-entry-gate.md 17 criteria all PASS
    ↓
Stage 4 execution may begin
```

All four arrows are enforced by `RUST_MIGRATION.md:713-717`: "Stage 4 begins only after R1-R11, the Stage 1-3 migration audit, R12 retirement/retention disposition, and the Stage-4 entry gate all pass. The first capability is generic Controlled Runtime Execution."

At HEAD ⋯ criteria 1, 3-6 are `NOT MET` by design — the migration gaps themselves (R4+ ports pending). Criteria 2, 7, 9-10, 12-17 are `PASS` / `PASS (scoped)` / `PASS (current surface)` where executable; 8 and 11 are `PARTIAL` until later ports. The remaining criteria are owned by R8-R12 milestones — no additional general hardening milestone is planned (gate's own note). **Stage 4 execution is therefore Not started** (`ROADMAP.md:361-363` + `stage4-entry-gate.md` verdict) and must remain deferred even while this map completes.

### 2.2 The lean product sentence that governs the ordering

Per **ADR 0036 lean product, composition, and extension model** (frozen constitution, pre-R3) and re-affirmed by `AGENTS.md:19` / `ARCHITECTURE.md:19` lean freeze:

> **"Siralos is a minimal, declarative AI coding harness with an inspectable execution environment, composed as: small privileged Host + declarative Profile + inspectable Context + declarative Skill + capability-scoped Plugin + bounded Run + measured Evolve workflow. Complexity grows primarily through user configuration, Skills, and explicitly installed Plugins — not by expanding Siralos Core."** (ADR 0036 § Decision)

And:

> **"The Host is deliberately not a plugin. Users cannot replace these"** + **"0 plugin marketplaces"** (ADR 0036 §1 + §36) + `AGENTS.md:25-26` `"Do not add a Godot package, placeholder domains, or a marketplace/plugin ecosystem."` (enforced by `scripts/check-rust-architecture.mjs:29-31 FORBIDDEN_CORE_SYMBOL_PATTERN`).

Applied to this layering: the generic runtime boundary is **Host** (non-replaceable, non-optional — `PROJECT_CONTEXT.md:698-706`); the Godot adapter is the first **capability-scoped Plugin** (explicitly installed via `siralos-core::domain` lifecycle, versioned `siralos:domain-abi@1.0.0`, never auto-installed — see fact sheet `decisions/03-godot-boundaries.md` §4). The Stage 4 sequence is generic Host before optional Plugin, matching the product vision diagram `USER CONFIG → SIRALOS HOST → OPTIONAL PLUGINS` in `ARCHITECTURE.md`.

---

## 3. Stages 4-6 remain staged product direction subject to evidence, not guaranteed commitments

This map does not promise delivery of Stage 4, Stage 5, or Stage 6. The roadmap's own status vocabulary governs ( `ROADMAP.md:6` head + §5-§6 tails):

| Stage                                                             | Status vocabulary on this map                                                                                                                                                                                                                                                                                                       | What that means for an executor                                                                          |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| **Stage 4 — Controlled execution** (lean vision, ADR 0036)        | **Not started. Stage 4 begins only after the Stage 3R migration and the pre-Stage-4 entry gate pass.** (`ROADMAP.md:361-363`) Its first milestone is generic Controlled Runtime Execution; the Godot runtime adapter is the first specialization, not the boundary itself.                                                          | Do not start Stage 4 work until the gate flips to `PASSED`. This map completes without building Stage 4. |
| **Stage 5 — Composition** (lean vision, ADR 0036)                 | **Not started. Staged product direction subject to evidence, not guaranteed implementation commitments.** (`ROADMAP.md:385-396`) Profiles, portable locking (`siralos.toml` / `siralos.lock`), Context controls (Live/Pinned/Frozen), Skills, capability-scoped Plugins, Tools, Views, optional Domains. Multi-agent not committed. | No Stage 5 artifact is due; no plugin marketplace before R10+                                            |
| **Stage 6 — Evolution and stabilization** (lean vision, ADR 0036) | **Not started. Staged product direction subject to evidence, not guaranteed.** (`ROADMAP.md:398-406`) Bounded measured `/evolve` workflows (baseline → candidate → evaluation → comparison → reject or propose).                                                                                                                    | No `/evolve` outside its planned milestone — see `AGENTS.md` guardrail                                   |

This confirmation is not a hedge — it is the **destination guardrail**: the Wayfinder map's Done state means "no decision remains before an executor could proceed," not "Stages 4-6 are committed builds." An effort that wants to redraw the Stage 4-6 scope must do so as a **fresh Wayfinder effort**, not as a resumption of this map.

---

## Self-loop verification (this decision)

| Criterion                                                                                 | Direct evidence                                                                                                                                                                                                                                                                                                                | Status |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------ |
| Generic Controlled Runtime must come before Godot specializations (and which Godot items) | §1 lists the 7-step Stage-4 sequence from `RUST_MIGRATION.md:719-732` + frozen generic-first order, with a 4-row "what generic provides vs what Godot specialization provides" table + a 3-entry "what breaks if reversed" section citing ADR 0036 Host-not-a-plugin + `SECURITY.md` fail-closed primitive + R11 measurement   | pass   |
| Exact entry gate + lean sentence                                                          | §2.1 names the 4-arrow ordered check + authoritative file `stage4-entry-gate.md` (17 criteria) and quotes `RUST_MIGRATION.md:713-717`; §2.2 quotes the two ADR 0036 lean sentences (small privileged Host + composition model; Host is not a plugin + 0 marketplaces) + `AGENTS.md:25-26` enforcement + architecture file:line | pass   |
| Stages 4-6 remain staged product direction subject to evidence                            | §3 quotes `ROADMAP.md:6` head + §5/§6 tails (Not started, staged product direction, not guaranteed) and notes that Wayfinder Done means "no decision remains," not "Stages 4-6 are committed"                                                                                                                                  | pass   |

Evidence ladder: L1 reads of `stage4-entry-gate.md` 17 criteria + verdict, `RUST_MIGRATION.md` milestone table + Stage 4 entry 719-732, ADR 0036 §1/§4/§36, `PROJECT_CONTEXT.md` §15-§16, `ROADMAP.md` tails, `ARCHITECTURE.md` Host vision — all quoted at file:line; L3 porting gate precedent; L4 decision markdown itself. No Stage 4 code, no runtime process, no Godot adapter.

---

## Out of scope for this decision (per lean ADR 0036)

No Stage 4 code, no runtime process, no Godot adapter, no visual capture, no QA workflow, no profiling. General Hooks, multi-agent machinery, TaskGraph, workflow engines, marketplaces, plugin ecosystems, model-router, generic Memory, GUI/TUI remain Future / Not Due. Stages 4-6 remain staged product direction — this decision does not unlock their implementation, only the gate sequence that would.
