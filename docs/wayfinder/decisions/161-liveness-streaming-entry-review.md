---
title: "TUI Liveness and Streaming Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "128"
supersedes: []
---

# TUI Liveness and Streaming Entry Review

Ticket [128](../tickets/128-tui-liveness-and-streaming.md) · entry review
[the Session Profile Reload entry review](157-reload-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record -- the inversion, stated plainly.** This entry review was
> written AFTER the change was implemented, verified, and committed (ten commits,
> listed in the ticket, 2026-09-12). There was no prior ticket and no prior entry
> review. The approvals that exist are in-chat: the owner's bug reports, the
> owner's mouse-tradeoff ruling, and the owner's scope approval (S1 + S2 + S3)
> after a comparison of three published harnesses. Nothing more is claimed, and
> nothing is backdated. The verdict below is a retroactive PASS over the
> committed diff, not a pre-commit authorization.

## 2. The Fixes

| Fix                                  | The change                                                                                                                                                                                                                              | The evidence                                                                                                                                        |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 CLEAR AND PAINT BEFORE THE TURN   | The submit state change is a tested helper (`accept_submitted_input`) and the loop paints one frame while a submit is pending -- before the synchronous turn runs.                                                                      | `submitted_input_clears_echoes_and_reports_the_line`, `empty_submit_clears_without_echoing`                                                         |
| F2 INCREMENTAL COLLECTOR             | `ProviderTurnCollector` exposes `push -> TurnStep` + `finish`; `collect_provider_turn` is a thin loop over it, so the same validation serves whole-turn and one-event-at-a-time callers.                                                | `incremental_collector_matches_the_whole_turn_wrapper`                                                                                              |
| F3 STREAM THAT OWNS ITS REQUEST      | `ModelProvider::open_stream` takes the request BY VALUE with an EAGER default, so all fourteen implementors kept their behaviour; only `GenericProvider` overrides it with the real streaming iterator.                                 | 326 adapter tests unchanged; the differential corpus unchanged                                                                                      |
| F4 SSE THAT STILL RECORDS A BODY     | `provider::sse::CompletionStream` assembles deltas into the same events incrementally AND into the equivalent non-streamed body, so recordings stay replayable through the shared converter.                                            | nine SSE tests, including `assembled_body_replays_through_the_shared_body_converter`                                                                |
| F5 ONE PROVIDER EVENT PER STEP       | `Phase::StreamTurn` pulls a single event per step, emits text deltas immediately, and strips them from the committed turn so nothing is emitted twice.                                                                                  | the 16 tool-loop differential scenarios at parity; `without_text_deltas`                                                                            |
| F6 STREAMING IS VISIBLE              | The TUI shares its terminal (`Rc<RefCell<Terminal>>`), owns one non-blocking `draw_now`, caches the last context pane, and the sink asks for a frame per accepted line, throttled to 33 ms.                                             | `sink_requests_a_coalesced_redraw_after_it_changes_the_transcript`                                                                                  |
| F7 KEEP-ALIVE, TYPE-AHEAD, INTERRUPT | One opt-in `ProviderPending` tick before the first pull; `drain_events` calls a `progress` callback for every event, and the TUI's callback repaints, folds typed characters into the prompt, and reports Esc as an interrupt.          | the tick is off by default, so the corpus and unit sequences are byte-identical; harness `canonical_event` maps it to nothing                       |
| F8 CREDENTIAL LIVE AND FAIL-LOUD     | The credential is a live cell and `/reload` resolves and applies it, clears it when a profile stops declaring one, and REPORTS an unresolvable one instead of sending an unauthenticated request.                                       | `reload_applies_the_recomposed_model_to_the_live_session` (credential asserted), `reload_reports_an_unresolvable_credential_instead_of_dropping_it` |
| F9 TOOL NAMES TRANSLATED             | Outbound definitions, replayed calls and inbound calls translate through one alias map; the real capability id stays authoritative inside Siralos, collisions are disambiguated, and an unknown inbound name passes through.            | three `tool_names` tests (dotted/dashed/colon, digit-leading and empty, truncation, collision round-trip)                                           |
| F10 MOUSE DEFAULT OFF                | Capture is off by default so native select/copy/paste work; the wheel scrolls through the terminal's arrow fallback on an empty prompt (history stays on Ctrl+Up/Down and any non-empty input); `/mouse` captures for raw wheel events. | `empty_prompt_arrows_scroll_instead_of_history`, `mouse_toggle_flips_capture_state_with_expected_message`                                           |

## 3. Criteria -> Evidence

| Criterion                                          | Evidence                                                                                  |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Enter clears the prompt and shows liveness         | F1 + F6 + F7                                                                              |
| The answer is visible while it is produced         | F2 + F3 + F4 + F5 + F6                                                                    |
| A credential added mid-session reaches the request | F8 -- asserted on the live credential cell                                                |
| A provider never receives a dotted tool name       | F9 -- asserted on the alias map, not on a label                                           |
| Copy and paste work without ceremony               | F10 -- asserted on the capture default and the arrow-scroll rule                          |
| The pinned behaviour is untouched                  | F3's eager default, F4's body recording, F7's opt-in tick; differential parity 352/352    |
| The repository gate holds                          | `npm run check` exit 0 at `8b3738d` -- 615 core / 326 adapters / 224 cli / 25 conformance |

## 4. Result

**PASS (retroactive), with recorded limits.** The freeze is fixed at its cause
rather than papered over: the turn genuinely streams, the frontend genuinely
repaints, and the interrupt is the Host's own cancellation token. Three limits
are recorded instead of implied: the spinner cannot animate while the thread
waits for the first byte (no read timeout in the blocking client); the reasoning
row (S3) is approved but undelivered; and the named openai/anthropic adapters
plus provider-recorded aliases still need their own follow-through.
