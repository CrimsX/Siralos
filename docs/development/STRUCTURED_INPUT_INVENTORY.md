# Siralos structured-input inventory

Status: authoritative (pre-Stage-4 assurance, contract Part 3).

Every meaningful structured-input boundary in Siralos, its trust
classification, and its fuzz/security priority.

The Rust implementation is the product's boundary surface: the TypeScript
tree was removed by decision 40, and the paths in the tables below are
reconciled against the Rust workspace. A row whose path starts with
`packages/` or `apps/` is a HISTORICAL reference — the boundary it
describes still exists, but it now lives in `crates/siralos-core`,
`crates/siralos-adapters` or `crates/siralos-cli`. Classifications:

- `trusted internal` — produced and consumed by host-owned code; no
  hostile input expected, still validated at parse boundaries.
- `host-generated` — produced by the host (files, checkpoints, evidence,
  run metadata); corruption indicates disk fault or tampering.
- `workspace-controlled` — content from the user's project workspace;
  untrusted but user-attested.
- `provider/model-controlled` — output of an external model/provider;
  always untrusted.
- `network-controlled` — fetched from the network; always untrusted.
- `domain-controlled` — content from an installed optional domain
  package; untrusted until package identity is verified.
- `user-controlled` — interactive or CLI input; untrusted.

Priority: `HIGH` = malformed input could affect authority, filesystem
effects, process execution, revision/approval binding, state
transitions, artifact identity, capability projection, or mutation;
`MEDIUM` = could affect determinism or diagnostics; `LOW` = cosmetic.

## TypeScript reference (TS-era archived surface)

| Boundary                                 | Location                                                                    | Classification            | Priority | Status                                                                                                                                                                       |
| ---------------------------------------- | --------------------------------------------------------------------------- | ------------------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| User configuration                       | `packages/adapters/src/config/user-config.ts`                               | user-controlled           | HIGH     | TS-era archived; Rust parse: `crates/siralos-adapters/src/config.rs` (`parse_user_config`)                                                                                   |
| Provider configuration                   | `packages/core/src/security/profile.ts`                                     | host-generated            | HIGH     | TS-era archived; no Rust parse (built-in profiles are constructed in `crates/siralos-core/src/security.rs`)                                                                  |
| TaskContract decoding                    | `packages/core/src/tasks/task-model.ts`                                     | host-generated            | HIGH     | TS-era archived; Rust validated by construction: `crates/siralos-core/src/task/contract.rs`                                                                                  |
| TaskPlan decoding                        | `packages/core/src/tasks/task-runtime-planning.ts`                          | host-generated            | HIGH     | TS-era archived; Rust model + validation: `crates/siralos-core/src/planning/model.rs`, `crates/siralos-core/src/planning/validation.rs`                                      |
| Execution contract / milestone manifests | `packages/core/src/executor/execution-contract.ts`, `milestone-manifest.ts` | host-generated            | HIGH     | TS-era archived; Rust types: `crates/siralos-core/src/executor/contracts.rs`, `crates/siralos-core/src/executor/milestone.rs`                                                |
| Workspace revision metadata              | `packages/core/src/workspace/workspace-revision.ts`                         | workspace-controlled      | HIGH     | TS-era archived; Rust revision handles: `crates/siralos-core/src/workspace/revision.rs`                                                                                      |
| Checkpoint metadata / preimages          | `packages/adapters/src/checkpoints/filesystem/checkpoint-file-state.ts`     | host-generated            | HIGH     | TS-era archived; Rust inspection: `crates/siralos-adapters/src/workspace/checkpoint.rs` (creation fails closed)                                                              |
| Evidence artifacts                       | `packages/core/src/tasks/task-runtime-evidence.ts`                          | host-generated            | MEDIUM   | TS-era archived; Rust validation: `crates/siralos-core/src/task/evidence.rs`                                                                                                 |
| Research artifacts                       | `packages/adapters/src/research/*`                                          | network-controlled        | HIGH     | TS-era archived; Rust bounded normalization, denied by default: `crates/siralos-core/src/research.rs`, `crates/siralos-adapters/src/research.rs`                             |
| Reference materialization                | `packages/adapters/src/reference/reference-materializer.ts`                 | workspace-controlled      | MEDIUM   | unavailable (fail-closed); `crates/siralos-adapters/src/reference.rs`                                                                                                        |
| Scene/resource parsing                   | `packages/adapters/src/godot/intelligence/*`                                | workspace-controlled      | HIGH     | TS-era archived; no Rust parser (scene/GDScript intelligence is external `siralos-godot`; reads report typed unsupported in `crates/siralos-adapters/src/workspace/read.rs`) |
| `project.godot` profiling                | `packages/adapters/src/godot/project/*`                                     | workspace-controlled      | HIGH     | TS-era archived; no Rust parser (project profiling is external `siralos-godot` domain)                                                                                       |
| Provider/tool messages                   | `packages/adapters/src/providers/*`                                         | provider/model-controlled | HIGH     | TS-era archived; Rust bounded protocol checks: `crates/siralos-adapters/src/provider/**`                                                                                     |
| Model responses                          | `packages/core/src/application/*`                                           | provider/model-controlled | HIGH     | TS-era archived; Rust terminal sanitizer: `crates/siralos-cli/src/sanitize.rs`                                                                                               |
| GDScript diagnostics output              | `packages/adapters/src/godot/diagnostics/*`                                 | provider/model-controlled | MEDIUM   | unavailable (fail-closed); GDScript check-only diagnostics never run (scanner is external `siralos-godot` domain)                                                            |
| Git status/diff output                   | `packages/adapters/src/git/cli/*`                                           | workspace-controlled      | MEDIUM   | unavailable (fail-closed); `crates/siralos-adapters/src/workspace/git.rs`                                                                                                    |
| CLI structured inputs                    | `apps/cli/src/input/*`                                                      | user-controlled           | MEDIUM   | TS-era archived; Rust parse: `crates/siralos-cli/src/lib.rs` (`parse_args`)                                                                                                  |

## Rust implementation (current surface)

| Boundary                         | Location                               | Classification       | Priority | Status                                                    |
| -------------------------------- | -------------------------------------- | -------------------- | -------- | --------------------------------------------------------- |
| Version string parsing           | `crates/siralos-core/src/version.rs`   | user/host-controlled | MEDIUM   | typed parse, bounded components; fuzz + property coverage |
| CLI argument parsing             | `crates/siralos-cli/src/lib.rs`        | user-controlled      | MEDIUM   | typed errors; fuzz coverage                               |
| Differential corpus JSON         | `harness/src/harness/loader.rs`        | host-generated       | MEDIUM   | serde deserialization; fuzz coverage                      |
| Cargo.toml version extraction    | `harness/src/harness/dispatch.rs`      | host-generated       | LOW      | toml crate parse                                          |
| State-dir environment resolution | `crates/siralos-adapters/src/paths.rs` | user-controlled      | MEDIUM   | OsString semantics, no UTF-8 assumption                   |

## Fuzz/property priority

Fuzz targets exist for: version parsing, CLI arguments, and differential
corpus decoding (see `fuzz/`, nightly toolchain); the corpus target is
stale against the extracted harness and is the open owner decision
recorded in ROADMAP.md. Property tests exist for version
round-trip/ordering (proptest) and canonical JSON idempotence
(deterministic generator). The remaining HIGH-priority boundaries
(provider messages, model responses, scene/resource parsing) are
fail-closed `unavailable` or live in the external `siralos-godot`
repository, so they carry no fuzz surface in this workspace today.
