---
title: "The Provider Add-Form UX Fixes"
label: "wayfinder:ticket"
status: closed
date: 2026-08-31
supersedes: []
---

# Ticket 110 — The Provider Add-Form UX Fixes

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`  
**Authorized by:** [decision 125](../decisions/125-add-form-ux-fixes-entry-review.md)

## Question

The user reported three add-form UX defects from live TUI usage of the provider add-flow: (1) URLs are rejected — typing `https://...` fails because `:` and `/` are not accepted in the Char handler; (2) no visible cursor — form fields do not render a cursor position indicator; (3) no Up/Down field navigation — only Enter advances forward.

## Fixes F1–F3 (authorized by decision 125)

- **F1 URL ACCEPTANCE:** the form's Char handler must accept ALL printable characters including `:` and `/` (the endpoint URL `https://...` needs both). The handler must not filter or intercept `:` and `/` — if it does, fix it to accept every `KeyCode::Char` except control characters.
- **F2 VISIBLE CURSOR:** each form field renders a visible cursor indicator (a block character `█` or underscore at the end of the current input text) so the user can see where they are typing. The cursor is at the END of the input (append-only — consistent with the Backspace-only editing model).
- **F3 UP/DOWN FIELD NAVIGATION:** Up returns to the PREVIOUS field (restoring its validated value for editing — the field's already-validated value is placed in the input buffer for re-editing); Down advances to the NEXT field if the current field's input validates (same validation as Enter); if validation fails, the error is shown and Down is a no-op. The form's field order is Provider -> Model -> CredentialEnv -> Endpoint; Up at Provider is a no-op; Down at Endpoint is the same as Enter (completes the form if valid).

## Red lines

The sanitizer is the single output boundary; approvals host-gated; no threads; no persistence; the stdio frontend byte-unchanged; decisions <= 124 untouched.

## Resolution

Open — entry review PASS per [decision 125](../decisions/125-add-form-ux-fixes-entry-review.md) (HITL 2026-08-31). Implementation tracked in [decision 126](../decisions/126-add-form-ux-fixes.md).

Closed 2026-09-12: the Resolution line above was written at the ENTRY REVIEW, so the
frontmatter said open while the work was still ahead. The implementation landed and is
recorded in [decision 126](../decisions/126-add-form-ux-fixes.md); nothing on this ticket is
left open.
