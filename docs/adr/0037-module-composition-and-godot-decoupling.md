---
id: ADR-0037
status: accepted
domains: [architecture, product, configuration, extensions]
paths:
  - README.md
  - ROADMAP.md
  - ARCHITECTURE.md
  - docs/development/PROJECT_CONTEXT.md
  - docs/development/PROTOCOL_VERSIONING.md
  - schemas/user-config.schema.json
  - crates/siralos-adapters/wit/module-abi.wit
supersedes: [ADR-0036]
---

# ADR 0037 — Module Composition Model and Godot Decoupling Boundary

- Status: accepted (owner directive 2026-10-10)
- Date: 2026-10-10
- Related: ADR 0036 (lean product, composition, extension model —
  superseded on domain/plugin vocabulary only), ADR 0034 (Godot domain
  host boundary), ADR 0033 (differential harness), ADR 0032 (Rust
  migration), decisions 38/39 (plugin UI slices)

This ADR supersedes ADR 0036's **domain/plugin vocabulary only**: the
two extension nouns (Plugin as package type, Domain as contributed
semantic specialization) are reconsolidated into one noun, **Module**.
Every lean guardrail in ADR 0036 stands unchanged: declarative
composition only; no marketplace, no automatic acquisition, no hooks,
no task-graph machinery, no multi-agent foundation. Where this ADR
narrows an earlier future-facing statement without touching its
measured decision, the narrowing is stated explicitly.

## 1. Why unify

ADR 0036 §32 already recorded the unification target: one
capability-scoped package mechanism for ordinary executable plugins
and domain-contributing plugins, preferred over two parallel
ecosystems. The implementation grew the two halves separately —
`PluginManifest` / `install_plugin` / `record_plugin` in
`siralos-adapters::domain::manifest`, `DomainLifecycle` /
`DomainPackage` / `DomainHost` underneath, `/domains-*` commands on
top — and users now configure `[plugins.<id>]` tables to install what
the code internally calls domains. One noun removes the seam the user
never asked for: a **module** is a user-installed optional feature
package (manifest + optional component + lifecycle). Skills,
profiles, context, and references are untouched: they are separate
composition primitives with their own authority rules, not modules.

## 2. What a module is

```text
Module = manifest + optional component + lifecycle
       + declared capability requests + Host grant
```

- A module is explicitly installed, optional, versioned, digestable,
  capability-scoped, and contained by the Host boundary. Core works
  with zero modules installed.
- A module may contribute Tools, Views, and specialized development
  intelligence (what ADR 0036 called a Domain). No other contribution
  categories are frozen now.
- Installing, enabling, or selecting a module never implies
  authority. The permission/capability terminology split stands:
  users see Permissions; the Host enforces Capabilities.

## 3. Manifest format (successor of `PluginManifest`)

The folder-root manifest file is renamed to `module-manifest.toml`;
the code type `PluginManifest` becomes `ModuleManifest`. The TOML
shape and every validation rule are unchanged — same keys (`id`,
`digest`, `abi`, `capabilities`, `component`), same bounds, same
`ManifestInvalid` refusals, same unknown-key tolerance:

```toml
id = "<package id>"
digest = "sha256:<hex>"
abi = "<abi identity>"
capabilities = ["workspace-read"]
component = "<single relative file name>" # optional
```

Rationale for keeping the keys: they are generic capability-package
descriptors, covered by unit tests, with no corpus coverage to
migrate; renaming them would churn user files for no semantic gain.
The error type `PluginFailure` becomes `ModuleFailure` with identical
codes; the install-record type `PluginRecord` becomes `ModuleRecord`
with identical `{id, path, digest}` fields.

## 4. User-configuration surface

Users declare their module setup in the workspace `siralos.toml`:

```toml
[modules.<id>]
path = "<workspace-relative folder>"
digest = "sha256:<hex>"

[profile]
modules = ["<id>", ...]   # narrowing-only selection filter
```

This is a mechanical rename of `[plugins.<id>]` and
`[profile] plugins`, with identical semantics: install records are
merged with conflict refusal (`RECORD_CONFLICT`), unrelated sections
are preserved byte-for-byte, and the profile filter intersects the
Host-enabled set so a profile can never enable, install, or activate
anything (`select_profile_plugins`, `decide_plugin_activation`
renamed with bodies unchanged).

