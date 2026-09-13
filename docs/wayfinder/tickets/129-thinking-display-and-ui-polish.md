---
title: "Thinking display and UI polish"
label: "wayfinder:ticket"
status: closed
date: "2026-09-12"
supersedes: []
---

# Thinking display and UI polish

Owner reports in order: "still cant seem to expand i see thoughts are they are
generated"; "is moreso laggy? delay when expanding then text does not appear
smoothly"; "the thoughts seems to be displaying line by line, maybe something
similar to deep seek harness where the text is displaying smoothly left to
right"; "maybe make errors coloured in red? and tool calls grey?"; "move the
working into thinking and have it right about user input with pulsating dots .
to .. to ..."; "working missing three dots in its loading cycle, also should be
updating every 1 second interval and be separate from the actual thinking, also
removing working/ready text in the bottom bar"; "the loading text from left to
right rather than instantly outputting a line should apply in expanded thoughts
as well as llm output"; and "system prompt needs to be updated? referencing
godot when we have separated that".

**Delivered.**

- **S3a, the channel** (\`50f316c\`): \`ModelEvent::ReasoningDelta\` and
  \`ToolLoopEvent::ReasoningDelta\` carry thinking as its own stream; the SSE
  assembler emits it and records it in the assembled body so a replay
  reproduces it; the collector bounds it on its own budget. Writing the test
  found a REAL leak: routing reasoning through \`push_text\` accumulated it into
  \`assistant_text\`, so thinking would have become the answer and the history.
  \`push_reasoning\` shares the budgets and appends nothing.
- **S3b, the row** (\`7a5985a\`): one collapsed row, Right expands, Left
  collapses, and a route that never reasons renders nothing.
- **Live and legible** (\`d31a356\`): the arrows work MID-FLIGHT (they were read
  and discarded while a turn ran), and the Session budget takes the hard cap
  (32) the frozen rules allow instead of the reference default of 8 that a real
  multi-step task exhausts.
- **Smooth paint and an honest prompt** (\`2a3e653\`): per-event repaints are
  throttled (unthrottled they made the stream lumpy) while a key press forces a
  frame; and the system prompt no longer calls Siralos "a host-owned AI agent
  harness for Godot Engine development" with a GDScript section -- the domain
  has been external since decisions 60-65, so the text is product-neutral and
  says optional domain intelligence is installed explicitly and never assumed.
- **Alive while it streams** (\`169691b\`, \`aee5fc4\`): the collapsed row
  previews the newest thinking text instead of a line count that only moved
  when a line completed; failures render red, tool activity grey; and the
  \`working\` state pulses \`.\` -> \`..\` -> \`...\` once a second.
- **Static and separate** (\`2f623a1\`, \`34b0237\`): the indicator owns a real row
  directly above the input, in the banner colour, kept separate from the
  thinking block; the bottom bar drops the \`ready\`/\`working\` word entirely,
  which is why the indicator keys off the turn timer rather than the status.
- **Left to right** (\`fa7534a\`): the transcript is line-based, so a line only
  appeared when the provider finished it. Text now goes to a REVEAL buffer and
  the incomplete line renders as a growing tail, on one budget shared by the
  answer and the thinking.

**Recorded limits.** The pulse and the reveal advance only when a frame is
painted, so during a blocking wait with no provider output (the first byte, a
long tool execution) they hold. Fixing that needs a socket read timeout
(\`reqwest\` 0.12 blocking has none) or a worker thread behind a decision. Nothing
yet ASKS a route for reasoning, so a model that only reasons when requested
shows no block. The named \`openai\`/\`anthropic\` adapters still send dotted tool
names.

> **Retroactive record.** Everything here was implemented, verified and
> committed BEFORE this ticket and its entry review were written (commits
> \`50f316c\`, \`7a5985a\`, \`d31a356\`, \`2a3e653\`, \`169691b\`, \`aee5fc4\`,
> \`2f623a1\`, \`34b0237\`, \`fa7534a\`). The approvals that exist are in-chat: the
> owner's reports and rulings above, on top of the S1+S2+S3 scope approval
> recorded in ticket 128. Nothing is backdated; the entry review
> ([163](../decisions/163-thinking-display-entry-review.md)) is itself
> retroactive and names the inversion.
