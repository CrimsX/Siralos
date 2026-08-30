---
title: "Stage 5.5 Plugin Selection by Profiles Entry Review - Narrowing-Only Domain Activation"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS per decision 51 (2026-08-28): C1-C6 approved; implemented as the Stage 5.5 slice."
blockedBy: []
---

# Ticket 51 — Stage 5.5 Plugin Selection by Profiles Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.5 Plugin Selection by Profiles Entry Review](../decisions/51-stage5-5-plugin-selection-entry-review.md) (opens after HITL grilling)

## Why now

ADR 0036 §6 lists Plugins among what a Profile may eventually select. The
primitives are landed: the profile overlay (5.1/5.2) and the plugin records +
Host-gated enable/activate surfaces (decisions 38/39). The missing piece is
the narrowing-only selection filter: a profile may reduce which enabled
plugins a session activates, never add one.

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the selection model: `ProfileRecord.plugins: Option<Vec<String>>` + `select_profile_plugins(enabled, selected)` with the mechanical intersection property (activated = enabled ∩ selected; the intersection can only shrink, so widening is impossible by construction) + digest-bound `PluginSelectionEvidence`. `siralos-adapters::profile_config` parses the `plugins` key (bounded, unique). Session application to live activation waits for its own slice. |
| C2  | Authority invariant | Selection filters, never grants: an id the Host has not enabled cannot be activated by naming it in a profile. Selected ids outside the enabled set produce truthful diagnostics and stay inactive. Enabled-set membership is Host authority (decision 39), never profile authority.                                                                                                                                                                                                              |
| C3  | Posture             | Zero spawn, zero fs in the core seam. Absent selection = typed unfiltered (all enabled stay). Empty selection = typed narrowed-to-none. Existing profile/lock evidence payloads are untouched (additive slice; old digests stable).                                                                                                                                                                                                                                                               |
| C4  | Evidence            | Frozen differential subject `composition-plugin-selection` ×4 at v43: (1) absent selection → unfiltered; (2) selection narrows → intersection activates; (3) selection names unknown ids → diagnostics, nothing broadened; (4) empty selection → activated plugins=0. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                           |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v42 → v43 (280 → 284 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (280 → 284; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                       |
| C6  | Lean guardrails     | No acquisition/installation machinery, no network, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                       |

## Open questions for HITL grilling

1. **Q1 Scope** — core selection model + adapters profile parsing, differential over the pure seam, session application deferred?
2. **Q2 Subject matrix** — `composition-plugin-selection` ×4 as drafted?
3. **Q3 Semantics** — confirm C2: intersection filter, unknown ids diagnosed but never activated, absent = unfiltered, empty = none?
4. **Q4 Approval** — approve C1–C6 as drafted?
