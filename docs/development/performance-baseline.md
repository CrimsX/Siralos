# Siralos performance baseline

Status: authoritative (pre-Stage-4 assurance, contract Parts 13–14).
Workload identity, input size, environment, toolchain, commit, and
median are recorded per benchmark. Regression budgets are defined for
high-value operations. The scheduled assurance workflow records and
uploads Criterion's machine-readable estimates plus the exact commit and
toolchain; it does not claim an automatic regression verdict until a
stable comparable-run history exists.

## Benchmarks

| Workload                     | Input size                 | Median                | Toolchain                             | Commit                   |
| ---------------------------- | -------------------------- | --------------------- | ------------------------------------- | ------------------------ |
| `version/parse-canonical`    | "1.97.1" (7 bytes)         | ~23.1 ns              | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `version/parse-reject`       | "not-a-version" (13 bytes) | ~78.9 ns              | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `version/display-round-trip` | Version(1,97,1)            | recorded by criterion | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |

Environment: Windows 11, `rustc 1.97.1` host `x86_64-pc-windows-msvc`
(pinned by `rust-toolchain.toml`), criterion 0.7.0,
`cargo bench -p siralos-core`.

The host triple and criterion version were read from `rustc -vV` and
`Cargo.lock` at the R9 revision below. This line previously named the
MinGW-w64 host and criterion 0.5; neither is what the pinned toolchain
reports.

Command:

```text
cargo bench --workspace --locked
```

## Regression budgets

Defined for the high-value current-surface operations (version parsing
throughput): a sustained median regression above 2× the recorded
baseline in two comparable scheduled assurance runs is a review item.
This is a human review policy, not an automatically enforced threshold.
Budgets become enforceable gates only after a stable baseline history
and a statistically sound comparison mechanism exist across platforms.

The weekly `performance` assurance job executes the locked benchmark
suite and retains `target/criterion` as an artifact named for the source
commit. A missing or failed benchmark job is therefore visible; a green
job proves measurement completed, not that performance is unchanged.

## Performance review (contract Part 14)

Current Rust surface review: no repeated filesystem traversal, repeated
parsing, repeated hashing, repeated canonical serialization,
unnecessary process creation, lock contention, unbounded queues/caches,
or eager work was found in the Stage 1–3 Rust candidate; the harness
reuses a single built binary (`cargo run --quiet`) and never rebuilds
per fixture. No optimizations were applied without measurement; the first
measured one is the secret-redaction work recorded in "Stage 3R R9
secret-redaction optimization" below.

## Stage 3R R3 task-kernel baseline

| Workload                              | Input size          | Median   | Toolchain                             | Commit                   |
| ------------------------------------- | ------------------- | -------- | ------------------------------------- | ------------------------ |
| `task/contract-validation-and-digest` | contract + SHA-256  | ~3.90 µs | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `task/create`                         | 1 step, 1 criterion | ~1.55 µs | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `task/phase-transition`               | prepared -> working | ~5.08 µs | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `task/evidence-attach-acceptance`     | 1 evidence record   | ~6.17 µs | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |
| `task/findings-validation`            | 1 finding           | ~190 ns  | stable 1.97.1 (release/bench profile) | see `git rev-parse HEAD` |

Command: `cargo bench -p siralos-core --locked --bench task_baseline`.
No optimization was justified by these baselines; they exist for later
before/after comparison when the task kernel gains consumers.

## Stage 3R R9 secret-redaction optimization

`siralos_core::doctor::sanitize_secrets_only` is the single
secret-redaction owner: six ordered whole-string passes, one per credential
pattern, over every report-safe line the product renders.

Command: `cargo bench -p siralos-core --locked --bench redaction`.
Six input shapes (`prose`, `near-trigger`, `all-rules`, `case-variants`,
`non-ascii`, `secret-marker`) at six sizes (0 B, 30 B, 64 B, 1 KiB, 16 KiB,
64 KiB): 36 workloads. The harness asserts each shape's claim once per
workload before timing it -- `prose`, `near-trigger` and `secret-marker`
must come back unchanged, every other shape must contain `<secret>` -- and
30 B is the smallest size below both fixed-width minimums in the patterns
(32 and 40) at which every shape still carries a trigger.

Medians are criterion medians on Windows 11, `rustc 1.97.1` host
`x86_64-pc-windows-msvc`, release/bench profile. The baseline column was
measured at commit `b86dad4` with a clean tree (`doctor.rs` SHA-256
`E30A2981...`); the two optimized columns add the changes below,
uncommitted when recorded (`doctor.rs` SHA-256 `645E83F0...`). "After
candidate-first" is the second of two same-code runs, per the limits below.

