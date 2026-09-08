---
title: "The TUI UX Pass 2"
label: "wayfinder:ticket"
status: open
date: 2026-08-31
supersedes: []
---

# Ticket 107 — The TUI UX Pass 2

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Authorized by:** [decision 119](../decisions/119-tui-ux2-entry-review.md)

## Question

The user's second usability round from live TUI usage: the header duplicates the bottom status when no provider is configured, startup is bare (no banner or greeting), the palette filter needs verification against typing, timestamps are UTC instead of local, waiting for the model has no loading indicator, and picking a provider is manual.

## Fixes H1–H6 (authorized by decision 119)

- **H1 HEADER DEDUP:** the header stops duplicating the bottom status when no provider is configured.
- **H2 ASCII BANNER + GREETING:** sessions open with an ASCII banner and greeting.
- **H3 PALETTE FILTER VERIFIED:** the decision 111 palette behavior is verified as typing.
- **H4 LOCAL-TIMEZONE STAMPS:** timestamps move from UTC to the user's local timezone via the time crate (a new CLI-only dependency, recorded).
- **H5 LOADING INDICATOR:** the working status is unmistakable within the synchronous architecture; a live spinner during a blocking model round requires a UI thread, which is an open architecture question.
- **H6 PROVIDER PICKER (scoped):** `/provider` becomes an interactive read-only picker listing configured providers; the add-flow (writing `siralos.toml`) is split to ticket 108 pending a config-write authority decision.

## Red lines

The sanitizer is the single output boundary; the input queue the single read owner; approvals host-gated; no threads in the session path; no persistence; the stdio frontend byte-unchanged; decisions ≤118 untouched.

## Resolution

Open — entry review PASS per [decision 119](../decisions/119-tui-ux2-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 120](../decisions/120-tui-ux-pass-2.md). The `/provider` add-flow is ticket 108 pending a config-write authority decision.
