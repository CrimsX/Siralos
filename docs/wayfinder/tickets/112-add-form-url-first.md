---
title: "The Add-Form URL-First Reorder"
label: "wayfinder:ticket"
status: closed
date: "2026-08-31"
supersedes: []
---

# Ticket 112 — The Add-Form URL-First Reorder

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 129](../decisions/129-url-first-entry-review.md)

## Question

The user's mental model is "I have a URL and a key" — the form asked for an abstract provider NAME first and they got stuck typing the URL into it.

## Fixes R1–R3 (authorized by decision 129)

- **R1 FIELD REORDER:** the form field order becomes Endpoint URL -> Model -> Credential env -> Provider name. The Up/Down navigation and the Enter advance follow the new order. The Esc cancel and the completed-data collection are unchanged.
- **R2 NAME AUTO-DERIVATION:** when the user advances past the Endpoint URL field, the Provider name field PRE-FILLS with a name derived from the URL host: take the host (strip the scheme), strip a leading "api." prefix, take the first dot-separated label, lowercase, replace every character outside [a-z0-9_-] with '-', truncate to 64. Examples: https://api.example-vendor.com/v1 -> example-vendor; https://api.openai.com/v1 -> openai; https://vendor.example.com -> vendor. The prefill is EDITABLE — the user can clear it and type their own name. If the URL was left empty, the name field starts empty (the old name-only flow).
- **R3 THE BOTH-FLOWS GUARANTEE:** URL-first flow: URL -> model -> credential -> auto-derived name (editable) -> complete. Name-only flow: empty URL -> model -> credential -> typed name -> complete (the default endpoint applies). Both flows produce valid completed data.

## Red lines

The sanitizer is the single output boundary; no threads; no persistence; the stdio frontend byte-unchanged; decisions <= 128 untouched.

## Resolution

Open — entry review PASS per [decision 129](../decisions/129-url-first-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 130](../decisions/130-url-first-reorder.md).

Closed 2026-09-12: the Resolution line above was written at the ENTRY REVIEW, so the
frontmatter said open while the work was still ahead. The implementation landed and is
recorded in [decision 130](../decisions/130-url-first-reorder.md); nothing on this ticket is
left open.
