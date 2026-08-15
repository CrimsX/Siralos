# Siralos protocol and schema versioning policy

Status: authoritative (pre-Stage-4 assurance, contract Part 20 / Part 21).

## Rules

1. **Every persisted or externally exchanged schema has an intentional
   compatibility rule.** Schemas without one are ephemeral private
   structs and are not part of any compatibility surface.
2. **Unknown versions fail explicitly.** An unknown future schema
   version is never silently interpreted as the current version.
   Implemented: the differential corpus manifest declares
   `schemaVersion`; a value other than 1 fails with a typed
   `HarnessError` (exit 2), on both the oracle and the candidate side.
3. **Artifact identity includes schema/domain separation where
   required.** Digests bind the exact serialized bytes of the schema
   version they were computed over (ADR 0028 discipline); corpus
   scenario digests are recomputed over the canonical serialization at
   validation time.
4. **Change classification** for any schema change:

   - `backward compatible` — old readers accept new data (additive
     optional fields only);
   - `forward compatible` — new readers accept old data (unknown fields
     ignored by policy);
   - `migration required` — a deterministic, tested conversion exists
     and runs before use;
   - `hard incompatible` — new schema version; old data is rejected
     explicitly, never reinterpreted.

   The differential corpus follows `hard incompatible` on any
   structural change: bump `corpusVersion` and `schemaVersion` as
   appropriate; unknown versions fail closed on both runners.

5. **Do not introduce compatibility machinery for purely ephemeral
   private structs.** The harness outcome records are a contract
   between the oracle and candidate runners and are versioned with the
   corpus; internal Rust/TS object layouts are never a compatibility
   surface.

## Versioned schemas today

| Schema                                         | Location                                     | Version                                | Rule                                                             |
| ---------------------------------------------- | -------------------------------------------- | -------------------------------------- | ---------------------------------------------------------------- |
| Differential corpus manifest                   | `tests/differential/corpus/manifest.json`    | `schemaVersion: 1`, `corpusVersion: 1` | hard incompatible                                                |
| Differential outcome records                   | emitted by both runners                      | tied to corpus schema                  | hard incompatible                                                |
| User configuration schema                      | `schemas/user-config.schema.json`            | draft 2020-12 `$id`                    | backward compatible (additive)                                   |
| Domain ABI world (R6, ADR 0034)                | `crates/siralos-adapters/wit/domain-abi.wit` | `siralos:domain-abi@1.0.0`             | hard incompatible (exact equality; unknown versions fail closed) |
| Cargo manifest / package.json version identity | differential `version.identity` scenario     | product version                        | parity-gated                                                     |

## Host/domain contracts

The domain boundary's protocol versioning (capability model,
package identity, request/response schemas) is defined by the domain ABI
ADR (ADR 0034) and follows the rules above. The production world is
versioned (`siralos:domain-abi@1.0.0`); the lifecycle checks the ABI
identity exactly, and the host additionally verifies the versioned
export name in the component bytes before instantiation, so unknown or
incompatible domain protocol versions fail closed — never silently
deserialized, downgraded, or reinterpreted.

## Public Rust API compatibility (contract Part 21)

The workspace crates are an internal modular monolith; no crate exposes
a genuine external SemVer surface today. All `pub` items are
deliberate exports for workspace-internal composition, kept minimal
per the style guide. Consequently:

- no `cargo-semver-checks` baseline exists yet — it is evaluated when
  the first genuinely external library/API surface is tagged;
- visibility reductions are encouraged whenever an item need not be
  public; private refactoring is never blocked by fake compatibility
  promises.
