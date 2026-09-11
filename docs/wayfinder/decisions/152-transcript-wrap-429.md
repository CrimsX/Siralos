---
title: "The Transcript Wrap and the Legible 429"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "123"
supersedes: []
---

# The Transcript Wrap and the Legible 429

Ticket [123](../tickets/123-transcript-wrap-429.md) · entry review
[151](151-transcript-wrap-429-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `6ef93d4` "fix(tui): wrap transcript lines and make
> 429 legible") BEFORE this record was written. There was no prior
> ticket, no prior entry review, and no prior human approval for this
> change; the provenance is an owner bug report — the unreadable 429
> and the clipping were reported live — nothing more is claimed here,
> and nothing is backdated. The entry review
> ([151](151-transcript-wrap-429-entry-review.md)) is itself retroactive
> and names the inversion.

## 2. The Implemented

| Fix                        | The change                                                                                                                                                                                                             | The evidence                    |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| M1 PRESENTATIONAL WRAPPING | `wrap_line_to_width` (exact-fit word packing, word-boundary breaks, hard-break for tokens wider than the pane) plus `wrapped_transcript_rows`, shared by both render paths, with scroll windowing over wrapped rows.   | the committed TUI diff          |
| M2 STORED TEXT UNTOUCHED   | Stored transcript text never mutated; lines that already fit pass through byte-identical.                                                                                                                              | the committed diff              |
| M3 LEGIBLE 429             | One actionable line appended on both the chat and `/models` paths (the provider is rate limiting this key — wait and retry, or switch model); the decisions-137/138 shape stays byte-identical for every other status. | the committed provider/TUI diff |
| M4 NO RETRIES              | Deliberately no retries or backoff: retrying into a rate limit makes it worse.                                                                                                                                         | the committed diff              |
| M5 RE-PINNED FRAME         | One post-freeze render expectation re-pinned because the frame now shows the wrapped row where it previously showed the clipped one.                                                                                   | the committed expectation diff  |

## 3. Criteria → Evidence

| Criterion                               | Evidence                                                                                                                                                                                                                      | Verdict  |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Clipped errors now read within the pane | wrapping tests pass (word packing, boundary breaks, over-wide hard-break); both render paths share the helper                                                                                                                 | pass     |
| Nothing else changed shape              | fitting lines byte-identical; non-429 statuses byte-identical to the decisions-137/138 shape                                                                                                                                  | pass     |
| No regression                           | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity held 352/352 applicable required, 4 platform skips, 0 deviations (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                      | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                      | reported |

## 4. Result

The transcript wrap and legible-429 change is complete as committed in
`6ef93d4`: provider errors read within the pane, the 429 tells the
user what to do, and everything that already fit is byte-identical.
Retroactive record closed; ticket 123 done.
