---
title: "The Transcript Wrap and the Legible 429 Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "123"
supersedes: []
---

# The Transcript Wrap and the Legible 429 Entry Review

Ticket [123](../tickets/123-transcript-wrap-429.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `6ef93d4` "fix(tui): wrap transcript lines and make
> 429 legible"). There was no prior ticket, no prior entry review, and
> no prior human approval for this change, and this record does not
> invent or backdate any of them. The provenance is an owner bug report
> — the unreadable 429 and the clipping were reported live — not a
> design approval. The normal order (entry review authorizes →
> implementation lands) is inverted here: the implementation landed
> first and this review records what it should have authorized. The
> verdict below is therefore a retroactive PASS over the
> already-committed diff, not a pre-commit authorization.

> **Owner-reported (retroactive).** A provider error could not be read
> because the transcript rendered one `Line` per entry into a
> `Paragraph` with no wrapping, so anything wider than the pane was
> clipped — `"Response failed: response failed: 429 at ... -
{"error"..."` ran off the screen. The fix: wrapping is PRESENTATIONAL
> — `wrap_line_to_width` (exact-fit word packing, word-boundary breaks,
> hard-break for tokens wider than the pane) plus
> `wrapped_transcript_rows`, shared by both render paths, with scroll
> windowing over wrapped rows; stored transcript text is never mutated
> and lines that already fit pass through byte-identical. HTTP 429
> gained one actionable line (the provider is rate limiting this key —
> wait and retry, or switch model) appended on both the chat and
> `/models` paths, while the decisions-137/138 shape stays
> byte-identical for every other status. Deliberately NO retries or
> backoff: retrying into a rate limit makes it worse. One post-freeze
> render expectation was re-pinned because the frame now shows the
> wrapped row where it previously showed the clipped one.

## 2. The Fixes

| Fix                        | The change                                                                                                                                                                                                                            | The evidence                          |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| F1 PRESENTATIONAL WRAPPING | `wrap_line_to_width` (exact-fit word packing, word-boundary breaks, hard-break for tokens wider than the pane) plus `wrapped_transcript_rows`, shared by both render paths; scroll windowing over wrapped rows.                       | the committed TUI diff                |
| F2 STORED TEXT UNTOUCHED   | Stored transcript text is never mutated; lines that already fit pass through byte-identical — wrapping changes only what the pane shows.                                                                                              | the committed diff; passthrough tests |
| F3 LEGIBLE 429             | HTTP 429 gains one actionable line (the provider is rate limiting this key — wait and retry, or switch model) appended on both the chat and `/models` paths; the decisions-137/138 shape stays byte-identical for every other status. | the committed provider/TUI diff       |
| F4 NO RETRIES, BY DECISION | Deliberately no retries or backoff: retrying into a rate limit makes it worse.                                                                                                                                                        | the committed diff (no retry path)    |
| F5 RE-PINNED FRAME         | One post-freeze render expectation re-pinned because the frame now shows the wrapped row where it previously showed the clipped one.                                                                                                  | the committed expectation diff        |

## 3. Criteria → Evidence

| Criterion                                            | Evidence                                                                                                                                                                                                            | Verdict |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Wide transcript rows are readable within the pane    | `wrap_line_to_width` / `wrapped_transcript_rows` tests: word-boundary breaks, hard-break for over-wide tokens, both render paths share the helper, scroll windows over wrapped rows                                 | pass    |
| Fitting lines and stored text unchanged              | passthrough tests: lines that fit render byte-identical; stored transcript text never mutated                                                                                                                       | pass    |
| 429 is actionable; every other status byte-identical | 429-line tests on the chat and `/models` paths; decisions-137/138 shape unchanged for all other statuses                                                                                                            | pass    |
| No regression                                        | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity 352/352 applicable required, 4 platform skips, 0 deviations (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): presentational transcript wrapping
with the actionable 429 line, as committed in `6ef93d4`, is the
correct fix — wide rows read within the pane, stored text and
fitting lines are byte-identical, non-429 errors keep their shape,
and no retry machinery was added. This review authorized nothing (the
code had already landed); it records what authorization would have
covered.
