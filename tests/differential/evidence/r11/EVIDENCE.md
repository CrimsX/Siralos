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

## Artefacts

| File                              | Platform | What it proves                                                                                                                                 | Status                                                                                  |
| --------------------------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `audit-linux-corpus-v23.json`     | Linux    | Digest-bound migration audit at schema 3, corpus v23, from the exact repository invocation on a Tier-1 Linux runner                            | **MISSING — blocks promotion**                                                          |
| `sandbox-conformance-linux.txt`   | Linux    | Live sandbox conformance output (available where installed; otherwise loudly unavailable)                                                      | **MISSING — blocks promotion**                                                          |
| `audit-macos-corpus-v23.json`     | macOS    | Same harness invocation on a Tier-1 macOS runner                                                                                               | **MISSING — blocks promotion**                                                          |
| `sandbox-conformance-macos.txt`   | macOS    | Live sandbox conformance output                                                                                                                | **MISSING — blocks promotion**                                                          |
| `audit-windows-corpus-v23.json`   | Windows  | Same harness invocation on windows-latest or local Windows                                                                                     | **MISSING — blocks promotion** (prior v22-era copies removed as stale: pre-fix digests) |
| `sandbox-conformance-windows.txt` | Windows  | Live sandbox conformance: truthful loud skip (`setup-required`) — retained posture evidence, superseded by the v23-era rerun at promotion time | present (v22-era; refresh with the audit)                                               |

Diagnostic records (ticket 19 repair register inputs; scrubbed of
runner-identifying paths where required by check:public):

| File                                                                | Purpose                                                                                      |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `windows-differential-failure.json` / `windows-failure-latest.json` | Finding #2 crash record pre-fix (`checkpoint.undo-plan`, store.list count=0)                 |
| `windows-failure-3e15be4.json`                                      | Same crash post-canonical-write attempt, confirming fingerprint divergence was the mechanism |
| `typescript-gate-macos-3e15be4.txt`                                 | macOS gate log pre-fix (scrubbed): documents findings #3 remainder and #5                    |

## Open findings (promotion blockers)

Tracked in [Tier-1 Repair Register](../../../../docs/wayfinder/tickets/19-r11-tier1-repair-register.md):

1. **#2 Windows checkpoint probe** — fix landed in code (store exposes
   its own fingerprint; oracle writes through it). Awaiting fresh
   dispatch confirmation.
2. **#1 Windows git link-refusal tests** — canonicalized repo roots;
   awaiting fresh dispatch confirmation.
3. **#3 macOS diagnostics/enumeration/inspector/discovery** — fixes
   landed (service construction + validateCheckScript entry + test-root
   canonicalization); latest confirmed log predates them. Await fresh
   dispatch.
4. **#5 macOS SSH_AUTH_SOCK confinement refusal** — fail-closed posture
   is correct; needs an upstream wrapper scrub or an explicit accepted
   deviation recorded in the promotion decision. Never a deny-list
   weakening.

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
