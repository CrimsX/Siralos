---
title: "The Multi-Model Evaluation Run: The Offline Half, Delivered"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "135"
supersedes: []
---

# The Multi-Model Evaluation Run: The Offline Half, Delivered

[Ticket 135](../tickets/135-multi-model-evaluation-run.md) ·
[decision 178](178-evaluation-rows-ruled.md) ·
[decision 94](94-sensitivity-sweep-rule.md) ·
[decision 102](102-provider-interface-takeaways.md) ·
[Map](../siralos-roadmap.md)

## 1. What was built

Three pieces, no new dependency, no new persistence:

- **Core (`siralos-core::evaluation`)** — the pure shapes: `RunOutcome` (provider,
  model, corpus identity, cases run/passed, turns, tool rounds, reported usage,
  failure count plus bounded summaries, cancellation, wall time), `ComparisonRow`,
  `ComparisonTable`, `compare_runs`, `render_comparison`, `UsageTotals`, and
  `task_set_digest`. `ComparisonTable::informational` is `true` and no API can make it
  anything else — decision 94's label, pinned by construction rather than by comment.
- **Runner (`siralos-cli::evaluation`)** — `evaluation_corpus()` (three bounded,
  semantic cases: `arithmetic`, `capital`, `json-object`), `run_evaluation`,
  `evaluate_targets`, `render_records_json`. Each run composes ONE session through the
  same `compose_session` both frontends call, so an evaluation cannot see a session the
  product does not. The drain is headless: it reads the same `WorkerSession` seam, keeps
  only what the record needs, and renders nothing — there is no second render path to
  fork. A turn is bounded twice (4096 events, and a wall-time deadline) and exceeding
  either cancels the turn and records a failure, so an evaluation can never hang the
  gate.
- **Live entry (`siralos-harness evaluate`, `npm run evaluate`)** —
  `--run <label>=<workspace>` repeated, `--out <file>`, optional `--turn-timeout-ms`.
  Each target's provider, model and credential resolution come from that workspace's own
  `siralos.toml`. It is opt-in, owner-run, and never part of `npm run check`, because it
  spends real provider budget.

## 2. The offline proof, and what it actually shows

The gate test is the `siralos-cli` lib test
`evaluation::tests::two_recorded_models_and_the_echo_fake_produce_records_and_one_table`,
which runs inside `npm run check` through `cargo test --workspace --all-targets
--all-features`. It composes three targets over ONE task set (digest
`a8231395c95c154126faf6b16495e6e61dd7e0a16a60563b97514881dad12b13`) and asserts:

```text
Multi-model evaluation (INFORMATIONAL -- evidence only, never a gate)
  corpus siralos-evaluation-smoke (a8231395)
  provider / model | cases | turns | tools | in/out/cached tokens | failures | cancelled
  deterministic-fake / echo | 0/3 | 3 | 0 | -/-/- | 3 | no
  deterministic-fake / recorded-good | 3/3 | 3 | 0 | -/-/- | 0 | no
  deterministic-fake / recorded-weak | 2/3 | 3 | 0 | -/-/- | 1 | no
```

The fake is not flattered. It echoes, the task set is semantic, so it scores 0/3 and the
record says so instead of being tuned to look good. The two recorded models answer the
same task set differently (3/3 and 2/3, with the mismatch naming its case), which is the
property a comparison must have to be worth reading. All three records carry the same
task-set digest, and the rendered table is asserted byte-equal to the sanitized core
render.

## 3. Two deviations from the frozen contract, and why

**The replay recording is built in the test, not checked in.** Ticket 135's contract said
"a checked-in replay recording standing in for a second model". There is no checked-in
store to reuse: the `replay-store` differential fixture
(`tests/differential/corpus/replay-store.json`) is an 8-line scenario descriptor with an
empty input, not a store. More importantly, a checked-in store would be a byte copy of
the writer's format that can drift while the test keeps passing on the stale bytes. The
proof now builds its stores through the product's own bounded writer
(`siralos_adapters::replay_store::write_replay_store`) and then lets the session load and
serve them, so write → load → replay is exercised end to end instead of one direction of
it.

