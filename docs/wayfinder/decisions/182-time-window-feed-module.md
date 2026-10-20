---
title: "The First Extension Is a Clock: the time.window Feed and the deepseek-hours Module"
label: "wayfinder:decision"
status: "accepted"
date: "2026-10-19"
supersedes: []
---

# 182 — The First Extension Is a Clock: the `time.window` Feed and the `deepseek-hours` Module

Map · [ADR 0039](../../adr/0039-modular-terminal-ui-components.md) · [ADR 0037](../../adr/0037-module-composition-and-godot-decoupling.md) · [ADR 0036](../../adr/0036-lean-product-composition-and-extension-model.md) · [181](181-single-row-footer.md)

## 1. The report

Owner-directed, 2026-10-19 (session HITL): build the first custom module — a
TUI calculator showing the current time and a countdown to the next
peak/off-peak switch of the DeepSeek hourly pricing windows. The owner chose
the lands through four asks: a panels component (the F2 view, ADR 0039's
seam); a generic `time.window` feed rather than a deepseek-shaped one; refresh
on every F2 entry; and a holiday parameter, empty by default. When asked
whether the module would be committed, the owner fixed the boundary for
everything downstream: **the module is a user's customization, not core to
Siralos itself — nothing of it enters git.** That splits the work in two: a
committing half (the feed, the product capability the module consumes) and a
never-committing half (the module package under `modules/`, an ignored
folder). The `time.window` feed is the product surface — like the ADR 0033
corpus, it must be acquisition-proof and verified — while the module is an
extension the way a skill is an extension: authored once, installed from a
folder, installable and uninstallable through the existing module commands.

The two halves resolve the owner's mid-build confusion exactly: the feed is
the machine behind the clock (a closed route the host provisions, the
extension API), the module is the data carried on it (the application, a
pure-data manifest with no code). The feed stays committed; the module stays
ignored.

## 2. The rule

- **One new feed, no new authority.** `UiFeed::TimeWindow` (`time.window`)
  joins the closed seven-feed enum in `siralos-core::ui::spec`; the resolver
  (`siralos_adapters::ui_feeds::resolve_time_window`) reuses the existing
  feed context: bounded params (≤8 per spec, ≤8 specs — unchanged ADR 0039
  §1 bounds map to windows ≤8, holidays ≤64, 400-day scan), typed refusals
  with the same bounded-parse grammar discipline, and
  `feed_capability → None` — no capability gate, because pure clock
  arithmetic on injected inputs needs no authority (§2's "none" rows are
  already-visible or provider-visible state; this one needs nothing at all).
- **Three calm lines, expressed data.** The render is exactly:
  `now YYYY-MM-DD HH:MM UTC (peak|off-peak)`, then
  `peak W & W UTC days[ (label)]`, then
  `switch in Nh Nm|Nd Nh (YYYY-MM-DD HH:MM UTC)` — or, when honesty
  requires it, `now: no wall clock` and `switch: no wall clock`, and
  `switch: none within 400 days` when the schedule itself has no next
  boundary in the scan window. Windows are `[start,end)`, so 01:00 exactly
  already peak-verdicts and 04:00 exactly already off-peak-verdicts; the
  countdown to a boundary in the future is exact by construction (the footer
  decided no fabricated zero; the feed refuses to fabricate a boundary the
  same way).
- **No wall clock, no verdict — ever.** The clock enters as
  `FeedContext.now_utc_ms: Option<u64>`, passed from host time through
  `UiFeedInputs.now_utc_ms` (`now_utc_millis()` at every live caller);
  `None` (pre-epoch or harness-injected absent clock) renders the honest
  lines rather than an invented verdict. The harness subject takes the
  clock from scenario input (`now_utc_ms`), making the moment-bearing feed
  fully controlled — the corpus never reaches for the host wall.
- **Refresh on entry, not on emptiness.** The F2 arm now sets
  `panels_refresh_pending` unconditionally and the loop's
  `refresh_panels_if_requested` dropped its staged-snapshot guard: a staged
  snapshot from last week may not keep showing an aged verdict. A clock is
  moment-bearing by nature; the entry-time refresh is the general policy,
  and moment-bearing feeds did not exist when the guard was written (the
  guard protected a fetch cost that turned out to be the wrong axis).
- **The module is never committed.** `modules/deepseek-hours/` holds a
  `module-manifest.toml` (id `deepseek-hours`, declared digest, ABI
  `siralos:domain-abi@1.0.0`, `capabilities = []`, one component:
  feed `time.window`, windows `01:00-04:00,06:00-10:00`, days `mon-fri`,
  holiday list empty/none until the owner fills it — DeepSeek's current
  windows run Monday-Friday excluding Chinese public holidays, and the
  owner declined the auto-acquisition of a calendar feed in the same
  breath that refused committed modules) plus a README. `.gitignore`
  gains one product line — `/modules/` — mirroring ADR 0036 §11's
  user-session-data rule (`**/siralos.toml` was already ignored); the
  differential audit walks untracked files, so the folder must be invisible
  to git by rule, not by hope.
- **Corpus v91, freezer untouched.** Four `time-window.*` render scenarios
  (peak schedule with label, day-horizon countdown into the weekend, a
  holiday off-peak day, the no-wall-clock honesty case) with
  candidate-authored expectation records harvested from the real candidate
  (the same v85 amendment recipe); the manifest regenerated, `CORPUS_VERSION`
  moved to 91 in `contract.mjs` and `harness.rs`, the supersession and
  retirement envelopes re-stamped with recomputed entry digests, and the
  negative guards untouched (their stale stamps still refuse correctly).
  Parity held 349/349 applicable required (4 explicit platform skips, 0
  deviations); `oracle.json` stays byte-identical.

