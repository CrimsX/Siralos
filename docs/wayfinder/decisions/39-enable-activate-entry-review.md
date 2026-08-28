# Decision — Enable/Activate Host-Gated UI Entry Review

**Wayfinder ticket:** [Enable/Activate Entry Review](../tickets/39-enable-activate-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Add Plugin UI Entry Review](38-add-plugin-ui-entry-review.md) (PASS, slice implemented at `7fe9b66`)
**Decided:** 2026-08-27 (resolver session; HITL grilling over command surface, Host authority, persistence, failure UX)
**Status:** **PASS — Enable/Activate slice frozen; authorized as next implementation arrow**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors decisions 35/37/38 (one arrow, entry-reviewed slice). No implementation lands here.

---

## Summary

The second arrow of the domains feature — Host-gated `Enable`/`Activate` for installed plugins — is frozen as two separate Host-gated commands with no persistence until `Activate`. It remains fail-closed and Host-authoritative.

## 1. Frozen contract (this slice)

| Item            | Frozen decision                                                                                                                                                                                                            |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Command surface | `/domains-enable <id>` and `/domains-activate <id>` in `siralos-cli`; registered in core catalog (`commands.rs`) with group `domains`                                                                                      |
| Enable gate     | Host authority **empty** (`HostAuthority::parse([])`); only lifecycle `Installed → Enabled` is checked, no capability grant                                                                                                |
| Activate gate   | Host authority **declared** — `HostAuthority` must contain every capability in `manifest.capabilities`; otherwise `CAPABILITY_DENIED` typed, no state change                                                               |
| Persistence     | `Enable` is **memory-only** until `Activate`; `siralos.toml` stays `path + digest` only (no `enabled` flag). `Activate` will create runtime state (future `.siralos/` Host-owned, not in this decision)                    |
| Failure UX      | Any Host-denied, missing-component, or digest-mismatch failure returns a typed `DomainFailure` code + reason and leaves lifecycle at `Installed`; view stays `Domains installed:` with the record, never empty/unavailable |

## 2. HITL answers (2026-08-27)

| #   | Frontier question    | Human answer                                                                 |
| --- | -------------------- | ---------------------------------------------------------------------------- |
| 1   | Command surface      | `/domains-enable <id>` + `/domains-activate <id>` (two commands, Host-gated) |
| 2   | Host authority       | Enable = host empty, Activate = declared (Recommended)                       |
| 3   | Persistence          | No — memory only until Activate                                              |
| 4   | Failure presentation | Typed reason + stay installed (Recommended)                                  |

## 3. Boundaries — not in this slice

- No `siralos.toml` `enabled` flag, no `.siralos/` runtime state, no `DomainHost` engine activation (lazy; `install` already verified bytes, `enable` is lifecycle-only)
- No external repo, no marketplace, no `available` flip

## 4. Authorization

**Enable/Activate UI is authorized as the next implementation arrow** against this contract. Corpus bump deferred — no new differential subject, catalog gains 2 entries (`domains-enable`, `domains-activate`) at implementation time (lockstep TS+Rust to keep `command-catalog-snapshot` parity, no new scenario file).

---

## Self-loop verification

| Criterion                              | Direct evidence                                                       | Status |
| -------------------------------------- | --------------------------------------------------------------------- | ------ |
| All four frontier questions answered   | §2 table                                                              | pass   |
| Contract is Host-gated and fail-closed | §1: empty vs declared authority, typed failures, no silent state loss | pass   |
| Boundaries explicit                    | §3: no persistence, no engine, no marketplace                         | pass   |
| No code before decision                | Worktree clean at `4b84265`+`7fe9b66`; only docs in this decision     | pass   |
