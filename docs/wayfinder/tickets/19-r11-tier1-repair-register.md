# Ticket — R11.3 Tier-1 Repair Register

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:repair` HITL
**Blocked by:** nothing — this is the active R11.3 completion work
**Blocks:** [R11 Verified Promotion] (ticket to be opened once green)
**Status:** OPEN

## Context

The first full POSIX/CI execution of the oracle suite surfaced five
genuine cross-platform findings ([EVIDENCE.md](../../tests/differential/evidence/r11/EVIDENCE.md)
holds the canonical register). Two are already fixed
(`reference-services.ts` containment canonicalization;
`createTempWorkspace` realpath). This ticket holds the remaining three
plus one CI-environment diagnosis. **None may be papered over**: every
one lives in security-relevant fail-closed code, and AGENTS.md forbids
weakening that posture to close a gate.

## Findings and repair strategies

| #   | Platform       | Finding                                                                                                                     | First repair move                                                                                                                                                         |
| --- | -------------- | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | windows-latest | Git launch refused: PATH git resolves through a link at `verifyExecutableAtLaunch` (`git-adapter.test.ts:248/286`)          | Reproduce locally against a link-resolved git; decide whether discovery should land on a non-link spelling (preferred) or tests become environment-adaptive               |
| 2   | windows-latest | Checkpoint oracle probe exits nonzero during replay AND `check:differential` (run `32668508719` / `32669596727`)            | Re-dispatch `tier1-evidence.yml` at ≥ `d9d70a3`: the new spill step prints `failure.json` into the job log — diagnose from that record                                    |
| 3   | macos-latest   | godot-diagnostics prepare → `invalid_input`/`failed` instead of ready (`godot-diagnostics-service.test.ts:172/201/212/301`) | Download `tier1-evidence-macos-latest-*` artifact; read `typescript-gate.txt` stack traces; suspect tmpdir-realpath semantics inside `verifyProjectPathContainment` chain |
| 4   | macos-latest   | Sandbox wrapper env includes denied `SSH_AUTH_SOCK`; fail-closed refusal fires (`anthropic-sandbox-runtime-backend.ts:850`) | Correct posture. Resolution upstream (wrapper env scrub) or an explicit accepted-decision entry — never weakening the deny list                                           |

## Evidence pointers

- ubuntu-latest: fully green — artifact
  `tier1-evidence-ubuntu-latest-cb6008fc54bcd0789aec80473bbcbe8348fcf448`
- macos-latest: differential+sandbox pass, TS gate red on #3/#4 —
  artifact `tier1-evidence-macos-latest-cb6008f...` (86 KB, contains
  `typescript-gate.txt`)
- windows-latest: audit blocked by #2 — re-run at ≥ `d9d70a3` for the
  spilled failure record
- Local Windows full gate: exit 0 throughout (all findings are
  CI/macOS-environment-conditioned)

## Definition of done

All four findings repaired or explicitly decided; a fresh
`tier1-evidence.yml` dispatch shows **three green platforms with
artifacts**; then [R11 Verified Promotion] proceeds with the seven-surface advancement.
