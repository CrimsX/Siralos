# Ticket — R11.3 Tier-1 Repair Register

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:repair` HITL
**Blocked by:** nothing — this is the active R11.3 completion work
**Blocks:** [R11 Verified Promotion] (ticket to be opened once three platforms are green)
**Status:** CLOSED — #1/#2/#3/#6 confirmed by the fresh three-platform
green dispatch at `eea0029` with artifacts retained in
[ EVIDENCE.md](../../../tests/differential/evidence/r11/EVIDENCE.md);
#5 resolved by HITL decision **(a)** — accepted deviation recorded in
the [R11 Verified Promotion] decision

## Context

The first full POSIX/CI execution of the oracle suite surfaced five
genuine cross-platform findings ([EVIDENCE.md](../../../tests/differential/evidence/r11/EVIDENCE.md)
holds the canonical register). Four are fixed in code and were confirmed
by a fresh three-platform green `tier1-evidence.yml` dispatch at
`eea0029`; one needs a HITL decision. **None may be papered over**: every
one lives in security-relevant fail-closed code, and AGENTS.md forbids
weakening that posture to close a gate.

## Findings

| #   | Platform                            | Finding                                                                                                                                                                                                        | Status                                                                                                                                                                                                                                                                                                                                        | Repair                              |
| --- | ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| 1   | windows-latest (CI)                 | Git launch refused via link identity (`git-adapter.test.ts` tamper tests)                                                                                                                                      | **Fixed in code** — canonicalized repo root in `createTempRepo`; confirmed by dispatch                                                                                                                                                                                                                                                        | Confirmed by dispatch               |
| 2   | windows-latest (CI)                 | Checkpoint probe crash: `store.get` returned null for freshly written fixtures (`checkpoint.undo-plan`)                                                                                                        | **Fixed in code** — store exposes its own fingerprint as single source of truth (`866fdea`); oracle + vitest helper consume it. Root cause: oracle/store recomputed the namespace hash independently and diverged on CI                                                                                                                       | Confirmed by dispatch               |
| 3   | macos-latest                        | godot-diagnostics prepare → `invalid_input`/`failed`; script-enumeration ×3; discovery ×2; project-inspector ×1; reference-path comparisons                                                                    | **Fixed in code** — service-construction canonicalization, validateCheckScript entry-point realpath, test-helper root canonicalization across diagnostics/discovery/project-inspector/reference fixtures. Confirmed green locally; macOS gate log at `3e15be4` predates these fixes                                                           | Confirmed by dispatch               |
| 6   | windows-latest + slow Windows hosts | Replay-stress wall-clock budgets sized for the pre-v23 corpus lost the deadline race on Windows (`replay.test.mjs` ×3, `harness.test.mjs` oracle-determinism ×2) — Vitest timeouts, not determinism mismatches | **Fixed in code** — Windows-scaled deadlines and replay depth in the two test files; assertions untouched, POSIX bounds held on the retained macOS v23 gate record (replay 137 s of 180 s). Local slow-Windows reproduction: replay stress needed 531 s of its new 720 s bound                                                                | Confirmed by dispatch               |
| 4   | macos-latest (superseded numbering) | _(was SSH_AUTH_SOCK in an earlier draft — renumbered to #5 to match EVIDENCE.md)_                                                                                                                              | —                                                                                                                                                                                                                                                                                                                                             | See #5                              |
| 5   | macos-latest                        | Sandbox wrapper injects denied `SSH_AUTH_SOCK`; fail-closed refusal fires correctly (`mergeWrapperEnvironment`)                                                                                                | **RESOLVED — HITL option (a)**: recorded as an accepted macOS deviation in the [R11 Verified Promotion] decision. CI unsets only its own injected variable (`aa13128`); the deny list is unchanged and still unit-covered, and the macOS live suite now skips loudly on the typed private-run-directory boundary. Never a deny-list weakening | Decision + promotion-decision entry |

## Evidence state

- **Linux:** green in the fresh dispatch at `eea0029`; the
  `audit-linux-corpus-v23.json` + `sandbox-conformance-linux.txt`
  artifacts await download and retention here.
- **Windows:** `audit-windows-corpus-v23.json` +
  `sandbox-conformance-windows.txt` checked in from the local Windows
  host (decision 18 permits this) — digest-bound to corpus v23
  (`50c0575f…`), 217/217 applicable required parity, identity
  `9e0e05f`. Re-confirmed green by the fresh dispatch.
- **macOS:** prior audits removed as stale (pre-fix digests); green in
  the fresh dispatch at `eea0029`; the named macOS artifacts await
  download and retention here.

Diagnostic records retained (scrubbed per check:public):
`windows-differential-failure.json`, `windows-failure-latest.json`,
`windows-failure-3e15be4.json`, `typescript-gate-macos-3e15be4.txt`.

## Definition of done

A fresh `tier1-evidence.yml` dispatch shows **three green platforms
with artifacts** (macOS may show only the recorded #5 deviation if
option (a) is chosen), then [R11 Verified Promotion] proceeds with the
seven-surface advancement.

**Met.** The `eea0029` dispatch returned three green platforms; all six
artefacts are retained with matching provenance in
[ EVIDENCE.md](../../../tests/differential/evidence/r11/EVIDENCE.md).
[R11 Verified Promotion] proceeds.
