# Decision — R13.1 Entry Review — Host Introspection & Authority Scenarios

**Wayfinder ticket:** [R13.1 Entry Review](../tickets/23-r13-1-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Remaining-Surface Ports Entry Review](21-r13-remaining-surface-parity-entry-review.md) (PASS — R13 contract frozen)
**Decided:** 2026-08-24 (resolver session, interactive HITL grilling over reads of `packages/core/src/security/*`, `commands/*`, `doctor/*`, `self/*`, and the existing `siralos_core::tool` overlap check)
**Status:** **PASS — R13.1 scenario contract frozen; implementation authorized**
**Self-loop ledger:** 3 criteria, one implementation pass (verification below)

> Mirrors [R11 Entry Review](18-r11-entry-review.md): behavior, subjects,
> and acceptance are frozen here; no implementation lands in this record.

---

## Summary

R13.1 ports the authority/introspection backbone to the Rust candidate
and proves it differentially: permission evaluation with profile
constraints over the built-in default policies, one-time digest-bound
approval binding, protected behavioral-configuration classification, the
command catalog/registry surface, and deterministic doctor/self-reference
reporting. The existing `siralos_core::tool::permission` layer is the
generic decision seam from R7.2 and is **not** duplicated; this slice
ports the concrete policy object graph around it.

## 1. Frozen scenario set (~20 fixtures, corpus v24)

### `security-permissions` ×10

| #   | Case                          | What it proves                                                                                            |
| --- | ----------------------------- | --------------------------------------------------------------------------------------------------------- |
| 1   | allow-rule-without-constraint | `workspace.read` under a permitting profile evaluates `allow` with no reason payload                      |
| 2   | missing-rule-fails-closed     | unknown capability → `deny` with the exact fail-closed reason                                             |
| 3   | explicit-deny                 | `research.fetch` denied in every built-in profile                                                         |
| 4   | ask-rule                      | one-time-approval capabilities (`godot.diagnose`) evaluate `ask`                                          |
| 5   | process-profile-constraint    | `process.execute` denied when the profile does not enable process execution                               |
| 6   | workspace-write-constraint    | `workspace.write` differs by profile filesystem access (read-only deny vs ask)                            |
| 7   | network-universal-deny        | `network.outbound` never permitted by any built-in profile                                                |
| 8   | default-policy-table-snapshot | full rule map per built-in profile (`inspect`, `develop-offline`, `validation-offline`, …) is byte-stable |
| 9   | approval-binding-one-time     | digest-bound approval accepts exactly its prepared content; mismatch and reuse fail typed                 |
| 10  | behavioral-config-protected   | `AGENTS.md` at depth and `.siralos/**` classify as protected regardless of casing/depth                   |

### `command-catalog` ×5

| #   | Case                           | What it proves                                                            |
| --- | ------------------------------ | ------------------------------------------------------------------------- |
| 1   | catalog-snapshot-deterministic | ordered entries with classifications render identically from the registry |
| 2   | unknown-command-refusal        | unknown slash command fails typed without ending anything                 |
| 3   | registration-order-detachment  | definitions preserve registration order and detach from caller data       |
| 4   | command-digest-stability       | command identity digest is content-bound and stable                       |
| 5   | runner-availability-truthful   | node-script/npm-script runners report unavailable truthfully              |

### `capability-doctor` ×5

| #   | Case                           | What it proves                                                          |
| --- | ------------------------------ | ----------------------------------------------------------------------- |
| 1   | report-over-injected-state     | doctor report over harness-injected fake runtime state is deterministic |
| 2   | safe-report-redaction          | report-safe output carries no secrets or absolute paths                 |
| 3   | unknown-area-refusal           | filtering to an unknown area refuses typed, session intact              |
| 4   | self-reference-bounded-offline | installed-runtime document generation is offline, bounded, and stable   |
| 5   | config-schema-summary-stable   | config schema summary rendering is deterministic                        |

## 2. Mechanics

- Probe layout follows the established pattern: new oracle probes execute
  the **real TypeScript reference** functions with bounded stdin JSON and
  emit canonical outcome records; the Rust side adds the corresponding
  modules behind the same subject names. Placement obeys the dependency
  direction (`cli → adapters → core`; core imports no adapters).
- Corpus bumps **v24** at the R13.1 reconciliation commit; schema stays 3.
- Doctor scenarios source runtime state exclusively from
  **harness-injected fakes** (HITL decision 2026-08-24) — never live host
  introspection — keeping records byte-stable cross-platform.
- Acceptance mirrors every prior slice: all applicable required scenarios
  match byte-for-byte on both implementations, plus the focused Rust unit
  tests; the full local gate passes on the reconciliation tree.

## 3. Boundaries — not in R13.1

- No unavailable effect becomes operational; approval binding never
  grants capability by itself (it gates exactly one prepared operation).
- Behavioral-config classification stays classification-only; no new
  mutation path exists to touch protected files.
- No live sandbox/Godot/network probing; research fetch remains denied.
- CLI wiring of these services belongs to R13.5, not here.

## 4. Authorization

Implementation of R13.1 is authorized against this frozen set. Landing
commit(s), differential results, and gate evidence are recorded in the
[R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                        | Direct evidence                                                                                                                                                       | Status |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Scenarios exercise reference-observable behavior | §1 cases cite concrete functions (`evaluatePermission`, `createDefaultPolicy` tables, catalog/registry, doctor model) verified by reading the TS sources this session | pass   |
| Overlap resolved, no double port                 | `siralos_core::tool::permission` owns only the generic decision layer; §Summary scopes this slice to the concrete policy graph around it                              | pass   |
| Human decided the material cuts                  | HITL answers 2026-08-24: 20-scenario set approved as proposed; approval + behavioral-config included; injected fakes confirmed                                        | pass   |
