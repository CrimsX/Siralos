---
title: "Context Representations — L0-L4 Layers (Slice 2)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 80 — Context Representations — L0-L4 Layers (Slice 2)

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Entry review:** [79](79-context-management-foundations.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Slice 2 of decision 79's staged plan: layered representations over the Context Graph, additive and keyed by node id so the v57 pin is untouched. Frozen clause (b) is mechanically enforced: a model-derived L2 is a typed refusal.

## 2. The layers

| Layer | Name       | Origin                                                     | Notes                                      |
| ----- | ---------- | ---------------------------------------------------------- | ------------------------------------------ |
| L0    | Identity   | host: id/kind/digest/bounds metadata                       | host metadata only                         |
| L1    | Summary    | model-derived or host, provenance-bound when model-derived | optional, never authoritative              |
| L2    | Structured | HOST-ONLY, typed refusal otherwise                         | deterministic host extraction              |
| L3    | Detailed   | host or model-derived with provenance                      | bound, provenance-bound when model-derived |
| L4    | Source     | host or model-derived with provenance                      | content may be empty = reference-only      |

## 3. Criteria → evidence

| Criterion                                                     | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                     | Status |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Construction, validation, canonical order, digest, resolution | 10 new core tests (build_ok_canonical_order, duplicate_level_refusal, l2_model_derived_refusal, model_derived_without_provenance_refusal_at_l1, l4_empty_content_ok, digest_mismatch_refusal, bounds_refusals, store_unique_ids_and_canonical_order, available_levels_ascending, resolve_exact_level); cargo test --workspace --all-targets --all-features exit 0 (core 539, adapters 170, conformance 25, cli 71, 0 failed) | pass   |
| Clause (b) enforced                                           | L2 ModelDerived -> typed refusal ModelDerivedStructured { node_id }; ModelDerived without provenance -> typed refusal UnprovenancedDerived { node_id, level }                                                                                                                                                                                                                                                                | pass   |
| The contract is pinned in the differential audit              | context-representation subject at corpus v58/326 files; audit 321/321 applicable required, 4 explicit platform skips, 0 accepted informational deviations; expectations 87 records via canonicalRecordDocument (surgical 86->87 diff); pinned v32 oracle untouched                                                                                                                                                           | pass   |
| Pinned oracle untouched and gates green                       | pinned v32 oracle still 234/234; cargo fmt --all --check exit 0; cargo clippy --workspace --all-targets --all-features -- -D warnings exit 0; npm run check:differential exit 0                                                                                                                                                                                                                                              | pass   |

## 4. Result

Slice 2 — layered representations — is complete and pinned at corpus v58. Slice 3 (the tiered scheduler) is pending.