Two deliberate non-changes, both pinned by evidence:

- The user-config schema (`schemas/user-config.schema.json`) gains
  no module keys and loses no godot keys. Its `godot` section is
  load-bearing for the `user-config-matrix` corpus record, which has
  already consumed its one supersession and cannot be superseded
  again (§9). The schema file documents install records, not live
  package state.
- `PROTOCOL_VERSIONING.md` keeps its contract table; only the
  row/section names that describe the renamed file move with it.

## 5. Lifecycle verbs (authority semantics preserved exactly)

The lifecycle is renamed, not redefined: `DomainLifecycle` becomes
`ModuleLifecycle` with states absent/installed/enabled/active;
`DomainHost` becomes `ModuleHost`; `DomainPackage` /
`DomainPackageId` / `PackageDigest` become `ModulePackage` /
`ModulePackageId` / `ModuleDigest` with identical parsing and
verification. The CLI vocabulary becomes `/modules`,
`/modules-add`, `/modules-enable`, `/modules-activate`, and the
Add-Plugin UI becomes Add-Module. These non-negotiables survive
verbatim (decision 39 and the fail-closed posture):

- install != enable != activate; enable is memory-only until
  Activate; typed stay-installed failures;
- one-time digest-bound approval, capability policy, sandbox
  enforcement, checkpointing, and stale-state checks are independent
  gates — success at one never implies another;
- no `unavailable` capability flips; nothing in this ADR makes an
  effect available.

## 6. ABI identity: frozen wire strings, renamed code

The versioned identifiers inside the component boundary are a stability
contract, not vocabulary: the package string
`siralos:domain-abi@1.0.0`, the world `siralos-domain`, the interface
`domain-api`, and the export identity
`siralos:domain-abi/domain-api@1.0.0` are **frozen**. Twelve pinned
`domain-lifecycle` oracle records echo the ABI string; renaming it
would cost twelve supersessions and a conformance-guest rebuild for
zero authority or behavior gain, and `PROTOCOL_VERSIONING.md` exists
precisely to keep versioned identifiers stable. The code around them
is renamed (`DomainAbi` becomes `ModuleAbi`, still parsing the same
string; the WIT file moves to
`crates/siralos-adapters/wit/module-abi.wit` with byte-identical
content, so no fixture rebuild and no digest-constant change). The
synthetic conformance guests, their directory, the
`npm run build:domain-conformance` script name, and the retained
`experiments/domain-abi` prototypes keep their names: R6-era tooling
stability outranks aesthetic consistency, and renaming the script
would require touching every doc citation for no behavioral reason.

## 7. Godot decoupling plan — and its evidence boundary

The owner directive asks for full removal of `siralos-godot` from
this repository: dropping the `harness/Cargo.toml` path dependency,
the CI clone/provision steps, the pins, and the `godot-*` corpus
subjects with their harness runners. That removal is **blocked by the
pinned-evidence binding**, not by engineering effort. The mechanics,
verified against the gate sources:

- A supersession must name a scenario present in the current corpus
  AND in the pinned oracle (`run-differential.mjs`, unknown-scenario
  refusal); at most 64 entries; one per id, once only.
