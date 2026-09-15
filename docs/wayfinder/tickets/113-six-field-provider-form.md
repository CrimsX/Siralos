---
title: "The Six-Field Provider Form"
label: "wayfinder:ticket"
status: closed
date: "2026-08-31"
supersedes: []
---

# Ticket 113 — The Six-Field Provider Form

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 131](../decisions/131-six-field-entry-review.md)

## Question

The add-form has four fields with example-heavy labels that feel messy; the user wants six fields in an exact order with no examples in the labels, and the /models fetch wired into the form.

## Fixes S1–S6 (authorized by decision 131)

- **S1 FIELD RESTRUCTURE:** six fields in the user's exact order — (1) url (label "url"; endpoint renamed), (2) api key (label "api key"; stores the ENV-VAR NAME — description: "the environment variable holding your key; set it before starting Siralos"), (3) display name (label "display name"; auto-derives from url host when advanced past url — decision 130 kept), (4) api protocol (label "api protocol"; closed set openai-compatible (default) or anthropic; free text rejected), (5) model (label "model"; picker on fetch success, free text otherwise), (6) model display name (label "model display name"; optional — empty allowed; shown in header/status instead of raw model id). No (e.g. ...) examples in ANY label — descriptions stay short and dim.
- **S2 MODEL FETCH PICKER:** after api key validated and advance, attempt model fetch (decision 124 fetch_models with url + credential env name); while fetching status shows "fetching models..."; on SUCCESS open picker modal (Up/Down wrapping, Enter selects, Esc falls back to free text) filling model field; on FAILURE show honest one-line note ("model list unavailable from this provider - enter the model manually") and continue free-text. Fetch is blocking with freeze documented.
- **S3 ADDITIVE PROFILE KEYS:** ProfileRecord gains protocol (enum OpenAiCompatible | Anthropic; absent -> OpenAiCompatible) and model_display_name (Option<String>; absent -> None). parse with decision 54 absent-transparency; malformed (unknown protocol, oversize display name) -> profile UNAPPLIED with diagnostic. Existing corpus unchanged (keys absent — 352/352). Protocol request-shaping is future work — stored and displayed now.
- **S4 THE WRITE EXTENSION:** write_profile_config gains the two keys — protocol written only when not default (openai-compatible omitted), model_display_name written only when non-empty. toml_edit merge and re-parse validation unchanged.
- **S5 DISPLAY INTEGRATION:** header and status line show model display name when present (fallback to raw model id). /models output unchanged.
- **S6 THE SECURITY NOTE:** api key field NEVER stores secret value — env-var name only (decision 68 env-only policy; secret-hygiene gate stays green).

## Red lines

The sanitizer is the single output boundary; no threads; no persistence beyond the siralos.toml write; the stdio frontend byte-unchanged; decisions <= 130 untouched.

## Resolution

Open — entry review PASS per [decision 131](../decisions/131-six-field-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 132](../decisions/132-six-field-provider-form.md).

Closed 2026-09-12: the Resolution line above was written at the ENTRY REVIEW, so the
frontmatter said open while the work was still ahead. The implementation landed and is
recorded in [decision 132](../decisions/132-six-field-provider-form.md); nothing on this ticket is
left open.
