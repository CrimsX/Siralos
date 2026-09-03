---
title: "Cross-Session Replay — the Bounded Recordings Store (Amending the Never-Persisted Invariant)"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 78 — Cross-Session Replay — the Bounded Recordings Store

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Amends:** [73](73-recorded-response-replay-provider.md) · **Prior roll-up:** [76](76-real-provider-followthrough-verified-rollup.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** The user chose Option B over the deferred alternative: cross-session replay via a bounded persisted recordings store. This decision amends decision 73's never-persisted contract with the mechanical hygiene contract below; implementation proceeds in sub-slices B1 (the store, authorized here), B2 (session composition over the [profile] keys), B3 (differential pinning at v56).

## 1. The hygiene contract (each property mechanically enforceable + adversarially tested)

- **a) Sanitization-before-persist:** a hand-rolled credential-shape scanner (mirroring the check:secrets surface patterns a-d: openai-key-shape, aws-access-key-shape with the AKIAIOSFODNN7EXAMPLE allowlist, bearer-token-shape, credential-assignment-shape with the env:/${VAR} escapes) runs over every body before any write; any match → the whole write is refused with a typed error naming recording index + pattern name; matched text is never echoed.
- **b) Bounded store:** at most 64 recordings and 2 MiB total body bytes; eviction is deterministic oldest-first (the store is a bounded cache, not an archive); body size is already bounded at record time.
- **c) Digest-bound integrity:** the store file carries the digest of its canonical contents (siralos.lock pattern: verification recomputes digests, never trusts file bytes); tampered or unexpected content → typed Untrusted, never repaired or deleted automatically (checkpoint-data posture).
- **d) Untrusted-input posture:** the store at .siralos/replay-store.json is runtime DATA, not behavioral configuration; loading is bounded (file cap 2 MiB) and treats every byte as untrusted; malformed → typed unavailable.
- **e) Atomic writes over the established lockfile pattern; zero spawn paths; nothing flips unavailable.**
- **f) Configuration (B2):** additive [profile] record-replay and replay keys; absent = byte-transparent live behavior; malformed values leave the profile unapplied (decision 54 pattern).

## 2. Criteria → evidence

| Contract property                                       | Adversarial tests that cover it (named in B1; f covered in B2)                                                                                                                                                              | Status     |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| a) Sanitization-before-persist                          | credential rejections: one test per pattern (sk-, AKIA with allowlist escape, Bearer, credential-assignment) + the env:/${VAR} escapes accepted + allowlisted AWS sample accepted; no error variant ever contains body text | pass in B1 |
| b) Bounded store                                        | eviction: 65 recordings → oldest dropped, count 64, deterministic; over-bytes eviction; bounds errors typed and correct at the boundaries (64 ok, 65 err; total bytes boundary)                                             | pass in B1 |
| c) Digest-bound integrity                               | round trip write→load preserves recordings and digest; tamper (modify one body byte in the file) → UntrustedDigest; no auto-repair                                                                                          | pass in B1 |
| d) Untrusted-input posture                              | over-cap file read → typed Malformed; absent → NotFound; malformed JSON → Malformed; file >2 MiB → Malformed; every byte treated as untrusted                                                                               | pass in B1 |
| e) Atomic writes; zero spawn; nothing flips unavailable | atomic write mirrors src/lockfile.rs (temp file + rename, same fs conventions); spawn sweep clean; unavailable posture unchanged                                                                                            | pass in B1 |
| f) Configuration (B2)                                   | additive [profile] record-replay and replay keys; absent = byte-transparent; malformed → profile unapplied                                                                                                                  | pending B2 |

## 3. Result

**B1 (the store) is authorized and landed; B2 and B3 are pending their slices.**
