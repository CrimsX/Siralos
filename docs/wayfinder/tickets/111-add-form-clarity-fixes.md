---
title: "The Provider Add-Form Clarity Fixes"
label: "wayfinder:ticket"
status: closed
date: "2026-08-31"
supersedes: []
---

# Ticket 111 — The Provider Add-Form Clarity Fixes

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 127](../decisions/127-add-form-clarity-entry-review.md)

## Question

The user is stuck on the Provider field of the add-form because the form doesn't explain what each field expects. The field label just says "provider" with no hint what to type, the validation error is cryptic regex ("A provider must match [a-z0-9_-]{1,64}"), and the user doesn't know what "endpoint" means. The Up/Down navigation code is correct — the user can't advance because they're typing a URL in the Provider field which wants a short identifier name.

## Fixes D1–D4 (authorized by decision 127)

- **D1 DESCRIPTIVE FIELD LABELS:** each form field renders with a descriptive label and an example: "provider name (e.g. openai, example-vendor):", "model (e.g. model-a, gpt-4o):", "credential env var (e.g. OPENAI_API_KEY):", "endpoint URL (optional, e.g. https://api.openai.com/v1):". The labels replace the current bare "provider:", "model:", etc.
- **D2 HUMAN-READABLE ERRORS:** the validation errors are rewritten in plain English: "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)" instead of "A provider must match [a-z0-9_-]{1,64}"; "Model name must be printable and between 1 and 256 characters (e.g. model-a, gpt-4o)" instead of the current bound message; "Credential env var must be uppercase letters, numbers, and underscores (e.g. OPENAI_API_KEY) - set this variable with your API key before starting Siralos" instead of "A credential env name must match [A-Z0-9_]{1,64}"; "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)" instead of "An endpoint must start with...". Apply the same rewriting to ALL validation errors in the form (provider, model, credential, endpoint).
- **D3 FIELD DESCRIPTIONS:** each form field renders a dim description line BELOW the label explaining what the field is for: provider -> "the name you'll use to identify this provider", model -> "which model to use for completions", credential env -> "the environment variable that holds your API key (set it before starting Siralos)", endpoint -> "the API URL (leave empty for the default)".
- **D4 THE PROVIDER FIELD EXPLAINED:** the Provider field collects a SHORT IDENTIFIER NAME for the provider (like "openai", "example-vendor"), NOT a URL. The URL goes in the Endpoint field. This is made clear by the label + description + error message.

## Red lines

The sanitizer is the single output boundary; no threads; no persistence; the stdio frontend byte-unchanged; decisions <= 126 untouched.

## Resolution

Open — entry review PASS per [decision 127](../decisions/127-add-form-clarity-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 128](../decisions/128-add-form-clarity-fixes.md).

Closed 2026-09-12: the Resolution line above was written at the ENTRY REVIEW, so the
frontmatter said open while the work was still ahead. The implementation landed and is
recorded in [decision 128](../decisions/128-add-form-clarity-fixes.md); nothing on this ticket is
left open.
