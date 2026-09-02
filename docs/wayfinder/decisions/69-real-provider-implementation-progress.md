---
title: "Real Provider Implementation Progress — Record of Landed Slices and Remaining Frontier"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 69 — Real Provider Implementation Progress

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) (PASS 2026-08-31) · contract frozen in [67 — Entry Review](67-real-model-provider-entry-review.md)
**Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Implementation of the Real Model/Provider range proceeded by direct user direction across the working session; this decision records what landed, the verified evidence, and the exact remaining frontier. It grants **no new scope**: the remaining slices below still require their own entry review / verification per ADR 0036.

## 1. Landed (evidence-backed)

- **`ProfileRecord` provider/model/credential/endpoint fields + `siralos.toml` parsing** (the slice authorized per decision 67 C6): bounded, validated at the boundary; validation failure leaves the profile unapplied with a truthful diagnostic.
- **`HostCredential` — env-only resolution**: `from_env_ref` accepts only `^env:[A-Z0-9_]{1,64}$`; resolved bytes live in memory for one `ModelProvider` call; `Debug`/`Display` redacted; never serialized into `siralos.toml`/`siralos.lock`/Context/logs. Byte-construction exists only under `cfg(any(test, feature = "differential-harness"))` — the ungated fallback constructor was removed (commit `8126eb8`).
- **Registry**: typed OpenAI/Anthropic adapters plus the all-purpose `GenericProvider` (decision 68 §2); typed refusals preserved for malformed input.
- **HTTP adapters**: `reqwest` blocking + rustls, 10s connect / 60s read timeouts, cancellation checks; response bodies bounded **at read time** (at most 1 MiB buffered via `io::Read::take`), control-character-sanitized, diagnostics bounded to 512-char snippets (commit `2d6f5d9`).
- **User-directed amendments recorded here**: (a) the differential harness never performs live network I/O and never transmits real credentials — provider-subject records are pinned to an unreachable loopback endpoint with fake credentials, and the `provider-generic` expectation was re-authored hermetically per decision 40 C7 (commit `8126eb8`); (b) `GenericProvider` defaults are provider-neutral placeholders (`https://generic.invalid/endpoint` — RFC 6761 reserved TLD — and `generic-model`) so a missing configuration can never route a request or its credential header to a real provider (commit `6de6e2b`).
- **Differential subject**: `provider-generic` at corpus v53 (321 scenario files, strict loader updated); audit **316/316 applicable required** (4 explicit platform skips, 0 informational), pinned v32 oracle untouched.

## 2. Verified evidence

- Fresh full `npm run check` **exit 0** on the landed content: prettier/eslint/doc-links/project-context/identity/public-hygiene/rust-architecture green; `cargo fmt --check` 0 diffs; clippy `-D warnings` clean; tests 143 (adapters) + 25 (harness) + 70 (cli) + 505 (core), 0 failed; differential 316/316 applicable required with the pinned oracle untouched.
- Monorepo commit chain (main): `a83898d` → `8d11b2d` → `8126eb8` → `2d6f5d9` → `6de6e2b`.

## 3. Remaining frontier (authority unchanged — no new scope)

1. **Determinism/replay recording of provider calls** (decision 68 §3): responses recorded via the `siralos_core::determinism`/`identity` ports for replay; a non-recorded live call is a typed `unavailable` for replay. Not yet implemented — the adapters note it as the follow-up slice.
2. **Secret-hygiene 3-surface audit** (decision 68 §4): the `grep` sweep proving no secret value appears in `siralos.toml`/`siralos.lock`/Context/logs has not yet been run as a recorded gate artefact.
3. **Real Provider Verified roll-up**: fresh full gate + this range's decisions annotated + status flip ("in progress" → "Verified") in README, ROADMAP (which still lacks a Stage 7 / Real Provider section), PROJECT_CONTEXT, and AGENTS.

## 4. External repository note

`siralos-godot` has been pushed to GitHub and is managed independently in its own repository; the monorepo tracks only the pinned path dep (`siralos-godot = { path = "../siralos-godot" }`).

## 5. Criteria → evidence

| Criterion                                                                     | Evidence                                                                                       | Status   |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | -------- |
| `ProfileRecord` provider/model/credential/endpoint landed and validated       | `profile_config.rs` / `composition.rs` boundary validation; profile unapplied on failure       | pass     |
| Credential is env-only, redacted, never serialized                            | `provider/credential.rs` (`from_env_ref`, redacted `Debug`/`Display`, gated byte constructors) | pass     |
| Registry maps bounded `provider` → `ModelProvider`; unknown input fails typed | `provider/registry.rs`; `GenericProvider` placeholders for absent config                       | pass     |
| HTTP adapters bounded, Host-observed, sanitized                               | read-time 1 MiB bound + 512-char snippets (`2d6f5d9`); timeouts + cancellation                 | pass     |
| Differential harness hermetic (no live I/O, no real credentials)              | loopback-pinned record + re-authored expectation (`8126eb8`); audit 316/316                    | pass     |
| Determinism/replay recording of provider calls                                | decision 68 §3 requirement; adapters defer to a follow-up slice                                | **open** |
| Secret-hygiene 3-surface audit artefact                                       | decision 68 §4 sweep not yet recorded                                                          | **open** |
| Real Provider Verified roll-up                                                | pending; statuses still say "in progress"                                                      | **open** |
