# R11 Tier-1 cross-platform evidence manifest

**Milestone:** Stage 3R R11 — full differential, effect-boundary,
security, recovery, and cross-platform parity.
**Contract:** [R11 Entry Review](../../../../docs/wayfinder/decisions/18-r11-entry-review.md)
§3; closure criteria [R11 Gate](../../../../docs/wayfinder/decisions/06-r11-gate.md)
§2.2/§2.4.

This directory retains the per-platform promotion artefacts required by
decision 18. A missing platform artifact **blocks the R11 Verified
promotion** until attached here; it is never silently dropped.

**Corpus note:** corpus v22 was briefly ambiguous (fixture-set changed
mid-version without a bump); the version was bumped to **v23** (222
fixtures, manifest `50c0575f…`) so all retained audits are digest-bound
to exactly one fixture set. Pre-v23 audit files were removed from this
directory as stale.

**Provenance:** every artefact below was produced by the
`tier1-evidence.yml` dispatch at commit
`eea0029e70aae7248b3e1022c3be1cb669fd5a09`. All three audits carry the
identical `sourceTreeSha256` (`ad1e4963…`, 1264 source files),
`corpusVersion: 23`, `schemaVersion: 3`, corpus digest
`50c0575f279a7481dc16f4ec98d90879db45256460ecf16c841c78bfc529537e`,
`parityHeld: true`, `deviationCount: 0`, and
`matchedRequiredScenarios: 217 / requiredApplicableScenarios: 217`.

## Artefacts

| File                              | Platform | What it proves                                                                                                      | Status                                                                                         |
| --------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `audit-linux-corpus-v23.json`     | Linux    | Digest-bound migration audit at schema 3, corpus v23, from the exact repository invocation on a Tier-1 Linux runner | **present** — dispatch `eea0029`; parity held, 217/217 applicable required, zero deviations    |
| `sandbox-conformance-linux.txt`   | Linux    | Live sandbox conformance output (available where installed; otherwise loudly unavailable)                           | **present** — loud skip (`dependency-missing`: rg/bwrap/socat absent); never treated as secure |
| `audit-macos-corpus-v23.json`     | macOS    | Same harness invocation on a Tier-1 macOS runner                                                                    | **present** — dispatch `eea0029`; parity held, 217/217 applicable required, zero deviations    |
| `sandbox-conformance-macos.txt`   | macOS    | Live sandbox conformance output                                                                                     | **present** — loud skip (33 probes; private run-directory creation unavailable, typed reason)  |
| `audit-windows-corpus-v23.json`   | Windows  | Same harness invocation on windows-latest or local Windows                                                          | **present** — dispatch `eea0029`; parity held, 217/217 applicable required, zero deviations    |
| `sandbox-conformance-windows.txt` | Windows  | Live sandbox conformance: truthful loud skip (`setup-required`) — retained posture evidence                         | **present** — dispatch `eea0029`; loud skip (`setup-required`), superseding the v22-era copy   |

The two POSIX audits differ in exactly one informational record
(`state-dir.fallback.posix` embeds the runner's real home-derived state
directory digest), classified informational per ADR-0033 — OS-account
fallbacks are never required-parity fixtures. Windows carries zero
informational deviations.

Diagnostic records (ticket 19 repair register inputs; scrubbed of
runner-identifying paths where required by check:public):

| File                                                                | Purpose                                                                                      |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `windows-differential-failure.json` / `windows-failure-latest.json` | Finding #2 crash record pre-fix (`checkpoint.undo-plan`, store.list count=0)                 |
| `windows-failure-3e15be4.json`                                      | Same crash post-canonical-write attempt, confirming fingerprint divergence was the mechanism |
| `typescript-gate-macos-3e15be4.txt`                                 | macOS gate log pre-fix (scrubbed): documents findings #3 remainder and #5                    |
| `typescript-gate.txt`                                               | Windows TypeScript gate record from the `eea0029` dispatch: 211 files / 3231 tests passed    |

## Findings — resolved

All register findings are closed; detail lives in
[Tier-1 Repair Register](../../../../docs/wayfinder/tickets/19-r11-tier1-repair-register.md):

1. **#1 Windows git link-refusal** — fixed (canonicalized temp-repo
   roots); confirmed by the `eea0029` dispatch.
2. **#2 Windows checkpoint fingerprint divergence** — fixed (store owns
   its namespace fingerprint); confirmed by the dispatch.
3. **#3 macOS diagnostics/enumeration/inspector/discovery
   canonicalization family** — fixed; confirmed by the dispatch.
4. **#6 Replay-stress deadline race on slow Windows hosts** — fixed
   (platform-scaled deadlines/replay depth; assertions untouched);
   confirmed by the dispatch.
5. **#5 macOS SSH_AUTH_SOCK confinement refusal** — **resolved by HITL
   decision (a)**: recorded as an accepted deviation in the
   [R11 Verified Promotion] decision. CI unsets only its own injected
   variable (`aa13128`); the deny list is unchanged and still covered by
   unit tests, and the macOS live suite then skips loudly on the typed
   private-run-directory boundary. Never a deny-list weakening.

## How to produce the artefacts

The same invocation produces each platform's audit on an installed
Node.js (24.17) + Rust (toolchain-pinned) host:

```text
npm ci
cargo build --locked -p siralos-cli --bin siralos-harness --features differential-harness
npm run check:differential        # writes tests/differential/out/audit.json
cp tests/differential/out/audit.json tests/differential/evidence/r11/audit-<platform>-corpus-v<manifest version>.json
npm run test:sandbox > tests/differential/evidence/r11/sandbox-conformance-<platform>.txt
```

Or dispatch `.github/workflows/tier1-evidence.yml` and drop the
downloaded artifact contents here (names already match).
