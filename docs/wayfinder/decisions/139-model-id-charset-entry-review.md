---
title: "The Provider Model-Id Charset and Bound Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "117"
supersedes: []
---

# The Provider Model-Id Charset and Bound Entry Review

Ticket [117](../tickets/117-provider-model-id-charset.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `11d5eb5` "fix(profile): accept provider-issued model
> ids", 2026-09-10), at the human owner's explicit request. There was no
> prior ticket, no prior entry review, and no prior human approval for
> this change, and this record does not invent or backdate any of them.
> The normal order (entry review authorizes → implementation lands) is
> inverted here: the implementation landed first and this review records
> what it should have authorized. The verdict below is therefore a
> retroactive PASS over the already-committed diff, not a pre-commit
> authorization.

> **User-directed 2026-09-10 (retroactive).** The `[profile]` model-id
> rule allowed only `[a-zA-Z0-9._-]`, so real provider-issued ids such as
> `example/model-a` (vendor separator `/`) and
> `example/model-b:free` (tag suffix `:`) could neither
> be saved by the add-flow (`write_profile_config`) nor applied at load.
> The fix: the charset becomes ASCII alphanumeric plus `. _ - / : @`,
> defined ONCE as `siralos_core::composition::is_model_id_char` and used
> by the core validator, the write boundary, and the TUI form (no
> divergent copy); `MAX_PROFILE_MODEL_BYTES` goes 128 -> 256, matching
> the bound the add-form already told the user it enforced; every
> rejection message states the rule it actually enforces.

## 2. The Fixes

| Fix                          | The change                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | The evidence                                                                                                                                                          |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1 SINGLE CHARSET DEFINITION | New `siralos_core::composition::is_model_id_char`: ASCII alphanumeric or one of `.` `_` `-` `/` `:` `@`, documented as the single definition of the model-id charset. The core `validate_model_field`, the `write_profile_config` write boundary (`siralos-cli::interactive`), and the TUI `validate_model_name` (`siralos-cli::tui`) all enforce this predicate — no second copy of the charset.                                                                                                                                                             | `composition.rs` `is_model_id_char`; `validate_model_field` calls it; `interactive.rs` `write_profile_config` calls it; `tui.rs` `validate_model_name` calls it       |
| F2 BOUND 128 -> 256          | `MAX_PROFILE_MODEL_BYTES` 128 -> 256. The TUI bound check moves from the hardcoded `128` to `MAX_PROFILE_MODEL_BYTES`, so the enforced bound equals the bound the form message already stated (256). The `ProviderAddData.model` doc comment becomes "1 to 256 bytes, no NUL, letters/numbers or . _ - / : @".                                                                                                                                                                                                                                                | `composition.rs` const; `tui.rs` bound check + doc comment; 256-accepted / 257-rejected tests                                                                         |
| F3 HONEST REJECTIONS         | Every rejection message states the enforced rule: core + write boundary `"A model must match [a-zA-Z0-9._/:@-]{1,256}..."` (write boundary appends "with no NUL"); TUI `"Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"` for all three failure modes (empty/over-bound, NUL, bad charset). The write-boundary round-trip test asserts rejections contain "256" or "NUL".                                                                                                                           | `validate_model_field` message; `write_profile_config` message; `validate_model_name` messages; `write_profile_config_model_id_rule_matches_core` rejection assertion |
| F4 ROUND-TRIP COVERAGE       | Two tests pin the rule at both layers: core `model_id_rule_accepts_provider_ids_and_bounds` (accepted `model-a`, `gpt-4o`, `example/model-a`, `example/model-b:free`, `openai/gpt-4o@2024-08-06`, 256 x `a`; rejected empty, 257 x `a`, `has space`) and write-boundary `write_profile_config_model_id_rule_matches_core` (same accepted ids written then `load_workspace_profile`-applied; rejected empty, over-bound, `has space`, `ab\0cd`). The TUI `validate_model_name` test matrix gains the provider ids, the 256/257 boundary, space, and NUL cases. | `model_id_rule_accepts_provider_ids_and_bounds`, `write_profile_config_model_id_rule_matches_core`, `tui.rs` `validate_model_name` assertions                         |

## 3. Criteria → Evidence

| Criterion                          | Evidence                                                                                                                                                                                                                                                                                                                              | Verdict |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Provider-issued ids save AND apply | `write_profile_config_model_id_rule_matches_core`: each accepted id is written by `write_profile_config` and then `load_workspace_profile` returns an applied `Record` with the same model                                                                                                                                            | pass    |
| Core accepts the same ids          | `model_id_rule_accepts_provider_ids_and_bounds` accepted set incl. `/`, `:`, `@` forms and the 256-byte boundary                                                                                                                                                                                                                      | pass    |
| One charset, three call sites      | `is_model_id_char` defined once in `composition.rs`; `validate_model_field`, `write_profile_config`, `validate_model_name` all call it (no inline charset copy remains)                                                                                                                                                               | pass    |
| Bound is 256 everywhere enforced   | `MAX_PROFILE_MODEL_BYTES = 256`; TUI check uses the constant; 256 accepted / 257 rejected at core, write, and TUI layers                                                                                                                                                                                                              | pass    |
| Rejections state the rule          | Core/write messages carry the `[a-zA-Z0-9._/:@-]{1,256}` rule (+ NUL note at the write boundary); TUI message states charset + bound in plain words; round-trip test asserts "256" or "NUL" present                                                                                                                                   | pass    |
| Full gate green (as committed)     | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 213 cli, 0 failed); `npm run check:differential` exit 0, parity 352/352 applicable required, 4 platform skips, 0 deviations; `npm run check:rust-all` green — evidence cited from the orchestrator, not re-run here | pass    |

## 4. Result

Entry review PASS (retroactive): the charset/bound change as committed
in `11d5eb5` is the correct fix — one shared predicate, the 256 bound
the form already promised, honest rejections, round-trip coverage at the
core and write layers. This review authorized nothing (the code had
already landed); it records what authorization would have covered. The
unrelated `check:secrets` failure (untracked gitignored `siralos.toml`
stores a literal key) is an open item outside this change.
