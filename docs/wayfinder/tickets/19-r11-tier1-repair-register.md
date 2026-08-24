# Ticket — R11.3 Tier-1 Repair Register

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:repair` HITL
**Blocked by:** nothing — this is the active R11.3 completion work
**Blocks:** [R11 Verified Promotion] (ticket to be opened once three platforms are green)
**Status:** OPEN — #1/#2/#3/#6 fixed in code awaiting fresh dispatch; #5 awaiting HITL decision

## Context

The first full POSIX/CI execution of the oracle suite surfaced five
genuine cross-platform findings ([EVIDENCE.md](../../../tests/differential/evidence/r11/EVIDENCE.md)
holds the canonical register). Three are fixed in code; one needs a HITL
decision; all await confirmation by a fresh `tier1-evidence.yml`
dispatch at HEAD. **None may be papered over**: every one lives in
security-relevant fail-closed code, and AGENTS.md forbids weakening that
posture to close a gate.

## Findings

| #   | Platform                            | Finding                                                                                                                                                                                                        | Status                                                                                                                                                                                                                                                                              | Repair                              |
| --- | ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| 1   | windows-latest (CI)                 | Git launch refused via link identity (`git-adapter.test.ts` tamper tests)                                                                                                                                      | **Fixed in code** — canonicalized repo root in `createTempRepo`; unconfirmed pending dispatch                                                                                                                                                                                       | Fresh dispatch confirms             |
| 2   | windows-latest (CI)                 | Checkpoint probe crash: `store.get` returned null for freshly written fixtures (`checkpoint.undo-plan`)                                                                                                        | **Fixed in code** — store exposes its own fingerprint as single source of truth (`866fdea`); oracle + vitest helper consume it. Root cause: oracle/store recomputed the namespace hash independently and diverged on CI                                                             | Fresh dispatch confirms             |
| 3   | macos-latest                        | godot-diagnostics prepare → `invalid_input`/`failed`; script-enumeration ×3; discovery ×2; project-inspector ×1; reference-path comparisons                                                                    | **Fixed in code** — service-construction canonicalization, validateCheckScript entry-point realpath, test-helper root canonicalization across diagnostics/discovery/project-inspector/reference fixtures. Confirmed green locally; macOS gate log at `3e15be4` predates these fixes | Fresh dispatch confirms             |
| 6   | windows-latest + slow Windows hosts | Replay-stress wall-clock budgets sized for the pre-v23 corpus lost the deadline race on Windows (`replay.test.mjs` ×3, `harness.test.mjs` oracle-determinism ×2) — Vitest timeouts, not determinism mismatches | **Fixed in code** — Windows-scaled deadlines and replay depth in the two test files; assertions untouched, POSIX bounds held on the retained macOS v23 gate record (replay 137 s of 180 s). Local slow-Windows reproduction: replay stress needed 531 s of its new 720 s bound      | Fresh dispatch confirms             |
| 4   | macos-latest (superseded numbering) | _(was SSH_AUTH_SOCK in an earlier draft — renumbered to #5 to match EVIDENCE.md)_                                                                                                                              | —                                                                                                                                                                                                                                                                                   | See #5                              |
| 5   | macos-latest                        | Sandbox wrapper injects denied `SSH_AUTH_SOCK`; fail-closed refusal fires correctly (`mergeWrapperEnvironment`)                                                                                                | **OPEN — needs HITL decision**: (a) record as accepted macOS deviation in the promotion decision, (b) pursue upstream wrapper scrub, or (c) defer to R12-era work. Never a deny-list weakening                                                                                      | Decision + promotion-decision entry |

## Evidence state

- **Linux:** genuinely absent (no runner access); produced by
  `tier1-evidence.yml` dispatch or equivalent invocation.
- **Windows:** `audit-windows-corpus-v23.json` +
  `sandbox-conformance-windows.txt` checked in from the local Windows
  host (decision 18 permits this) — digest-bound to corpus v23
  (`50c0575f…`), 217/217 applicable required parity, identity
  `9e0e05f`.
- **macOS:** prior audits removed as stale (pre-fix digests); fresh
  artifacts land with the next dispatch.

Diagnostic records retained (scrubbed per check:public):
`windows-differential-failure.json`, `windows-failure-latest.json`,
`windows-failure-3e15be4.json`, `typescript-gate-macos-3e15be4.txt`.

## Definition of done

A fresh `tier1-evidence.yml` dispatch shows **three green platforms
with artifacts** (macOS may show only the recorded #5 deviation if
option (a) is chosen), then [R11 Verified Promotion] proceeds with the
seven-surface advancement.