**Three runs, not two.** Acceptance 1 said "two models (fake + replay)". Two were not
enough to be honest: the fake scores 0/3, so a single store would leave exactly one real
datapoint and the table would compare a model against an echo. The third target costs
milliseconds and gives the comparison a difference to report.

## 4. Findings that belong in the record

- **The deterministic fake cannot be recorded.** The fake's own recording reason is
  "deterministic-fake records no HTTP responses (inherently deterministic echo)", and
  `HostProvider::with_replay_support` wraps only the HTTP providers, so a
  `record-replay = true` run with the fake flushes an EMPTY store. Build the store
  directly, as the proof does. The record → flush path itself stays covered by the
  provider-replay subject (corpus v54) and the replay-store subject (v56).
- **Token columns are live-only evidence.** Playback serves the recorded body and does
  not re-report the recorded identity's usage, and the recorder exists only in a
  `record-replay` run — so a replay record's token fields are `None` and render as `-`.
  The proof asserts the dashes rather than fabricating zeros. A live `record-replay` run
  against a real provider fills them through decision 102 P2's capture path; that is the
  owner-run half.
- **Nothing that should stay home crosses into a record.** The identity is provider +
  model only. `SessionStatus` carries an endpoint and a redacted credential display;
  neither is copied, and `RunOutcome` has no field for them. A test asserts the emitted
  JSON parses, carries no workspace path, and contains no `credential`, `endpoint` or
  `env:` substring.
- **Usage is summed, never averaged, and absent stays absent.** `UsageTotals` fields are
  `None` unless at least one recording reported them; no zero is fabricated.
- **`task_set_digest` is deliberately not `create_corpus_evidence`.** The existing corpus
  evidence binds the match outcomes, so two providers answering the same corpus
  differently would carry different digests. The comparison needs one identity for the
  task set, so the new digest binds the corpus id and the ordered `(id, prompt, expected)`
  triples only — asserted distinct from the evidence digest.
- **The provider column shows the profile's provider, not the recording's.** A replay
  target reports `deterministic-fake` because that is the id its profile declares and the
  replay provider is constructed with it; the model label is what distinguishes recorded
  runs. A live run against real providers gets distinct provider ids.

## 5. What is NOT done

Acceptance 2 of [ticket 135](../tickets/135-multi-model-evaluation-run.md) — a live run
demonstrated once by the owner and recorded. It spends real provider budget, so the call
is the owner's and the ticket stays open for exactly that. The entry point is built,
documented and exercised end to end against recorded targets (exit 0, the record above),
but no live provider was called by this work, and this record does not claim otherwise.

## 6. Verification

- `npm run check` exit 0 on the delivered commit: prettier, eslint, doc links,
  project-context, identity, public-hygiene, secrets, Rust architecture, differential
  parity (352/352 applicable required, 4 platform skips, 0 deviations, pinned v32 oracle
  untouched), fmt, clippy with warnings denied, and the full test run.
- `siralos-cli` lib tests 284 (was 279): five new — the three-target proof, the
  unconfigured-workspace label, the failure-count/sampling bounds, the turn-bound proof
  (a session double that stalls into the wall-time bound and floods into the event bound),
  and the empty-target refusal. Core gained four asserts in its own `evaluation` module.
- Nothing in the runner is left unbounded: a turn is bounded by events AND wall time, and
  both bounds are proven to cancel and record rather than hang.
- The live command was exercised against the three recorded targets the test builds
  (`--run good=… --run weak=… --run echo=… --out …`), exit 0, printing the same table and
  writing the same digest-bound record.
- Acceptance ledger: 1 met (offline proof in the gate); 2 NOT MET (owner-run, above);
  3 met (hygiene assertions plus the JSON parse test); 4 met
  (`informational: true`, pinned in core and asserted in the CLI test).
