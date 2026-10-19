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

## 5. Owner-directed revision (2026-10-18): the row's internal layout

One day after this decision landed, the owner re-decided the bottom row's
internal layout (in-conversation revision; sections 1–4 above stay as the
2026-10-17 record). The two-group rule replaces the three-block rule of
section 2's `compose_bottom_row` description:

- The row is `provider / model            | tps 42.1 · tok in/out · cache %`
  (… ` · ctx %` last): provider/model in green at the LEFT edge, usage
  metrics in dark gray flushed RIGHT behind a single `" | "` separator that
  exists only while any metrics segment survives; no separator when the
  metrics group is empty. When no provider is configured the plain
  `no provider configured` text takes the left edge itself.
- The status segment leaves the footer entirely — "ready" and the turn-state
  word render nowhere in the row. The pulsing liveness row above the input
  (`working`/`working..`/`working...`, owner QoL 2026-09-12) keeps carrying
  the working state; `TuiState.status`, the worker's `SessionStatus.status`
  re-announcement, and every transient/notice write are unchanged plumbing
  (the `/panels` view still surfaces the status hint).
- Width policy: the metrics segments drop `tps` → `tok` → `cache` so the
  provider/model block stays whole (`ctx` is lost last, only through
  truncation); when even the surviving metrics leave no room the metrics
  group truncates with the same `…` marker, and the provider/model block
  truncates in place only on absurdly narrow frames. Same segments, same
  formulas, same observed-only rule as section 2 — no usage capture, replay
  digest, or streaming refusal changed.
- Evidence: footer tests rewritten for the two-group row including
  `footer_has_no_status_or_turn_state_text` and the truncation-priority
  assertions (`usage_metrics_format_real_values_and_drop_order` now pins
  the whole/wide/squeeze/clip/absurd ladder); the `t4_frame_*` mirrors kept
  their transcript-window, pane, and modal assertions. Corpus v90:
  manifest version + digest regenerated, the four `tui-render` expectation
  records re-pinned from fresh candidate output with the printed audit, the
  supersession/retirement envelopes and their negative guards re-stamped,
  the two stale corpus-mismatch fixtures and zero-digest pairs preserved;
  the frozen oracle stays byte-identical. Pinned differential held parity
  345/345 (4 platform skips, 0 deviations); guards held (8 + 8 named
  refusals); workspace tests green (adapters 521 / cli 422), harness 77.

## 6. Requested prompt caching (2026-10-18): the cache% gains a cause

The observed-only rule of section 2 is unchanged: the cache segment still
renders only what the provider reports, a reported zero stays 0%, and an
unreported cache count renders nothing. What changes is that the provider
requests now ask for caching, so the reported number reflects a real
retrieval rather than an accidental one:

- Both Anthropic message bodies (the named adapter and the generic
  protocol shim) request prompt caching by the deterministic owner
  convention: the system block is promoted to its one-block array form
  carrying an `ephemeral` `cache_control` marker, and, when the
  conversation brings at least two message blocks, the penultimate
  message's last content block carries the marker too — the trailing
  message is the round's volatile content and stays unmarked, so each
  round's marker extends the prefix the next round reuses. Placement is a
  pure function of the assembled conversation: identical requests assemble
  identical bodies (no randomness, no timestamps, at most two breakpoints,
  API cap four).
- The OpenAI-completions body carries the fixed literal
  `prompt_cache_key: "siralos"` so compatible gateways route every request
  of a session into the same cache namespace; decision 102's
  measurement-only refusal is reversed by owner direction and recorded in
  that decision's adoption note (§5 there), which stays historical
  otherwise.
- Replay neutrality is proven by construction and by test: the neutral
  request digest hashes the structured `ModelRequest` plus route strings
  and never the wire body (`provider/record.rs`), so the existing
  recordings replay unchanged — `legacy_playback_uses_only_unbound_entries_in_a_mixed_cache`
  and `route_bound_composer_records_and_replays_the_live_route_digest`
  pass untouched.
- Corpus answer (W6): no subject pins the Anthropic wire request body —
  the 350 corpus scenario inputs carry zero "anthropic" occurrences and
  the pinned expectations carry zero usage/request-digest fields; the four
  `tui-render` records pin footer frames, which do not change. No corpus
  amendment needed; parity and oracle byte-identity were verified with
  this change in place (no `CORPUS_VERSION` bump).
- Tests: `anthropic_request_carries_the_system_and_second_to_last_message_breakpoints`,
  `anthropic_request_keeps_the_historical_shape_without_a_stable_penultimate`,
  `anthropic_request_breakpoints_are_identical_for_identical_conversations`,
  `generic_anthropic_protocol_body_carries_the_identical_breakpoints`,
  `completions_body_carries_the_stable_prompt_cache_key`, and the footer
  chain `two_turn_observation_surfaces_cache_reads_on_the_second_turn`
  (turn 1 zero read → 0%, turn 2 reads → the composed row renders the
  percentage). The no-fabrication battery is untouched and stayed green:
  `observation_anthropic_cache_is_normalized` (creation counts into the
  prompt total, never the cached count), `accumulate_round_adds_only_observed_fields`,
  and the stream_options refusal pair.
- Recorded limit: below the provider's cacheable minimum the reported
  read is zero and the footer truthfully shows 0% — asking for caching
  never fabricates a hit; live verification of the climbing percentage
  rests on a real provider session on the owner's machine.
