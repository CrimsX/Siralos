---
title: "Session Profile Reload: Live Endpoint and Protocol"
label: "wayfinder:ticket"
status: closed
date: "2026-09-12"
supersedes: []
---

# Session Profile Reload: Live Endpoint and Protocol

Ticket [126](126-session-profile-reload.md) closed `/reload` with its limit
recorded rather than implied: only the MODEL applied live, while provider,
endpoint, protocol and credential changes were reported as needing a restart to
converge (decision [158](../decisions/158-session-profile-reload.md) §3). The
limit had a named cause — `GenericProvider` held the endpoint and the protocol
as plain fields read at construction, so there was no live cell to move.

This ticket consumes the endpoint and protocol half of that limit. The adapters
now hold both as live cells read at request time — the endpoint base, and the
protocol that selects the POST path appended to it — and the apply step moves
them the same way the model moves: only a CHANGED value is written, the live
accessor is what the tests assert (not a label), the endpoint VALUE is never
echoed, and an unchanged field stays a no-op so the pure report path is
untouched. A changed provider NAME and a changed CREDENTIAL still say
`(restart to converge)`: the provider name is display identity for the live
session, and there is no live credential cell.

The same pass corrected a sentence the earlier limit had made false. The
edited-profile report ended `; live session unchanged`, but the apply step runs
in the same command and moves the live cells, so the printed output contradicted
itself:

```text
reload would change: provider unchanged; model example/model-a -> example/model-b; live session unchanged
applied: model example/model-a -> example/model-b (live, no restart)
```

The tail now states the diff only, and each part names its own disposition.
A test pins the contradiction so it cannot come back.

**Still recorded, not implied:** a changed provider name or credential needs a
restart. Nothing here widens what a session may do: the reload applies exactly
what a restart would apply from the same file, and never more, and the
never-widens-authority invariant (decision [157](../decisions/157-reload-entry-review.md),
pinned by `reload_cannot_widen_session_authority`) is untouched.

> **Retroactive record.** This change was implemented, verified, and committed
> (commits `6d74e80`, `283d022`, `f00e4ec`) BEFORE this ticket and its entry
> review were written. There was no prior ticket and no prior entry review; the
> provenance is the owner's in-chat direction on 2026-09-12 to continue the
> work directly, plus the deferred item decision 158 §3 had already recorded —
> not a new design approval. The `/reload` approval that exists (2026-09-11,
> decision [157](../decisions/157-reload-entry-review.md)) covers a reload that
> re-reads declarative configuration only and never widens authority; applying
> the endpoint and protocol live stays inside it. Nothing is backdated; the
> entry review ([159](../decisions/159-reload-live-apply-entry-review.md)) is
> itself retroactive and names the inversion.
