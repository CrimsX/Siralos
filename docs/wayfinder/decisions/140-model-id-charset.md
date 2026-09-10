---
title: "The Provider Model-Id Charset and Bound"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "117"
supersedes: []
---

# The Provider Model-Id Charset and Bound

Ticket [117](../tickets/117-provider-model-id-charset.md) · entry review
[139](139-model-id-charset-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `5520a41` "fix(profile): accept provider-issued model
> ids", 2026-09-10, 3 files:
> `crates/siralos-core/src/composition.rs`,
> `crates/siralos-cli/src/interactive.rs`,
> `crates/siralos-cli/src/tui.rs`) BEFORE this record was written, at the
> human owner's explicit request. There was no prior ticket, no prior
> entry review, and no prior human approval for this change; this record
> invents none of them and backdates nothing. The entry review ([139](139-model-id-charset-entry-review.md))
> is itself retroactive and names the inversion.

## 2. The Implemented

| Fix | The change | The evidence |
| --- | --- | --- |
| M1 SINGLE CHARSET DEFINITION | `siralos_core::composition::is_model_id_char` — ASCII alphanumeric or `.` `_` `-` `/` `:` `@` — is the one definition of the model-id charset. Core `validate_model_field` (`!value.chars().all(is_model_id_char)`), `write_profile_config`, and TUI `validate_model_name` all call it; the old inline `[a-zA-Z0-9._-]` copies are gone. | `composition.rs` predicate + three call sites; no divergent copy |
| M2 BOUND 128 -> 256 | `MAX_PROFILE_MODEL_BYTES` 128 -> 256. The TUI length check (`value.len() > 128`) becomes `> MAX_PROFILE_MODEL_BYTES`; the stale `tui.rs` comment claiming "validation rule unchanged (128 bound...)" is replaced with the true rule. | `composition.rs` const; `tui.rs` bound + comment |
| M3 HONEST REJECTIONS | Core: `"A model must match [a-zA-Z0-9._/:@-]{1,256}."` Write boundary: `"A model must match [a-zA-Z0-9._/:@-]{1,256} with no NUL."` TUI (all three failure modes): `"Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"`. | rejection strings in the committed diff; round-trip assertion on "256"/"NUL" |
| M4 TESTS | Core `model_id_rule_accepts_provider_ids_and_bounds`; write-boundary `write_profile_config_model_id_rule_matches_core` (write-then-`load_workspace_profile`-applied round trip); TUI `validate_model_name` matrix extended (provider ids, 256/257 boundary, space, NUL) with the empty-error expectation updated to the new message. | the three tests in the committed diff |

## 3. Criteria → Evidence

| Criterion | Evidence | Verdict |
| --- | --- | --- |
| Provider ids save and apply end-to-end | `write_profile_config_model_id_rule_matches_core` passes (targeted round-trip test) | pass |
| Core rule matches the write boundary | `model_id_rule_accepts_provider_ids_and_bounds` passes; both layers share `is_model_id_char` and the 256 bound | pass |
| TUI enforces the same rule with honest errors | extended `validate_model_name` assertions pass; every failure mode states charset + bound | pass |
| No regression | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 213 cli, 0 failed); `npm run check:differential` exit 0, parity held 352/352 applicable required, 4 platform skips, 0 deviations; `npm run check:rust-all` green (evidence gathered by the orchestrator and cited here, not re-invented) | pass |
| Docs/context gates | `npm run check:docs` and `npm run check:context` outcomes reported below; `npm run check` NOT run (red at `check:secrets` for the unrelated pre-existing gitignored-`siralos.toml`-stores-a-literal-key reason) | reported |

## 4. Result

The model-id charset/bound change is complete as committed in `5520a41`:
provider-issued ids (`/` vendor separator, `:` tag suffix, `@` pin)
save through the add-flow and apply at load; one shared predicate owns
the charset; the bound is 256 everywhere the form promised 256; every
rejection states its rule. Retroactive record closed; ticket 117 done.

Open item (not this change): `npm run check` fails at `check:secrets`
because the untracked, gitignored workspace file `siralos.toml` stores a
literal key — pre-existing and unrelated. That file and that script were
not touched.

(End of file - total 48 lines)