| Workload                       | Input size | Baseline  | After byte scan | After candidate-first | Change |
| ------------------------------ | ---------- | --------- | --------------- | --------------------- | ------ |
| `redaction/prose-0b`           | 0 B        | 178.19 ns | 93.701 ns       | 84.679 ns             | -52.5% |
| `redaction/prose-30b`          | 30 B       | 4.7436 µs | 886.69 ns       | 627.48 ns             | -86.8% |
| `redaction/prose-64b`          | 64 B       | 9.3498 µs | 2.3008 µs       | 1.4791 µs             | -84.2% |
| `redaction/prose-1kb`          | 1 KiB      | 64.365 µs | 31.227 µs       | 18.334 µs             | -71.5% |
| `redaction/prose-16kb`         | 16 KiB     | 714.88 µs | 435.79 µs       | 260.77 µs             | -63.5% |
| `redaction/prose-64kb`         | 64 KiB     | 3.1037 ms | 1.6182 ms       | 1.1466 ms             | -63.1% |
| `redaction/near-trigger-0b`    | 0 B        | 179.20 ns | 101.51 ns       | 104.46 ns             | -41.7% |
| `redaction/near-trigger-30b`   | 30 B       | 5.0201 µs | 971.15 ns       | 972.02 ns             | -80.6% |
| `redaction/near-trigger-64b`   | 64 B       | 8.2039 µs | 2.3450 µs       | 2.1447 µs             | -73.9% |
| `redaction/near-trigger-1kb`   | 1 KiB      | 63.854 µs | 28.368 µs       | 18.908 µs             | -70.4% |
| `redaction/near-trigger-16kb`  | 16 KiB     | 750.99 µs | 360.90 µs       | 273.09 µs             | -63.6% |
| `redaction/near-trigger-64kb`  | 64 KiB     | 3.3083 ms | 1.4692 ms       | 1.0809 ms             | -67.3% |
| `redaction/all-rules-0b`       | 0 B        | 171.94 ns | 78.243 ns       | 79.016 ns             | -54.0% |
| `redaction/all-rules-30b`      | 30 B       | 4.6683 µs | 826.71 ns       | 641.20 ns             | -86.3% |
| `redaction/all-rules-64b`      | 64 B       | 5.5042 µs | 1.3231 µs       | 965.32 ns             | -82.5% |
| `redaction/all-rules-1kb`      | 1 KiB      | 54.349 µs | 18.211 µs       | 12.535 µs             | -76.9% |
| `redaction/all-rules-16kb`     | 16 KiB     | 711.44 µs | 267.05 µs       | 179.29 µs             | -74.8% |
| `redaction/all-rules-64kb`     | 64 KiB     | 2.4783 ms | 1.1949 ms       | 809.76 µs             | -67.3% |
| `redaction/case-variants-0b`   | 0 B        | 199.93 ns | 85.722 ns       | 83.874 ns             | -58.0% |
| `redaction/case-variants-30b`  | 30 B       | 4.6133 µs | 818.91 ns       | 638.55 ns             | -86.2% |
| `redaction/case-variants-64b`  | 64 B       | 6.5514 µs | 1.4795 µs       | 900.03 ns             | -86.3% |
| `redaction/case-variants-1kb`  | 1 KiB      | 40.695 µs | 21.922 µs       | 11.379 µs             | -72.0% |
| `redaction/case-variants-16kb` | 16 KiB     | 592.23 µs | 292.09 µs       | 233.17 µs             | -60.6% |
| `redaction/case-variants-64kb` | 64 KiB     | 2.2464 ms | 1.0841 ms       | 763.47 µs             | -66.0% |
| `redaction/non-ascii-0b`       | 0 B        | 179.39 ns | 76.870 ns       | 103.35 ns             | -42.4% |
| `redaction/non-ascii-30b`      | 30 B       | 4.5451 µs | 905.67 ns       | 756.84 ns             | -83.3% |
| `redaction/non-ascii-64b`      | 64 B       | 5.8340 µs | 1.8476 µs       | 1.6860 µs             | -71.1% |
| `redaction/non-ascii-1kb`      | 1 KiB      | 31.115 µs | 22.923 µs       | 17.717 µs             | -43.1% |
| `redaction/non-ascii-16kb`     | 16 KiB     | 474.20 µs | 315.52 µs       | 191.58 µs             | -59.6% |
| `redaction/non-ascii-64kb`     | 64 KiB     | 1.9672 ms | 1.3031 ms       | 871.24 µs             | -55.7% |
| `redaction/secret-marker-0b`   | 0 B        | 159.31 ns | 81.539 ns       | 83.879 ns             | -47.3% |
| `redaction/secret-marker-30b`  | 30 B       | 4.9282 µs | 898.46 ns       | 1.0962 µs             | -77.8% |
| `redaction/secret-marker-64b`  | 64 B       | 6.8714 µs | 1.9545 µs       | 1.6122 µs             | -76.5% |
| `redaction/secret-marker-1kb`  | 1 KiB      | 46.380 µs | 23.780 µs       | 15.708 µs             | -66.1% |
| `redaction/secret-marker-16kb` | 16 KiB     | 732.39 µs | 430.04 µs       | 269.10 µs             | -63.3% |
| `redaction/secret-marker-64kb` | 64 KiB     | 2.9514 ms | 1.7015 ms       | 1.0020 ms             | -66.1% |

