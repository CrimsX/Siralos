# Siralos structured-input inventory

Status: authoritative (pre-Stage-4 assurance, contract Part 3).

Every meaningful structured-input boundary in current Stage 1–3 Siralos,
its trust classification, and its fuzz/security priority. The TypeScript
reference remains the product's actual boundary surface; the Rust
candidate's boundaries are listed where they exist today. Classifications:

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

## TypeScript reference (current product surface)

| Boundary                                 | Location                                                                    | Classification            | Priority | Status                                               |
| ---------------------------------------- | --------------------------------------------------------------------------- | ------------------------- | -------- | ---------------------------------------------------- |
| User configuration                       | `packages/adapters/src/config/user-config.ts`                               | user-controlled           | HIGH     | TS validation + schema; Rust parse pending migration |
| Provider configuration                   | `packages/core/src/security/profile.ts`                                     | host-generated            | HIGH     | TS validation                                        |
| TaskContract decoding                    | `packages/core/src/tasks/task-model.ts`                                     | host-generated            | HIGH     | TS runtime validation                                |
| TaskPlan decoding                        | `packages/core/src/tasks/task-runtime-planning.ts`                          | host-generated            | HIGH     | TS runtime validation                                |
| Execution contract / milestone manifests | `packages/core/src/executor/execution-contract.ts`, `milestone-manifest.ts` | host-generated            | HIGH     | TS validation + manifests                            |
| Workspace revision metadata              | `packages/core/src/workspace/workspace-revision.ts`                         | workspace-controlled      | HIGH     | TS validation                                        |
| Checkpoint metadata / preimages          | `packages/adapters/src/checkpoints/filesystem/checkpoint-file-state.ts`     | host-generated            | HIGH     | TS verification, fail-closed                         |
| Evidence artifacts                       | `packages/core/src/tasks/task-runtime-evidence.ts`                          | host-generated            | MEDIUM   | TS validation                                        |
| Research artifacts                       | `packages/adapters/src/research/*`                                          | network-controlled        | HIGH     | TS bounded parsing, denied by default                |
| Reference materialization                | `packages/adapters/src/reference/reference-materializer.ts`                 | workspace-controlled      | MEDIUM   | unavailable (fail-closed)                            |
| Scene/resource parsing                   | `packages/adapters/src/godot/intelligence/*`                                | workspace-controlled      | HIGH     | TS tokenizer/parser, bounds                          |
| `project.godot` profiling                | `packages/adapters/src/godot/project/*`                                     | workspace-controlled      | HIGH     | TS static parsing                                    |
| Provider/tool messages                   | `packages/adapters/src/providers/*`                                         | provider/model-controlled | HIGH     | TS bounded protocol checks                           |
| Model responses                          | `packages/core/src/application/*`                                           | provider/model-controlled | HIGH     | TS sanitization at boundary                          |
| GDScript diagnostics output              | `packages/adapters/src/godot/diagnostics/*`                                 | provider/model-controlled | MEDIUM   | TS normalization                                     |
| Git status/diff output                   | `packages/adapters/src/git/cli/*`                                           | workspace-controlled      | MEDIUM   | unavailable (fail-closed)                            |
| CLI structured inputs                    | `apps/cli/src/input/*`                                                      | user-controlled           | MEDIUM   | TS parsing                                           |

## Rust candidate (current surface)

| Boundary                         | Location                               | Classification       | Priority | Status                                                    |
| -------------------------------- | -------------------------------------- | -------------------- | -------- | --------------------------------------------------------- |
| Version string parsing           | `crates/siralos-core/src/version.rs`   | user/host-controlled | MEDIUM   | typed parse, bounded components; fuzz + property coverage |
| CLI argument parsing             | `crates/siralos-cli/src/lib.rs`        | user-controlled      | MEDIUM   | typed errors; fuzz coverage                               |
| Differential corpus JSON         | `crates/siralos-cli/src/harness.rs`    | host-generated       | MEDIUM   | serde deserialization; fuzz coverage                      |
| Cargo.toml version extraction    | `crates/siralos-cli/src/harness.rs`    | host-generated       | LOW      | toml crate parse                                          |
| State-dir environment resolution | `crates/siralos-adapters/src/paths.rs` | user-controlled      | MEDIUM   | OsString semantics, no UTF-8 assumption                   |

## Fuzz/property priority

Fuzz targets exist for: version parsing, CLI arguments, and differential
corpus decoding (see `fuzz/`, nightly toolchain). Property tests exist
for version round-trip/ordering (proptest) and canonical JSON
idempotence (deterministic generator). Remaining HIGH-priority TS
boundaries (provider messages, model responses, scene/resource parsing)
receive fuzz/property coverage when the corresponding subsystems are
ported to Rust (R4+), at which point the corpus and fuzz targets grow
with them.
