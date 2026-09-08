---
title: "The Provider Config Flow & Input Fixes"
label: "wayfinder:ticket"
status: closed
date: 2026-08-31
supersedes: []
---

# Ticket 108 — The Provider Config Flow & Input Fixes

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Authorized by:** [decision 121](../decisions/121-provider-config-entry-review.md)

## Question

The provider configuration flow needs to be completed inside the TUI: `/provider` must gain an add-flow that collects provider/model/credential-env-name/endpoint and writes the `[profile]` section into `siralos.toml` atomically with format-preserving merge. The write must obey the env-only credential policy (never credential values, only the env-var name) and the established atomic-write pattern (temp + verify + rename). The user reported two input defects — the palette does not retain while typing and Tab does not complete — and the banner/greeting needs breathing room. The UI-thread question is answered as an explanation and recorded OPEN.

## Fixes (authorized by decision 121)

- **C1 THE ADD-FLOW:** `/provider` with no configured provider or an `add` entry in the picker opens a sequential modal form (provider name, model, credential env-var name, optional endpoint) with the established field suggestions and validation.
- **C2 THE CONFIG WRITE:** reads `siralos.toml` from the workspace root, merges the `[profile]` section with `credential = "env:<ENV_VAR_NAME>"` via `toml_edit`, writes atomically (temp + re-parse verification + rename, refusing symlinked/non-regular targets), preserves every other section byte-for-byte outside `[profile]`, and echoes the restart hint.
- **C3 LIVE RE-COMPOSITION DEFERRED:** the written config applies on next session start (restart hint echoed); live mid-session re-composition is recorded as future.
- **C4 PALETTE RETENTION FIX:** the palette recomputes on every input edit while `input` starts with `/`.
- **C5 TAB COMPLETION:** Tab completes the typed prefix to the matching command; multiple matches complete to the common prefix; no match is a no-op.
- **C6 BANNER NEWLINE:** a blank transcript line between the ASCII banner block and the greeting line.

## Red lines

The sanitizer is the single output boundary; approvals host-gated; no threads; decisions ≤ 120 untouched. The config write never stores credential values, only the env-var name; the write is atomic and format-preserving.

## Resolution

Open — entry review PASS per [decision 121](../decisions/121-provider-config-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 122](../decisions/122-provider-config-flow.md).

Closed — the add-flow per decisions 121-122 and the HITL rulings completed (HITL 2026-08-31).