- A pinned id absent from the corpus with no supersession stops the
  run (`PINNED_MISMATCH`: "absent from the current corpus without a
  supersession"); a pinned record whose scenario left the corpus
  stops evidence loading (`EVIDENCE_UNKNOWN_RECORD`: "contains a
  scenario absent from the corpus").
- The frozen v32 bundle still carries 43 `godot-*` records. Deleting
  the 43 scenario files therefore fails the gate twice over, with no
  sanctioned repair: supersession cannot cover absent ids,
  re-freezing without the (removed) TypeScript oracle would replace
  the trust anchor with candidate-authored records, and editing the
  gate logic to permit deletion is a check-weakening this ADR
  refuses.
- Converting godot scenarios into non-godot scenarios under the same
  ids to dodge the above is record-laundering, not retirement, and
  is refused for the same reason.
- The `user-config-matrix` record, whose config echoes the `godot`
  section, has consumed its single supersession; the godot config
  keys therefore stay parsed and emitted byte-identically.

Consequences, recorded so no later session rediscovers them:

- The `godot-*` subjects keep executing; the harness keeps its
  `siralos-godot = { path = "../../siralos-godot" }` dependency;
  `rust.yml` / `assurance.yml` keep their clone/provision steps and
  pins; `fuzz/Cargo.lock` keeps its transitive entries.
- The AGENTS.md `1bf2ca3` vs CI `c8777b1…` pin mismatch is NOT
  resolved by this campaign: both pins survive with the dependency,
  and reconciliation stays owned where it was (W1.6/W2.x).
- Docs may call the external plugin deprecated only if the owner
  separately deprecates that repository; this repository keeps
  describing what it executes.

Owner options if removal is still wanted (proposals only — each
needs an explicit owner decision and its own adversarial guards
before any gate logic is touched):

- (A) A digest-bound retired-subjects list, mechanically checked
  like supersessions (reason, decision, retired oracle digests,
  printed in the audit), as a reviewed gate extension.
- (B) A new freeze bundle re-baselining trust — NOT recommended: it
  voids the TypeScript-oracle anchor pinned mode exists to protect.
- (C) Keep executing the godot subjects — the current state.

## 8. Corpus amendment plan (corpus v82 → v83, 357 files unchanged)

No subject retires and no subject renames: differential subject names
are historical identifiers frozen by the pinned oracle, not product
vocabulary. The amendment is two supersessions plus a version stamp:

| Scenario                                                              | Change                                                                                               | Mechanism                                                                                          |
| --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `command-catalog-snapshot`                                            | `/domains-*` entries → `/modules-*`; Add Plugin strings → Add Module                                 | supersession (id persists, outcome COMPLETED, record differs)                                      |
| `cli-session-set`                                                     | catalog ids, help text, render strings → modules vocabulary                                          | supersession (same conditions)                                                                     |
| `domain-lifecycle` × 19, `domain-capability` × 4                      | none — ABI string frozen (§6), subjects keep ids                                                     | no change                                                                                          |
| `composition-plugin-*` × 8 and all other expectation-covered subjects | re-pin only where a rendered string changes (e.g. evidence `plugins={n}` wording); subjects keep ids | `candidate2.json` re-pin pattern (run, extract `canonicalRecordDocument` records, update in place) |
| `user-config-matrix`, `user-config-symlink.posix`                     | none — godot keys frozen (§4/§7)                                                                     | no change                                                                                          |
| `godot-*` × 43                                                        | none — §7                                                                                            | no change                                                                                          |
| `tui-render` × 4                                                      | re-pin only if palette/help frames move                                                              | `candidate2.json` pattern                                                                          |

Mechanics in one unit (bump + re-pin + green gate land together):
`CORPUS_VERSION` 82 → 83 in `harness/src/harness.rs` and
`tests/differential/shared/contract.mjs`; `manifest.json`
`corpusVersion` 83 with regenerated digests
(`tests/differential/regenerate-corpus-manifest.mjs`);
`supersessions.json` `corpusVersion` 83 with both new entries stamped
`supersededIn: 83` and `entriesSha256` re-stamped; the strict-loader
count assert stays 357 with its version (corpus-mechanics rule);
`ALLOWED_SUBJECTS` unchanged. The audit must PRINT both superseded
records (never silent), and `npm run check:supersessions` must still
refuse all eight adversarial lists.

## 9. Lean guardrails (unchanged)

Declarative composition only. Explicit install only — no
marketplace, no automatic acquisition, no workspace-triggered
installation, no recommendations. No general hooks, no TaskGraph, no
workflow engines, no multi-agent machinery, no plugin dependency
graphs. Core works with zero modules. These are restated, not
renegotiated.

## Consequences

- Code, CLI, and user-facing docs speak modules; protocol
  identifiers, differential subject names, and the godot execution
  surface stay exactly as the evidence binds them.
- The next reviewer verifies: two supersessions printed in the
  audit, v83 manifest digests, 357-count assert with its version,
  `check:supersessions` green, full gate exit 0 — and no godot
  removal claimed anywhere.