### What changed

1. **Scan bytes, not a decoded character vector.** `replace_pass` collected
   `text.chars()` into a `Vec<char>` once per pass and re-encoded the
   untouched spans character by character. Every rule is an ASCII pattern,
   so the passes now scan `text.as_bytes()` and copy untouched spans as
   `&str` slices. `\b` is unchanged by this: a non-ASCII character is never
   a word byte and every byte of one is `>= 0x80`, so the byte before or
   after an index decides what the decoded character decided; and because a
   match always begins and ends on an ASCII byte, its offsets are character
   boundaries and the spans are safe to slice. Two guards fall out of the
   same change: a `[0-9a-fA-F]{32,}` run cannot match in fewer than 32 bytes
   of remaining input and a `[A-Za-z0-9+/]{40,}` run cannot match in fewer
   than 40, so both passes return before scanning a shorter tail.

2. **Test the candidate before `\b`.** The four literal-based rules tested
   `\b` at every index and only then the literal. They now test the literal
   first -- for `Bearer`, one case-insensitive byte, then the literal, then
   `\b` -- because the literal rejects almost every index on its first
   comparison. The operands are pure predicates, so only the amount of work
   changes, never a value.

### Measurement limits

- The machine is shared and unpinned. Two runs of the _same_ final code
  differed by at most 6% at 1 KiB, 10% at 16 KiB and 14% at 64 KiB, and by
  up to 47% at 0 B and 30 B and 40% at 64 B. No effect smaller than that is
  claimed here.
- The six 0 B rows call the owner on the same empty input. Their medians
  span 79-151 ns within a single run and move by up to 47% between runs, so
  they carry no signal about the code and are recorded only as measured.
- Step 2 is therefore claimed at 1 KiB and above, where criterion's
  adjacent-run comparison puts every workload at -20% to -59% with
  intervals excluding zero, and not at 30 B or 64 B, where its deltas sit
  inside that same-code drift.
- The two length guards are exact, but their effect is confined to inputs
  shorter than 40 B, where the drift above is larger than any effect they
  can have. They are kept as an exact-equivalence guard, not claimed as a
  measured win.

### Rejected candidates

- A literal prescan inside a finder (`if !tail.contains("sk-") { return
None }`). `replace_pass` calls a finder again after every match with the
  match's end as the new starting index, so prescanning the remaining tail
  costs O(n) per call and O(n*k) for a pass that matches k times --
  quadratic on secret-dense input such as `all-rules`. Rejected on that
  argument, before measurement. Testing the candidate at each index, as
  step 2 does, has no such term: the scan only ever moves forward.
- A case-sensitive `contains("Bearer")`. Wrong: the reference matches that
  literal case-insensitively, and the `bearer-upper`, `bearer-mixed` and
  `bearer-mixed-2` corpus rows exist to catch exactly that.

### Result

Every one of the 36 workloads got faster. Grouped by input size, the change
from baseline to the optimized tree is:

| Input size | Change across shapes           |
| ---------- | ------------------------------ |
| 0 B        | -42% to -58% (noise-dominated) |
| 30 B       | -78% to -87%                   |
| 64 B       | -71% to -86%                   |
| 1 KiB      | -43% to -77%                   |
| 16 KiB     | -60% to -75%                   |
| 64 KiB     | -56% to -67%                   |

