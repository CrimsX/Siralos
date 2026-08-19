---
title: "R7 Verified Promotion — What Closes R7 Active?"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/02-r7-verified-promotion.md
blockedBy: ["01-r7-5-review-rubric.md"]
---

## Question

Decide what constitutes **R7 Verified** and the exact promotion mechanics.

Resolve:

- Definition of done for R7 overall: R7A + R7.1 + R7.2 + R7.3 + R7.4 + R7.5 each have which gate artefacts (fmt, `clippy -D warnings`, `cargo test --workspace`, `npm run check:rust`, `npm run check:architecture`, `npm run check:differential` at 128/128 applicable on Windows + 4 explicit skips + 1 deviation, `npm run check`)?
- Which status documents must atomically advance on PASS (`docs/development/PROJECT_CONTEXT.md` pointers: Last verified commit / Latest verified executable worktree / R7.5 candidate lines; `docs/development/RUST_MIGRATION.md` R7 section; `ROADMAP.md` R7 row and corpus 133 sentence)?
- Does R7 Verified immediately authorize R8, or does R8 need its own entry-review gate (mirroring R7.3 §14)?
- Lean wording: ensure R7 Verified description forbids implying Stage 4 readiness.

Decision, not promotion commit. Hand the checklist to the review that closes R7.

Blocked by: `01-r7-5-review-rubric.md`.

## Resolution

Closed — decision recorded in [decisions/02-r7-verified-promotion.md](../decisions/02-r7-verified-promotion.md). Promotion checklist handed to the close review that lands Verified commit V (see decision §5). Blocked successor: none (this ticket unblocks no further ticket — frontier is now [03-godot-boundaries-research.md](03-godot-boundaries-research.md) plus downstream grilling).
