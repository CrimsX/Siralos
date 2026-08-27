# Ticket — Add Plugin UI Entry Review — Freeze the Empty-State Domains Slice

**Type:** `wayfinder:grilling` HITL · **Status:** OPEN (resolver in session)
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [37 — Godot Crate Extraction Entry Review](37-godot-crate-extraction-entry-review.md) (PASS, extraction landed at `7f4e1ba`: `siralos-core` domain-neutral, parity 231/231 v31 retained)

## Decision to make

Freeze the implementation contract for the **empty-state `Domains` view + `Add Plugin` flow** per decision 34 §2 UI contract, after the extraction Verified. No code changes unless the decision authorizes them.

## Open questions resolved by HITL

| #   | Question                                                   | Answer                                                                                                                                                                                         |
| --- | ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Manifest file name/format for the Add Plugin picker        | `domain-manifest.toml` at picked-folder root; TOML keys `id`, `digest` (hex), `abi`, `capabilities[]`                                                                                          |
| 2   | Where the `siralos.toml` recording `[plugins.godot]` lives | Workspace root, single portable file next to `AGENTS.md`; `path = "..."`, `digest = "sha256:..."` per plugin key                                                                               |
| 3   | Slice scope                                                | View + Add Plugin only; `/domains` empty-state render + `/domains-add <folder>` install; Enable/Activate UI deferred to a later slice (Host-gated per decision 34 §2, unfrozen)                |
| 4   | Component presence at add                                  | Manifest may name an optional relative component file; the picker verifies the component exists, is bounded, and digest-matches at add time, with a typed reason on any mismatch (fail-closed) |

## Decision outcome (written after grilling)

See [decision 38](../decisions/38-add-plugin-ui-entry-review.md).
