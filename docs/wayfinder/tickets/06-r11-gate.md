---
title: "R11 Gate — Full Differential, Effect-Boundary, Security, Recovery, Cross-Platform Closure Criteria"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/06-r11-gate.md
blockedBy: ["05-r10-scope.md"]
---

## Question

Decide the **R11 closure gate** — the complete pre-retirement parity line.

Resolve:

- Entry conditions for R11 (which R10 artefacts must exist).
- What "full" means: effect-boundary enforcement (workspace create/edit/delete fail-closed until mechanically enforceable), security/sandbox conformance (`npm run test:sandbox` with truthful unavailable reporting), recovery classification (typed retryable/non-retryable/denied/stale/resource/uncertain), and cross-platform Tier-1 matrix (Linux/Windows/macOS digest-bound audits).
- Measurement discipline per `docs/development/RUST_STYLE.md` leverage principle: when a hot spot justifies specialization, what evidence is required.
- The exact artefacts that constitute PASS (corpus version bump, manifest/digest regeneration, harness replay stress).

Decision, not implementation.

Blocked by: `05-r10-scope.md`.
## Resolution

Closed — decision recorded in [decisions/06-r11-gate.md](../decisions/06-r11-gate.md). Entry requires R7 Verified (02), R8/R9 (04) and R10a-c (05) all Verified + harness schema 3. This was blockedBy 05, which closed before this close. This unblocks [R12 Disposition](../tickets/07-r12-disposition.md) — frontier now includes 07.
