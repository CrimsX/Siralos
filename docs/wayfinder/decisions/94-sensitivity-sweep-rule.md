---
title: "The Sensitivity-Sweep Rule"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# 94 — The Sensitivity-Sweep Rule

Governing plan 68 · entry review [79](79-context-management-foundations.md) · Map.

> **User-directed 2026-08-31 (session HITL).** After the decision 92 re-measurement flipped the benchmark GO on a single extreme estimator cell (bpt=5/oh=16, 8/9 cells passing), the externally-signed ruling is NO REVISION: the 9-cell sweep stays gated exactly as pre-committed in decisions 86/87/88, the primary-cell-only rule is rejected, and no estimator-range change is admissible except on independent real-tokenizer evidence — never on verdict grounds.

## 2. The signed rule (externally signed, verbatim)

**Rule (iii) — signed, verbatim:**

> "(iii) GO still requires, unchanged: depth-aware recall parity (V2 14/14) AND aggregate paged*2 < DeepAll AND 9/9 sensitivity cells AND dedup guard; the bpt{3,4,5}x{0,8,16} sweep stays gated, the primary-cell-only rule is rejected, and no estimator-range revision is admissible unless justified on independent tokenizer grounds (e1 evidence), never on verdict grounds."

**Rationale (signed, verbatim):**

> "the sweep is the anti-overfitting guard against estimator-dependent efficiency claims, 92 proved its value by firing on a real cost regression, and relaxing it upon failure destroys the gate's information content."

**Honesty caveat (verbatim):**

> "decision 92's numbers are already seen, so no (iii) touching the benchmark bar can be a true pre-commit anymore — which is precisely why (iii) is 'no revision' rather than a tuned bar."

**e1 amendment (verbatim):**

> "the real-tokenizer calibration (e1) may be measured at any time as an informational exercise, but its numbers may not enter any gate argument until this rule is signed — with this record, e1 becomes the prerequisite evidence for any future estimator-range argument."

## 3. Criteria → evidence

The rule is a commitment record, not a measurement; it pins the externally-signed NO REVISION ruling so no future gate revision can be laundered as a pre-commit.

| Criterion                                                             | Evidence                                                                                                                                                                                                                                                                                      | Status |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Decision 92 re-measured outcome it responds to                        | Re-measured under unchanged decision 87/88 rules: recall 14/14 parity held, paged V2 3170 vs DeepAll 7609 (58.3% reduction, paged\*2 < DeepAll true), dedup share 0%, 9-cell bpt{3,4,5}×oh{0,8,16} sweep 8/9 pass (bpt=5/oh=16 fails margin) → GO false; seam re-gated per decision 92 record | pass   |
| Sign-off provenance (externally signed)                               | External challenger, two-round discussion 2026-08-31 (session HITL); ruling signed before this record; this file is the pin — no benchmark re-run, no threshold tuning, no strategy change                                                                                                    | pass   |
| Decisions 86/87/88/92 records untouched (append-only)                 | `docs/wayfinder/decisions/86-v3-heterogeneous-regate.md`, `87-corrected-baseline-regate.md`, `88-escalation-policy-test.md`, `92-search-scoring-rerank.md` byte-identical to pre-94 state; this decision adds a new file only — no amendment to prior records                                 | pass   |
| No estimator-range change without independent tokenizer evidence (e1) | e1 real-tokenizer calibration admitted as informational evidence only, prerequisite for any future estimator-range argument per the signed e1 amendment; no range revision is admissible on verdict grounds alone                                                                             | pass   |
| Primary-cell-only rule rejected, 9/9 sweep remains gated              | Signed rule (iii) explicitly rejects the primary-cell-only relaxation; 9/9 sensitivity cells remain gated for every future benchmark run per decisions 86/87/88                                                                                                                               | pass   |

## 4. Result

The sensitivity-sweep rule stands unamended: 9/9 cells remain gated for every future benchmark run; the primary-cell-only relaxation is rejected; the real-tokenizer calibration is admitted as informational evidence only, prerequisite for any future estimator-range argument; decisions 86-93 stand untouched.
