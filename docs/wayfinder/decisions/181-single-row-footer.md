---
title: "The Footer Is One Row: Header Bar Out, Observed Round Metrics In"
label: "wayfinder:decision"
status: "accepted"
date: "2026-10-17"
supersedes: [117, 118]
---

# The Footer Is One Row: Header Bar Out, Observed Round Metrics In

Map · [117](117-tui-polish-entry-review.md) · [118](118-tui-polish-pass.md) ·
[120](120-tui-ux-pass-2.md) · [102](102-provider-interface-takeaways.md)

## 1. The report

Owner-directed, 2026-10-17 (session HITL): remove the top header bar and give
the session's real usage a home in the bottom row — throughput, tokens, cache,
context, provider and model, in one line. The header bar was decision
[118](118-tui-polish-pass.md) P2's session identity, deduplicated per
[120](120-tui-ux-pass-2.md) I1; the bottom status line carried only status
text. The owner wanted identity and measurement in one row: **one row less,
one row more informative**.

## 2. The rule

- **One bottom row.** `compose_bottom_row` (`crates/siralos-cli/src/tui/status.rs`)
  renders `status | tps · tok · cache · ctx | provider / model`. As width
  shrinks the segments drop tps → tok → cache (ctx is kept), `…` truncation is
  the final loss, and the provider/model block flushes right. The header row is
  gone; its layout height goes to the transcript. 118 P2's header is repealed
  by this decision, and the pass's plan origin (117's P2 row) is recorded
  here; 117 and 118 stay historical for their other fixes (latency, role
  colors, borders).
- **Rendered symbols are claimed exactly.** The row renders `|`, `·`, and the
  `…` truncation marker and nothing more; a repo-wide sweep found no doc
  claiming a literal `—` glyph renders anywhere, and the owner approved
  recording that result on 2026-10-17 (HITL session).
- **Observed only, never fabricated** (decision 102 P2's rule). A segment
  renders only when a real observation exists: tps needs output tokens and a
  measured round time, tok needs both reported counts (a partial report renders
  nothing rather than a fabricated `0/N`), cache needs the cached-token report
  and an input count to divide by, ctx needs the context session. No provider →
  "no provider configured". A reported zero is a real observation and stays.
- **Round time is measured where the round happens.** The provider seam
  (`call_generic`/`call_openai`/`call_anthropic`, after the cancellation check)
  starts a display-only timer and the terminal record consumes it
  (`ReplayHooks::begin_round`/`take_round_millis`). Replay and fake rounds
  observe tokens but never a fabricated wall time; a refused or cancelled
  round observes nothing. The OpenAI-completions streaming body now asks for
  `stream_options.include_usage` so the final usage frame arrives; buffered
  protocols are unchanged. A completions 4xx refusal whose bounded (redacted)
  body names the option is typed: the status-only message appends
  `-- gateway rejected stream_options.include_usage; token usage stays
  unobserved`; any other refusal keeps its plain class, and a protocol whose
  request carries no option can never claim its rejection.
- **Harvest per poll.** `ToolLoopEvent::ResponseStarted` is emitted once per
  prompt, not per round, so per-round attribution cannot hang off events.
  `SessionComposition` harvests the take-once cells after every application
  poll, attributing each round of a multi-round tool turn exactly once instead
  of losing all but the last.
- **The worker re-announces the header.** After a settled turn the loop sends
  `WorkerEvent::Ready` again, before `TurnFinished`, so the row shows fresh
  facts without a command of its own. The stdio frontend is byte-unchanged:
  this is a TUI render and session-worker contract, and stdio keeps draining
  the application directly.
- **Corpus v89.** The `tui-render` subject gains optional provider/model/
  metrics input; the shell-basic and transcript-scroll expectation records are
  re-pinned from candidate output; the post-freeze supersessions, retirements,
  and adversarial fixtures are re-stamped with their refusal codes intact and
  no frozen record edited.

## 3. Evidence

- `usage_metrics_format_real_values_and_drop_order`: the fixture row reads
  `tps 41.0 · tok 12.3k/4.1k · cache 68% · ctx 36%`; the narrow-width drop
  order, the `…` truncation, unobserved rendering nothing, and the whole
  status winning over decorative metrics are all asserted.
- `t4_frames_still_deterministic_without_header`: frame determinism holds with
  the header row gone.
- `streamed_turn_surfaces_usage_once_per_call` and
  `buffered_anthropic_usage_is_normalized_into_the_observation`: live rounds of
  both shapes carry measured round millis through the take-once seam;
  `round_millis_records_with_usage_and_clears_on_other_terminals` pins that a
  round that never began records no time, a replayable round consumes its
  start once, and failures and the evidence path clear it.
- `accumulate_round_adds_only_observed_fields`: absent fields add nothing, a
  reported zero cached count keeps the cache segment alive at 0%, and a partial
  observation (one side reported) renders no token pair and no cache percentage
  while its measured throughput still surfaces — found by the independent
  review and repaired at the root cause (`SessionMetrics` token counts are
  `Option`, so absence is representable).
- `completions_400_naming_stream_options_declares_the_usage_framing_rejected`
  and its two controls pin the typed refusal: a 4xx body naming `stream_options`
  appends the category after the stable status base; a refusal without the
  mention and any non-completions protocol keep the plain class.
- `a_settled_turn_reannounces_the_header_with_fresh_metrics`: the double
  changes its status after the first announcement, so a loop echoing the
  startup snapshot cannot pass; a refused prompt announces nothing extra.
  `the_worker_pushes_the_startup_pane_and_a_settled_one` pins the exact
  ordering: pane, header, turn events, pane, pane, header, `TurnFinished`.
- `a_replayed_round_populates_the_footer_metrics_without_inventing_time`: an
  all-legacy replay store through the real composition surfaces tokens 10/4
  with `provider_millis` 0 and tps `None`.
- `a_multi_round_tool_turn_accumulates_every_round_of_usage`: a replayed
  two-round tool turn (a `workspace.list` call the host executes
  approval-free, then the final answer) through the real composition lands
  13/6 — both rounds of usage attributed; a round lost at the take-once seam
  would read only the last round's 3/2.
- Differential parity held 345/345 applicable required (4 explicit platform
  skips, 0 informational deviations); supersession and retirement guards held
  (8 + 8 adversarial refusals with named codes); `npm run check` exit 0
  (adapters 518 / cli 420 + 28 / core 682 / harness 77).

## 4. Consequence

The TUI spends one row less and says one fact more: identity and measurement
share the bottom line, and the number only appears when the provider actually
reported one. Recorded limits: tps is a session average (owner-specified)
over measured live rounds only; replay and fake sessions show tokens but no
time; the cache segment exists only where a provider reports cached tokens;
the `tui-render` record echo still writes an unobserved count as `0` for
record-shape compatibility (the pinned frame is the artifact that shows the
absence, and the echo's note in the harness subject says so); multi-round
attribution is now end-to-end proven for a replayed two-round tool turn (both
rounds of usage land in the session totals), while live multi-round wall time
still rests on the take-once cells plus the per-poll harvest proven by code
inspection (replay never observes a clock, and no fixture drives live
rounds); and verification is headless plus replayed, not a live terminal.
