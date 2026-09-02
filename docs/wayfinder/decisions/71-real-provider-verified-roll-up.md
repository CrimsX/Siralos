---
title: "Real Provider Verified Roll-Up — Closure Record for the Real Model/Provider Range"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# Decision 71 — Real Provider Verified Roll-Up

**Governing plan:** [68 — Real Provider Credentials and Registry](68-real-provider-credentials-and-registry.md) · **Progress records:** [69](69-real-provider-implementation-progress.md), [70](70-real-provider-replay-recording.md) · **Map:** [Siralos Roadmap](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** Pure closure record — no behavior change in this decision. The Real Model/Provider range (decisions 66–70) is **Verified**: fresh full gate, spawn sweep, secret-hygiene audit, and every range decision annotated.

## 1. Realized range (decisions 66–70)

- **66 — Research** — fact sheet, PASS. Inventory of the deterministic-fake, ProfileRecord, `siralos.toml`/`siralos.lock`, determinism/replay, and ADR 0036 §10 `env:` requirements; research PASS and entry review unblocked.
- **67 — Entry review** — C1–C6 frozen HITL PASS — provider/model in ProfileRecord, env: only, bounded Host-observed, ToolRegistry gate, 4-step ordering. Authorized the ProfileRecord fields + `siralos.toml` parsing slice.
- **68 — Credentials & registry plan** — env-only HostCredential, registry, HTTP with determinism clock, secret hygiene — PASS. Credential `env:` only never in portable config, `provider` → `ModelProvider` registry with typed refusal, bounded Host-observed HTTP adapters, and the 3-surface hygiene audit; planning PASS.
- **69 — Implementation progress** — ProfileRecord fields + `siralos.toml` parsing, env-only HostCredential, registry + all-purpose GenericProvider with provider-neutral placeholder defaults, bounded HTTP adapters, hermetic provider-generic subject at v53. Differential `provider-generic` at corpus v53/321 files, audit 316/316 applicable required, pinned v32 oracle untouched.
- **70 — Replay recording + secret-hygiene audit** — ProviderResponseIdentity + ReplayRecorder determinism ports, typed Recorded/Unavailable availability, every observed response recorded exactly once, records carry body sha256 only; 68 §4 sweep recorded — no secret in portable surfaces. Default construction remains behavior-identical and yields typed Unavailable.

## 2. Verification criteria → evidence

| Criterion              | Evidence                                                                                                                                                                                       | Status |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Fresh full gate        | npm run check exit 0 on 2026-08-31 (prettier, eslint, doc links, project-context, identity, public hygiene, rust-arch, fmt, clippy -D warnings, workspace tests, pinned differential)          | pass   |
| Differential parity    | 316/316 applicable required at corpus v53/321 files, 82 expectation records, pinned v32 oracle untouched                                                                                       | pass   |
| Spawn sweep            | zero spawn paths in the provider range; repo-wide process matches are typed-unavailable primitives, process::id() temp-file naming, or ExitCode imports — the fail-closed posture is unchanged | pass   |
| Secret hygiene (68 §4) | recorded in decision 70 §2 — no secret-shaped value in any portable surface                                                                                                                    | pass   |
| Core neutrality        | rust-arch gate green — core stays domain-neutral, no infrastructure in core                                                                                                                    | pass   |
| Decisions annotated    | 66–71 recorded; map, README, ROADMAP, PROJECT_CONTEXT, and AGENTS status flips are part of this change set                                                                                     | pass   |
| No behavior change     | this roll-up touches documentation only                                                                                                                                                        | pass   |

## 3. Result

**Real Model/Provider is Verified.** No frontier is ticketed beyond this range — any next work starts with a new ticket + entry review per ADR 0036's lean model; the map's Out-of-scope list is unchanged.
