---
title: "siralos-godot Externalization Research — Inventory, Shape, and Harness Implications"
label: "wayfinder:research"
type: AFK
status: closed
resolution: "decisions/60-siralos-godot-externalization-research.md"
blockedBy: []
---

## Question

Survey the exact `crates/siralos-godot` boundary so the externalization entry review cannot smuggle scope, weaken `siralos-core` domain neutrality, or break differential parity.

Research inside this repo only (no external network) — collect facts with file:line pointers, not a proposal:

- **Crate inventory:** list every file currently in `crates/siralos-godot/src/**` (godot models/limits, `impact/`, `scene/`, `scene_mutation/`, `development/`, `runtime_adapter/`, adapters) plus `Cargo.toml`, `wit/` references, and `crates/siralos-cli` / `crates/siralos-adapters` consumers (`Cargo.toml` members, `scripts/check-rust-architecture.mjs` EXPECTED_CRATES/ALLOWED_DEPENDENCIES, `siralos-godot → siralos-core` only). Confirm `72 tests` vs current count and identify any `siralos-adapters/src/godot` thin shims still present.
- **Boundary & lean invariants:** `siralos:domain-abi@1.0.0` (`crates/siralos-adapters/wit/domain-abi.wit`) as sole host/guest boundary per decisions 34/37, `forbid(unsafe_code)` / `edition 2024` / `publish=false`, and `FORBIDDEN_CORE_SYMBOL_PATTERN` removal after decision 37 — why `siralos-core` stays domain-neutral after extraction.
- **Differential subjects:** enumerate the `godot-*` corpus corpus subjects currently proved by this crate (`godot-discovery` ×4, `godot-knowledge` ×5, `godot-diagnostics` ×4, `godot-lsp` ×4, `godot-scene-resolve` ×5, `godot-review-context` ×4, `godot-mutation-prepare` ×4, `godot-develop-plan` ×4, `godot-runtime-launch` ×5, `godot-runtime-evidence` ×4, plus visual/interaction/qa/profile contributions if any) and their corpus versions (v16→v34→v38→v52) — which audits would break if the crate is removed (current audit 315/315 at v52/320, 81 expectations).
- **Distribution surface today:** how `siralos.toml` `[plugins.godot]` + `siralos.lock` (Stage 5.4 `0a6d592`) + `/domains` + `/domains-add` + `DomainHost::install` (digest pin, `lstat`/`is_path_within`/SHA-256 before `Enabled→Active`) currently consume the in-repo crate, and how `cargo deny` / `Cargo.lock` pins it today.
- **CI / gate retention:** `npm run check:rust` / `check:differential` (pinned oracle at `tests/differential/evidence/typescript-freeze-v32/`) / `cargo test --workspace` / `tier1-evidence.yml` expectations — what must remain green after the crate moves, and whether harness needs a cross-repo expectation-coverage mechanism per decision 40 C7 (post-freeze `tests/differential/evidence/post-freeze/expectations.json`).

Deliver a fact sheet with paths + line pointers (file:line), not a migration plan. Branch `research/siralos-godot-externalization` with a context pointer from this ticket is optional (local-markdown fallback: record findings in the decision doc that consumes this ticket).

Blocked by: none (AFK frontier). Needed by: 61-siralos-godot-externalization-entry-review.

## Resolution

Closed — fact sheet recorded in [decisions/60-siralos-godot-externalization-research.md](../decisions/60-siralos-godot-externalization-research.md). Local-markdown fallback, no hosted `research/siralos-godot-externalization` branch created. This unblocks [61 — Entry Review](../tickets/61-siralos-godot-externalization-entry-review.md) — frontier now includes 61.