## 3. Evidence

- Feeds and math: 11 passing `ui_feeds::tests::time_window_*`
  (`time_window_resolves_the_deepseek_schedule` — Tue 2026-10-20 02:23 gives
  `peak` and `switch in 1h 37m (2026-10-20 04:00 UTC)`;
  `time_window_counts_to_the_next_window_across_days_and_weekends` — Wed and
  Sat cases skip to Monday; `time_window_treats_holidays_as_off_peak_and_renders_the_label`;
  `time_window_boundaries_are_exact` — 01:00 peak "3h 0m", 04:00 off-peak
  "2h 0m"; `time_window_refuses_malformed_params` — midnight wrap, overlap,
  bad clock/dates/day names, over-capacity, label >256 chars;
  `time_window_without_a_wall_clock_is_honest`;
  `time_window_reports_no_boundary_when_holidays_cover_the_scan`;
  `time_window_feed_is_open_to_every_spec` (capability `None`);
  the `spec.rs` round-trip pins `time.window` in the closed enum).
- Product seam: `UiFeedInputs.now_utc_ms` threaded through the session
  (`ui_preview` + `panels_report`), stdio preview, and the harness subject;
  the interactive preview test passes a fixed clock; the refresh flip is
  pinned by `entering_panels_view_arms_refresh_every_time` and
  `panels_refresh_fetches_on_entry_and_reentry` (closed view stays quiet;
  re-entry re-fetches with a staged snapshot present).
- Corpus: `time-window.render-deepseek` (02:23 UTC weekday verdict + label +
  the `merge` list line `deepseek-hours - DeepSeek hours (time.window, tab,
module:mod, enabled)`), `.render-day-horizon` (Sat 18:23 →
  `switch in 1d 6h (2026-10-26 01:00 UTC)`), `.render-holidays` (holiday
  → off-peak, switch to the next morning), `.render-no-clock` (absent
  clock) — all four pinned as expectation records; manifest v91 digest
  `b971050d70c8783e49b7e7817f64edc59fddfda216135a145fb1594be442427d`,
  354 scenarios, 149 expectation records, supersession/retirement guards
  held (8 + 8 named refusals).
- `npm run check` exit 0 with all slices in place (workspace tests green:
  core 682 / adapters 521+11 / cli 422; harness 77; identity/public hygiene
  without the ignored module folder).

## 4. Consequence

The lean product says modules are explicitly installed optional
intelligence and extensions are data ADR 0036 declares; `deepseek-hours` is
the first one either definition produced. The recorded limits: the holiday
calendar is owner-maintained (no acquisition, no refresh feed — the panel
truthfully shows a normal peak day until the holiday is listed); the feed
verdict is UTC-only (a converter's job, not a schedule's); and the
reachability catalogue still counts `provider/anthropic.rs` as
product-reachable (registry.rs:270 constructs it), unchanged by this
work. The next extension, if asked for, now has a worked example: contribute
a feed when a closed route genuinely fits, then hand the data over and
ignore the folder.

## 5. Amendment 2026-10-19 — spec-only enable (owner follow-up)

The owner immediately hit the frontier behind §2: `/modules-enable
deepseek-hours` refused with `manifest does not name a component; cannot
enable without bytes`. The interactive enable path constructed a
component host unconditionally, while the manifest layer (ADR 0039 §7)
never required one — `verify_component` already accepted componentless
manifests, and core's `ModuleLifecycle` supports the exact hostless
sequence (`install(package)` → `enable()`) with no bytes involved.

Decision (owner confirmation in session, Option A): spec-only modules
enable hostlessly through the pure core state machine. Activation is
refused with the typed message `manifest does not name a component;
nothing to activate` — activation exists to bind execution authority
for a live component, and the `Enabled` state already satisfies the
ADR 0039 §5 render gate. Add and enable keep mirrored kind guards (an id
holds one slot kind per record digest: a spec-only manifest may never sit
over a host slot, a component manifest never over a hostless slot), and
the panels state lookup consults component hosts first, hostless
lifecycles second, `Absent` otherwise.

Evidence: `modules.rs` `ensure_entity` + `ModuleEntity` +
`module_ui_state`, session/stdio threading of the hostless lifecycle map;
eight new interactive tests —
`spec_only_module_enables_hostlessly_and_publishes_declared_specs`
(publish through `module_ui_specs`),
`spec_only_enable_follows_the_profile_selection_gate`,
`spec_only_enable_rechecks_persisted_identity_before_enabling`,
`changed_spec_only_manifest_conflicts_against_enabled_lifecycle`,
`activate_refuses_spec_only_module_with_typed_message` (including the
ordinary gate for a not-yet-enabled id),
`render_add_module_refuses_component_manifest_over_hostless_slot`,
`spec_only_enable_reports_already_enabled_state`,
`stdio_flow_enables_a_spec_only_module_end_to_end`. `cargo test
--workspace` green (core 682, adapters 534, differential 28, cli 431),
clippy clean, `npm run check` exit 0. The differential corpus needed no
amendment: it pins the panel surface at the `ModuleUiSource` level,
which this change does not move. The owner runbook in
`modules/deepseek-hours/README.md` records that the `/modules-activate`
refusal is correct for this module.
