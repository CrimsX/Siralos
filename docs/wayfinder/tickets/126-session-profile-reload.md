---
title: "Session Profile Reload (/reload)"
label: "wayfinder:ticket"
status: closed
date: "2026-09-11"
supersedes: []
---

# Session Profile Reload (/reload)

The session composed its profile ONCE at startup (`compose_session` ->
`load_workspace_profile`) and threaded the resolved provider, model, credential,
endpoint and protocol through the event loop as snapshots, so every change to
`siralos.toml` needed a restart. The owner asked why ("why do i have to restart
for provider changes to take effect") and approved a reload command that
re-reads declarative configuration without restarting, on the stated invariant
that a reload may never widen what the session may do.

Delivered in three ordered slices. The REPORT half re-reads through the same
loader and composition path startup uses and reports what would change as a
PURE function -- it cannot mutate state by construction. The APPLY half moves a
changed model into the live provider cell the NEXT request reads and adopts the
profile file's display name for it (the file owns that value, so unlike `/model`
it is adopted rather than cleared); the display name travels with the model so
the status line cannot keep showing a name that describes the old one. An
invalid profile changes nothing and reports the validation diagnostic verbatim;
an absent profile follows startup semantics. A reload NEVER writes the profile
file, and an unchanged model is a no-op, which keeps the pure report path
byte-identical. The AUTHORITY slice pins the invariant: a profile that asks for
more than the Host grants is refused with the effective rules left exactly the
Host's own, and the refused profile never reaches the apply step.

**Honest limit, recorded rather than implied:** provider, endpoint, protocol and
credential changes are REPORTED as needing a restart to converge. Applying them
live needs the same live-cell treatment inside the adapters, which this ticket
does not deliver.

> **Retroactive record.** This change was implemented, verified, and committed
> (commits `ba66019`, `1bf4f55`, `453e31d`, `54128d6`) BEFORE this ticket and its
> entry review were written. There was no prior ticket and no prior entry review;
> the approval that exists is an in-chat design approval from the human owner on
> 2026-09-11 for a reload command with the never-widens-authority invariant --
> nothing more is claimed here, and nothing is backdated. The entry review
> ([157](../decisions/157-reload-entry-review.md)) is itself retroactive and
> names the inversion; the implementation record is
> [158](../decisions/158-session-profile-reload.md).
