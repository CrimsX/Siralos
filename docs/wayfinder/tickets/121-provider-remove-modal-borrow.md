---
title: "Scope the Modal Borrow So Provider Removal Does Not Panic"
label: "wayfinder:ticket"
status: "open"
date: "2026-09-10"
supersedes: []
---

# Scope the Modal Borrow So Provider Removal Does Not Panic

Choosing "- Remove provider" in the TUI opened the confirmation modal
and the app died the moment the user answered it (y, n, or Esc); the
provider was never removed. The event loop's modal branch held the
`RefMut` created in its if-let scrutinee for the whole body, so the
first `borrow()` inside the body panicked with "RefCell already
mutably borrowed" under edition 2024. The fix: take the decision
inside a block that ends the mutable borrow before the body
re-borrows, extracted as `handle_pending_approval_key()` so the loop's
decision path is unit-testable; the dormant approval path still
reports `Approved.`/`Denied.`, and removal still resolves through the
single outcome both frontends call.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `0dadfca`) BEFORE this ticket and its entry review
> were written. There was no prior ticket and no prior entry review
> for this change; the provenance is an owner bug report in-session
> about a feature that had just shipped ("unable to remove providers,
> crashes app") — nothing more is claimed here, and nothing is
> backdated. The entry-review inversion is named in [the retroactive
> entry
> review](../decisions/147-provider-remove-modal-borrow-entry-review.md):
> the record follows the implementation rather than authorizing it.

Authorized by
[the Scope the Modal Borrow So Provider Removal Does Not Panic Entry Review (retroactive)](../decisions/147-provider-remove-modal-borrow-entry-review.md).
Implemented and recorded in
[decision 148](../decisions/148-provider-remove-modal-borrow.md).

Note that this record's own feature (ticket 120, decisions 145/146)
shipped with this defect reachable: the deletion feature exposed the
panic without introducing it.

(End of file - total 41 lines)