Criterion's adjacent-run comparison attributes -18.5% to -83.6% to step 1
across all 36 workloads (p = 0.00, intervals excluding zero) and a further
-1.4% to -59% to step 2 across 33 of 36, the exceptions being 0 B rows.
Behaviour is unchanged: the 99-row executed-reference corpus passes as
written, and the bench's own shape assertions hold at every size.

## Stage 3R R5 language-normalization baseline and one measured fix

`normalize_diagnostic_set` owns diagnostic aggregation: exact duplicates
collapse on (path, line, column, code, message), the survivors sort in
JavaScript string order, and the run-wide bound applies with explicit
truncation. Its comparator, `utf16_cmp`, is also the ordering rule for symbol
sorting, so a sort in either module paid the same cost.

Command: `cargo bench -p siralos-core --locked --bench language_normalization`.
Two workloads: `normalize_diagnostic_set/10000` (10,000 diagnostics, the
`LANGUAGE_LIMITS.max_diagnostics_per_run` bound) and
`build_structural_summary/64-functions`, which does not reach the changed
function and is recorded as the control. This section is new: three of the four
benches in `crates/siralos-core/benches/` had numbers recorded here and this one
did not.

Medians are criterion medians on Windows 11, `rustc 1.97.1` host
`x86_64-pc-windows-msvc`, release/bench profile. The baseline column was
measured on a clean tree at commit `b1da8a3`; the after column adds the change
below, uncommitted when recorded.

| Workload                                | Input size   | Baseline  | After     | Change |
| --------------------------------------- | ------------ | --------- | --------- | ------ |
| `normalize_diagnostic_set/10000`        | 10,000 diags | 68.428 ms | 24.477 ms | -64.2% |
| `build_structural_summary/64-functions` | 64 functions | 4.6925 µs | 4.8571 µs | +3.5%  |

### What changed

`utf16_cmp` built a `Vec<u16>` for each side on every call, through a private
`utf16_units` helper, so the comparator inside `sort_by` allocated twice per
comparison — roughly 133,000 comparisons at n = 10,000, each one decoding and
allocating rather than comparing.

The vectors were never needed. `left.encode_utf16().cmp(right.encode_utf16())`
compares the same code units in the same order, and `Iterator::cmp` returns
`Less` for a proper prefix, which is what the trailing length comparison did.
The one subtle case is preserved rather than reasoned away: an astral scalar's
lead surrogate sorts below a BMP scalar above U+E000, which `language::tests`
pins against `utf16_cmp` and which still passes. `utf16_units` is deleted with
its only caller, and the ordering rule itself is unchanged — the differential
corpus asserts the orders this function feeds.

### Measurement limits

- The machine is shared and unpinned, and each column is a single run. The
  claimed row is claimed because criterion's intervals do not overlap:
  66.942-70.149 ms against 24.070-24.980 ms.
- The control row moved +3.5% with overlapping intervals (4.5824-4.8110 µs
  against 4.7028-5.0422 µs) and does not reach the changed function. That is the
  drift this run can produce, and no effect smaller than the claimed one is
  asserted.

### Not claimed

- The workload still costs ~24 ms, and the sort still re-decodes each compared
  string at every comparison. A decorate-sort-undecorate pass would encode each
  key once rather than once per comparison, but it restructures a
  parity-critical comparator to save time on a bounded path, so it was not
  taken.
- `projection/segments.rs::js_string_cmp` carries the identical defect — two
  `Vec<u16>` allocations per comparison — and the identical rule already exists
  in `language/diagnostic.rs`, which is one rule written twice. It is left
  alone because it was not measured: its only caller sorts context segments, a
  smaller domain than 10,000 diagnostics, and a change there belongs with the
  same measure-first discipline this section follows.

## Future workloads

Stage 1–3 operations that will gain benchmarks when their subsystems
are ported (R4+): repository discovery, file search, structural read,
revision hashing, prepared mutation generation, TaskState transitions,
ContextProjector/ToolProjector/EvidenceProjector, planning policy,
knowledge selection, and scene/resource parsing.

The lean vision (ADR 0036) adds a small future benchmark set sufficient
for before/after analysis:

- Profile resolution (and `siralos.lock` -> ResolvedProfile)
- digest/canonical identity computation
- semantic delta calculation
- Context compilation
- Context recompilation after one changed source
- Tool-surface construction

Later milestones may add Plugin cold/warm load, repository
indexing/search, and Godot parsing. Benchmarks are evidence tools, not
necessarily hard PR gates.
