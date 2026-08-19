---
title: "R8 vs R9 Cut — Which Godot Parity Ships in Which Slice?"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/04-r8-r9-cut.md
blockedBy: ["01-r7-5-review-rubric.md", "03-godot-boundaries-research.md"]
---

## Question

Decide the **R8 vs R9 slice boundary** for Optional Godot parity.

Using the research from `03-godot-boundaries-research.md`, decide:

- R8 ("Optional Godot Stage-2 parity") must contain which Godot surfaces from `ARCHITECTURE.md` § Godot static inspection (engine discovery/profiling, recovery contracts, API knowledge dump parser, GDScript check-only, bounded LSP, scene/resource intelligence)?
- R9 ("Optional Godot Stage-3 parity") must contain which scene/resource approvals, deployment workflow, and review/impact intelligence surfaces?
- For each surface, state the fail-closed posture (`unavailable` process launch, no private run-directory, no mirror repair) that the differential subject must prove.
- Explicitly rule out what is NOT in either slice (placeholder domains, plugin ecosystem, GUI/TUI) per ADR 0036 lean freeze.

Output: a two-row slice table with differential subjects per slice and the "Not in R8/R9" list. Do not implement either slice.

Blocked by: R7.5 rubric and Godot boundaries research.
## Resolution

Closed — decision recorded in [decisions/04-r8-r9-cut.md](../decisions/04-r8-r9-cut.md). No R8 code is authorized yet — R8 entry review still waits on R7 Verified. This was blockedBy rubric (01) + fact sheet (03), both closed before this close.
