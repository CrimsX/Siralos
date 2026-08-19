---
title: "Godot Boundaries Research — What siralos-core Isolation Must Protect for R8/R9"
label: "wayfinder:research"
type: AFK
status: closed
resolution: decisions/03-godot-boundaries.md
blockedBy: []
---

## Question

Survey the exact Godot-adjacent isolation the existing codebase already enforces, so the R8/R9 cut cannot smuggle placeholder domains.

Research inside this repo only (no external API):

- `crates/siralos-core` must never depend on infrastructure or a domain (`npm run check:rust` domain neutrality) — list current enforcement and where a Godot type would be caught.
- R4 hardening: symlink/junction escape rejection, bounded complete exact reads (never short-read EOF), protected paths (`AGENTS.md` / `.siralos/**`) — why mutation/checkpoint creation remains `unavailable`.
- Adapter-owned Godot static inspection (engine discovery, project profiling, hand-written `.tscn`/`.tres` tokenizer, `GodotSceneModel`/`GodotResourceModel`, `godot.inspect_*` tools) vs. core-owned contexts (TaskContract, projection, workspace revisions).
- ADR 0036 lean constraint: no marketplace/plugin ecosystem, no auto-installed Godot domain — collect the quote + enforcement point.

Deliver a fact sheet with paths + line pointers (file:line), not a proposal. Branch `research/godot-boundaries` with a context pointer from this ticket.

Blocked by: none (AFK frontier). Needed by: `04-r8-r9-cut`.
## Resolution

Closed — fact sheet recorded in [decisions/03-godot-boundaries.md](../decisions/03-godot-boundaries.md). Follow-up branch note: local-markdown fallback, no hosted `research/godot-boundaries` branch created. This unblocks [R8 vs R9 Cut](../tickets/04-r8-r9-cut.md) (which was blocked by this + rubric) — frontier now includes 04.
