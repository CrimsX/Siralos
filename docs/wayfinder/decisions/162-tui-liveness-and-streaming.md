---
title: "TUI Liveness and Streaming Turns"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "128"
supersedes: []
---

# TUI Liveness and Streaming Turns

Ticket [128](../tickets/128-tui-liveness-and-streaming.md) · entry review
[161](161-liveness-streaming-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** Everything below was implemented, verified and
> committed BEFORE this record was written. There was no prior ticket and no
> prior entry review; the approvals are in-chat (bug reports, the mouse-tradeoff
> ruling, and the S1 + S2 + S3 scope approval after a three-harness comparison).
> Nothing more is claimed here, and nothing is backdated.

## 2. The Implemented

| Slice                                      | The change                                                                                                                                                                                                                                                                                                                                         | The evidence                                                                                                                  |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| M1 CLEAR AND PAINT (`7365fc3`)             | The submit state change moved into the tested `tui::accept_submitted_input`, and the loop paints one frame when a submit is pending, BEFORE dispatching the synchronous turn -- so the cleared box, the echo and `working` are seen at once.                                                                                                       | two submit tests; 223 cli tests                                                                                               |
| M2 INCREMENTAL COLLECTOR (`c6279ed`)       | `ProviderTurnCollector` owns the bounded turn state and exposes `push -> TurnStep` + `finish`; the whole-turn wrapper is a thin loop over it, with provider-declared terminal outcomes keeping their original precedence.                                                                                                                          | `incremental_collector_matches_the_whole_turn_wrapper`; 105 existing core provider tests unchanged                            |
| M3 SSE ASSEMBLY (`601474b`)                | `provider::sse::CompletionStream` turns SSE into the same events incrementally (text per frame, tool calls accumulated by index, reasoning captured, usage retained) and into the equivalent non-streamed body for recording; `StreamingTurn` reads the open response in bounded chunks with incremental UTF-8 decoding and a plain-JSON fallback. | nine SSE tests; 326 adapter tests                                                                                             |
| M4 THE STREAM OWNS ITS REQUEST (`43514e3`) | `ModelProvider::open_stream(request)` with an eager default; `GenericProvider` overrides it; `provider::open_provider_turn` is the core opener (validate, build, open) that the whole-turn wrapper also uses.                                                                                                                                      | 615 core tests; the differential corpus unchanged                                                                             |
| M5 THE SESSION STREAMS (`c453ac9`)         | `Phase::StreamTurn` pulls one provider event per step, emits each accepted text delta live, checks the Host token between pulls, and strips the already-emitted deltas from the committed turn.                                                                                                                                                    | the 16 tool-loop scenarios at parity                                                                                          |
| M6 THE LOST-ANSWER FIX (`2037768`)         | The gate caught `c453ac9` shipping with a broken differential: live-text detection matched the event SHAPE, so text delivered as `Raw` was never emitted and then stripped away. The collector now reports what each push ACCEPTED (`text_delta_count` / `text_delta_at`).                                                                         | differential parity restored to 352/352                                                                                       |
| M7 VISIBLE STREAMING (`bc16526`)           | The terminal is shared, the loop owns one non-blocking `draw_now`, the last context pane is cached, and `TuiSink` asks for a coalesced frame (16 ms) whenever a line lands.                                                                                                                                                                        | `sink_requests_a_coalesced_redraw_after_it_changes_the_transcript`                                                            |
| M8 KEEP-ALIVE AND INTERRUPT (`8b3738d`)    | An opt-in `ProviderPending` tick before the first pull; `drain_events` takes a `progress` callback called for every event and cancels when it returns true; the TUI's callback repaints, folds typed characters into the prompt, and reports Esc.                                                                                                  | the tick is off by default, so every pinned sequence is byte-identical; the harness maps it to nothing                        |
| M9 CREDENTIAL LIVE (`63f939c`)             | The credential became a live cell: `/reload` resolves, applies, clears and REPORTS it; startup prints an unresolvable credential instead of dropping the auth header silently.                                                                                                                                                                     | `reload_applies_the_recomposed_model_to_the_live_session`, `reload_reports_an_unresolvable_credential_instead_of_dropping_it` |
| M10 TOOL NAMES (`f2335c4`)                 | `provider::tool_names`: a pure sanitizer plus a per-request alias map with collision suffixes; outbound definitions, replayed calls and inbound calls all translate, while the real capability id stays authoritative inside Siralos.                                                                                                              | three tool_names tests                                                                                                        |
| M11 MOUSE OFF BY DEFAULT (`63f939c`)       | Capture is off so native select/copy/paste work; the wheel scrolls via the terminal's arrow fallback on an empty prompt, history keeps Ctrl+Up/Down and any non-empty input, and `/mouse` captures for raw wheel events.                                                                                                                           | `empty_prompt_arrows_scroll_instead_of_history`; `mouse_toggle_flips_capture_state_with_expected_message`                     |

## 3. Scope Limits, Recorded

The spinner does not ANIMATE while the thread waits for the first byte: the
right trigger is a socket read timeout and `reqwest` 0.12's blocking builder has
none, so the frame is painted before the wait, on the first delta, and on every
delta after that. The reasoning row (S3) is approved and NOT delivered here. The
named `openai`/`anthropic` adapters still send dotted tool names, and a recording
captured from a live provider carries aliases (hand-written fixtures and the
corpus are unaffected).

Only `openai-completions` streams. The responses and messages protocols keep the whole-body path, because their SSE shapes differ, so a session on those protocols still shows the old freeze until they are ported -- a limit this record states rather than implies.

## 4. Criteria -> Evidence

| Criterion                                        | Evidence                                                                                                                              |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------- |
| Enter clears and the UI shows it is working      | M1 + M7 + M8                                                                                                                          |
| The answer is painted while it is produced       | M2-M7                                                                                                                                 |
| An interrupt reaches Host cancellation authority | M8 -- the drain calls the application's own `cancel`; the provider never gains mutation authority                                     |
| A mid-session credential reaches the request     | M9                                                                                                                                    |
| The provider never sees a dotted tool name       | M10                                                                                                                                   |
| Copy and paste work by default                   | M11                                                                                                                                   |
| Pinned behaviour is untouched                    | the eager `open_stream` default, body-shaped recordings, the opt-in tick; differential parity 352/352, 4 platform skips, 0 deviations |
| The repository gate holds                        | `npm run check` exit 0 at `8b3738d` -- 615 core / 326 adapters / 224 cli / 25 conformance, 0 failed                                   |

## 5. Result

The freeze is gone at its cause. A turn is now a stream the frontend can paint,
interrupt and type into, and the three follow-up bugs the owner hit live
(credential never applied, dotted tool names rejected, mouse capture stealing
copy and paste) are fixed with tests rather than explained away. The remaining
limits are named in section 3, and the next slice (S3, the reasoning row) is
already scoped in the ticket's brief.
