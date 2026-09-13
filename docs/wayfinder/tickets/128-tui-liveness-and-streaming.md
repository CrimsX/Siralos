---
title: "TUI liveness, streaming turns, and three follow-up fixes"
label: "wayfinder:ticket"
status: closed
date: "2026-09-12"
supersedes: []
---

# TUI liveness, streaming turns, and three follow-up fixes

Owner reports, in order: "when i press enter, chat message should clear and
there should be some type of thinking indicator so i know it is not froze";
"also arrow for expansion of llm i can view thinking"; a 400 from the provider
("tool names can only contain certain characters (A-Za-z0-9_)"); a 401
("missing authentication header"); and "cant copy or paste anymore". The owner
also asked for a comparison against three published harnesses and approved the
resulting scope (S1 + S2 + S3).

**Two symptoms, one defect.** Pressing Enter already cleared \`state.input\`,
echoed \`> message\` and set the status to \`working\` -- but the loop
dispatched the turn with NO frame in between, and the turn was synchronous:
\`GenericProvider::call_generic\` blocked on the request and returned a complete
event vector. The next \`terminal.draw\` sat at the bottom of the loop, so the
cleared box, the echo and the indicator were never painted until the response
arrived. Fixing the freeze fixes both reports.

**Delivered.**

- **S1** (\`7365fc3\`): \`tui::accept_submitted_input\` owns the submit state
  change (clear box, clear palette, echo with a local stamp, report the line to
  run) so it is testable instead of inline, and the loop paints one frame when
  a submit is pending -- before dispatching. No fake spinner: without streaming
  the loop genuinely cannot repaint.
- **S2** (\`c6279ed\` through \`8b3738d\`): the turn is now streamed end to end.
  \`ProviderTurnCollector\` made the bounded turn steppable; \`open_stream\` lets a
  turn OWN its request so the stream can be held across steps; the generic
  provider parses OpenAI-compatible SSE incrementally and still records a
  BODY, so recordings replay through the same converter; \`Phase::StreamTurn\`
  pulls one provider event per step and emits text deltas as they arrive; the
  TUI shares its terminal so the sink can ask for a coalesced frame (33 ms)
  while the loop is blocked in the drain; and an opt-in \`ProviderPending\`
  keep-alive tick gives the frontend a chance to repaint, keep type-ahead, and
  read Esc as an interrupt.
- **Three follow-up fixes**: the credential is a live cell that \`/reload\`
  applies (and an unresolvable credential is REPORTED instead of silently
  dropping the auth header) -- the 401 (\`63f939c\`); tool names are translated
  at the provider boundary, real ids inside, aliases on the wire -- the 400
  (\`f2335c4\`); and mouse capture is OFF by default so native select, copy and
  paste work, with the wheel scrolling through the terminal's arrow fallback
  and \`/mouse\` capturing when raw wheel events are wanted.

**Recorded limits.** The spinner cannot animate while the thread waits for the
first byte: a socket-level read timeout is the right trigger and \`reqwest\` 0.12's
blocking builder has none. The reasoning row (S3) is approved but NOT delivered
by this ticket. The named \`openai\`/\`anthropic\` adapters still send dotted tool
names, and a recording captured from a live provider carries aliases.

> **Retroactive record.** Everything here was implemented, verified, and
> committed BEFORE this ticket and its entry review were written (commits
> \`63f939c\`, \`f2335c4\`, \`7365fc3\`, \`c6279ed\`, \`601474b\`, \`43514e3\`,
> \`c453ac9\`, \`2037768\`, \`bc16526\`, \`8b3738d\`). The approvals that exist are
> in-chat: the owner's bug reports, the owner's ruling on the mouse trade-off
> (option (a): capture off, wheel through the arrow fallback), and the owner's
> scope approval for S1 + S2 + S3 after the harness comparison. Nothing is
> backdated; the entry review
> ([161](../decisions/161-liveness-streaming-entry-review.md)) is itself
> retroactive and names the inversion.
