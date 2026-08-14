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

Environment: Windows 11, x86_64-pc-windows-gnu host (MinGW-w64),
criterion 0.5, `cargo bench -p siralos-core`.

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
per fixture. No optimizations were applied without measurement; none
were needed for the current surface.

## Future workloads

Stage 1–3 operations that will gain benchmarks when their subsystems
are ported (R3+): repository discovery, file search, structural read,
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
