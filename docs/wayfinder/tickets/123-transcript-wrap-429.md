---
title: "The Transcript Wrap and the Legible 429"
label: "wayfinder:ticket"
status: closed
date: "2026-09-11"
supersedes: []
---

# The Transcript Wrap and the Legible 429

A provider error could not be read: the transcript rendered one `Line`
per entry into a `Paragraph` with no wrapping, so anything wider than
the pane was clipped — `"Response failed: response failed: 429 at ... -
{"error"..."` ran off the screen. The fix: wrapping is PRESENTATIONAL
— `wrap_line_to_width` (exact-fit word packing, word-boundary breaks,
hard-break for tokens wider than the pane) plus
`wrapped_transcript_rows`, shared by both render paths, with scroll
windowing over wrapped rows; stored transcript text is never mutated
and lines that already fit pass through byte-identical. HTTP 429 gained
one actionable line (the provider is rate limiting this key — wait and
retry, or switch model) appended on both the chat and `/models` paths,
while the decisions-137/138 shape stays byte-identical for every other
status. Deliberately NO retries or backoff: retrying into a rate limit
makes it worse.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `6ef93d4` "fix(tui): wrap transcript lines and make
> 429 legible") BEFORE this ticket and its entry review were written.
> There was no prior ticket and no prior entry review for this change;
> the provenance is an owner bug report — the unreadable 429 and the
> clipping were reported live — nothing more is claimed here, and
> nothing is backdated. The entry-review inversion is named in [the
> retroactive entry
> review](../decisions/151-transcript-wrap-429-entry-review.md): the
> record follows the implementation rather than authorizing it.

Authorized by
[the Transcript Wrap and the Legible 429 Entry Review (retroactive)](../decisions/151-transcript-wrap-429-entry-review.md).
Implemented and recorded in
[decision 152](../decisions/152-transcript-wrap-429.md).
