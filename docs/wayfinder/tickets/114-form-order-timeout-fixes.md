---
title: "The Form Order and Fetch-Timeout Fixes"
label: "wayfinder:ticket"
status: closed
date: "2026-08-31"
supersedes: []
---

# Ticket 114 — The Form Order and Fetch-Timeout Fixes

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 133](../decisions/133-order-timeout-entry-review.md)

## Question

The add-form asks for display name in the middle, the model fetch freezes for 60 seconds on a dead endpoint, and the api key error does not explain the env-var-name distinction.

## Fixes O1–O4 (authorized by decision 133)

- **O1 DISPLAY NAME FIRST:** the field order becomes DisplayName -> Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName. The DisplayName field may be left EMPTY when advancing (no validation error); after the Url field validates, if the display name is still empty the derivation prefill applies (derive_provider_name from the url host); at form completion, an empty display name (no url either) is an error ("a display name is required - enter one or provide a url so one can be derived"). The Up/Down navigation and the Enter/Down advance follow the new order.
- **O2 FETCH FAST-FAIL:** the fetch_models client timeout drops from 60s to 5 seconds (read) and 3 seconds (connect) — a dead or slow endpoint fails within ~5s and the honest fallback note appears instead of a frozen form. The completion-call client (the existing 60s) is UNCHANGED — only the fetch client tightens.
- **O3 API KEY ERROR CLARITY:** when the api key input fails the env-var-name validation AND looks like a secret (contains lowercase letters, or starts with "sk-", or contains characters outside [A-Z0-9_]), the error says: "this looks like the key itself - Siralos stores the NAME of the environment variable holding your key; create it with setx YOUR_API_KEY_NAME \"the-key\" and enter YOUR_API_KEY_NAME here". Otherwise the standard human-readable env-var message (decision 128) applies.
- **O4 THE STALL FIX VERIFIED:** with the 5s timeout the api-key advance cannot exceed ~5s; a test proves the fetch failure path completes quickly with the fallback note (a mocked unreachable endpoint or an injected failure).

## Red lines

The sanitizer is the single output boundary; no threads; the env-only credential policy holds; the stdio frontend byte-unchanged; decisions <= 132 untouched.

## Resolution

Open — entry review PASS per [decision 133](../decisions/133-order-timeout-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 134](../decisions/134-order-timeout-fixes.md).

Closed 2026-09-12: the Resolution line above was written at the ENTRY REVIEW, so the
frontmatter said open while the work was still ahead. The implementation landed and is
recorded in [decision 134](../decisions/134-order-timeout-fixes.md); nothing on this ticket is
left open.
