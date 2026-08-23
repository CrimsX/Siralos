# R11 Tier-1 cross-platform evidence manifest

**Milestone:** Stage 3R R11 — full differential, effect-boundary,
security, recovery, and cross-platform parity.
**Contract:** [R11 Entry Review](../../../../docs/wayfinder/decisions/18-r11-entry-review.md)
§3; closure criteria [R11 Gate](../../../../docs/wayfinder/decisions/06-r11-gate.md)
§2.2/§2.4.

This directory retains the per-platform promotion artefacts required by
decision 18. A missing platform artifact **blocks the R11 Verified
promotion** until attached here; it is never silently dropped.

## Artefacts

| File                              | Platform | What it proves                                                                                                                                                                                                                                                          | Status                           |
| --------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `audit-windows-corpus-v22.json`   | Windows  | Digest-bound migration audit at schema 3, corpus version 22 (`8b495e001c60965e45527b983e7da56e2c11c0ef2e64592c72b71486d559f2e5`): parity held, 221 reference records, 0 deviations, 216/216 applicable required scenarios matched, 4 explicit POSIX-only platform skips | **present**                      |
| `sandbox-conformance-windows.txt` | Windows  | Live `npm run test:sandbox` output: `SKIPPED - backend unavailable (state: setup-required, platform: windows)` with the exact package-supported install command; an unavailable backend is never treated as secure                                                      | **present (truthful loud skip)** |
| `audit-linux-corpus-v22.json`     | Linux    | Same harness invocation on a Tier-1 Linux runner                                                                                                                                                                                                                        | **MISSING — blocks promotion**   |
| `sandbox-conformance-linux.txt`   | Linux    | Live sandbox conformance output (available where installed; otherwise loudly unavailable)                                                                                                                                                                               | **MISSING — blocks promotion**   |
| `audit-macos-corpus-v22.json`     | macOS    | Same harness invocation on a Tier-1 macOS runner                                                                                                                                                                                                                        | **MISSING — blocks promotion**   |
| `sandbox-conformance-macos.txt`   | macOS    | Live sandbox conformance output                                                                                                                                                                                                                                         | **MISSING — blocks promotion**   |

## How to produce the missing artefacts

The same invocation produces each platform's audit on an installed
Node.js (22+) + Rust (1.85+) host:

```text
npm ci
cargo build -p siralos-cli --bin siralos-harness --features differential-harness
npm run check:differential        # writes tests/differential/out/audit.json
cp tests/differential/out/audit.json tests/differential/evidence/r11/audit-<platform>-corpus-v22.json
npm run test:sandbox > tests/differential/evidence/r11/sandbox-conformance-<platform>.txt
```

The replay-stress determinism property is exercised by
`tests/differential/replay.test.mjs` as part of `npm test`; its result is
covered by the full-gate run recorded for each platform alongside the
audit copy.

## Recorded local-host findings (Windows, 2026-08-23)

- The Windows host has WSL installed but **no distributions**, so no
  local Linux run is possible without installing one or using CI.
- The sandbox backend reports `setup-required` on Windows by design;
  running the optional `npx sandbox-runtime windows-install` would change
  that posture and is deliberately NOT part of R11 — the gate closes on
  truthful reporting, not on availability ([R11 Gate] §2.2 exit).

## Open Tier-1 findings (promotion blockers, 2026-08-23)

Surfaced by the first full POSIX/CI execution of the oracle suite —
these are genuine cross-platform divergences to repair; none may be
papered over:

1. **Windows (CI): git launch refused via link identity.**
   `git-cli-adapter.ts` refuses PATH-resolved Git when
   `samePathIdentity(realpath(git), git)` fails at launch; the runner
   image's Git layout trips this. The adapter posture is correct;
   either the discovery must land on a non-link spelling or the tests
   need environment-adaptive expectations.
   (`git-adapter.test.ts:248/286`)
2. **Windows (CI): checkpoint probe crash in replay.**
   `runWorkspaceProbe` reports the checkpoint oracle exiting nonzero on
   windows-latest during replay; passes locally. Needs artifact-level
   diagnosis (`failure.json`/probe stderr).
3. **macOS: godot-diagnostics prepare returns invalid_input/failed.**
   `validateCheckScript` → `verifyProjectPathContainment` rejects
   fixture scripts under macOS tmpdir realpath semantics
   (`/var/folders` vs `/private/var/folders`) for raw workspace roots.
   (`godot-diagnostics-service.test.ts:172/201/212/301`)
4. **macOS: reference-root containment bypassed** — FIXED in
   `reference-services.ts` (canonicalize workspace root before
   comparison); covered by `reference-services.test.ts`.
5. **macOS: sandbox wrapper injects denied `SSH_AUTH_SOCK`.**
   The pinned sandbox-runtime wrapper's env includes a variable Siralos
   denies; the fail-closed refusal is correct. Resolution belongs
   upstream (wrapper env) or as an explicit accepted decision — never
   by weakening the deny list. (`anthropic-sandbox-runtime-backend.ts`
   mergeWrapperEnvironment)
6. **Windows/macOS: reference-path comparisons vs raw tmpdir spelling**
   (`RUNNER~1`, `/var/...`) — FIXED via canonicalizing
   `createTempWorkspace`.
