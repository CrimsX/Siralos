# Stability contract — the 1.x line

Status: the promises this repository makes about its 1.x line, and the ones it
deliberately does not. Every claim below names the document or the check that
owns it. A claim with no owner does not belong here, and milestone status is not
restated — it lives in [ROADMAP.md](../../ROADMAP.md).

## Frozen for the 1.x line

| Frozen surface                | What is frozen                                                                                                                                                                                                                                                                                                                                            | Owned and enforced by                                                                                                    |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| The model-facing tool surface | Exactly three tools are registered unconditionally — `workspace.list`, `workspace.read`, `workspace.search` — and all three are read-only. Further read-only context tools join them only when the context subsystem is opted in.                                                                                                                         | [ARCHITECTURE.md](../../ARCHITECTURE.md), the tool-loop differential subject                                             |
| Composition                   | A Profile is declarative, versioned, and **narrowing-only**: it may restrict what the host permits and may never widen it. An unknown field, a malformed capability id, a rule that is not `allow`/`ask`/`deny`, or a malformed credential reference leaves the profile unapplied rather than half-applied.                                               | [ADR 0036](../adr/0036-lean-product-composition-and-extension-model.md), `crates/siralos-adapters/src/profile_config.rs` |
| The fail-closed taxonomy      | Effects that cannot be enforced report a typed `unavailable` **before** approval, checkpoint creation, or spawn. The set of closed surfaces is enumerated in [ARCHITECTURE.md](../../ARCHITECTURE.md) and [SECURITY.md](../../SECURITY.md).                                                                                                               | The differential corpus, which pins the outcome for those subjects                                                       |
| How a closed surface reopens  | Reopening one is a **reviewed oracle amendment**, never a code change alone: the frozen reference records and the corpus scenarios that pin the closed outcome must be amended deliberately and in public.                                                                                                                                                | [AGENTS.md](../../AGENTS.md), [ADR 0033](../adr/0033-differential-behavioral-harness.md)                                 |
| Verification                  | Behaviour claims are checked by the differential harness: a scenario corpus compared against a pinned reference set of frozen oracle records plus candidate-authored expectation records, with typed canonical outcomes. Reference records may be retired only through the digest-bound supersession list, and the audit prints every retirement in full. | [ADR 0033](../adr/0033-differential-behavioral-harness.md), `npm run check:differential`, `npm run check:supersessions`  |
| Version identity              | The manifest version, the release tag, and `siralos --version` are one number; the release path refuses a tag that disagrees with it.                                                                                                                                                                                                                     | [CHANGELOG.md](../../CHANGELOG.md), the release workflow                                                                 |
| Provider neutrality           | The core is provider-neutral and the host owns the path from proposal to evidence. Provider integrations are user-directed and their network behaviour belongs to the profile that configures them.                                                                                                                                                       | [ARCHITECTURE.md](../../ARCHITECTURE.md), `npm run check:rust`                                                           |

## Explicitly not promised by 1.x

- **Availability of any fail-closed surface.** Mutation, checkpoints, command
  execution, Git inspection, and engine or language-server probes stay
  `unavailable` until their security property is mechanically enforceable and
  covered by adversarial tests. No date is promised, and no version number
  obliges it.
- **A desktop UI.** 1.x ships a terminal frontend and a headless mode. Views and
  a desktop surface are direction, not a commitment.
- **Reproducible or byte-identical binaries.** "Same dependencies" and
  "byte-identical output" are different guarantees; the second is not claimed
  until it is demonstrated on independent clean environments.
- **An SBOM or a provenance attestation.** The identity chain a release must
  eventually bind is designed in
  [release-provenance.md](release-provenance.md); the tooling is not introduced
  yet, and nothing here should be read as a supply-chain claim.
- **Performance or platform-compatibility numbers** beyond what the repository
  gate and CI actually prove. A skipped probe is never a pass.
- **That the release workflow works.** It has never executed. Its status is the
  same `unknown` recorded in [ROADMAP.md](../../ROADMAP.md), and no local dry run
  may be reported as a real one.
- **Stability of internals.** The harness crate, the corpus fixture packages
  (`fuzz/`, `experiments/domain-abi/**`, the conformance guests), and the CI
  workflow surface may change in a minor release. They are versioned
  independently of the product on purpose.
- **Semantic stability of model output.** The harness governs authority, not
  prose. A given prompt may produce a different answer on a different model or a
  different day.

## How a promise here changes

- Changing a frozen surface is an **ADR amendment** with its own review, not a
  patch. If the change alters observable behaviour, the differential corpus moves
  with it, deliberately and in the same change.
- Adding a promise means naming the check that enforces it. A promise without an
  enforcing check is deleted from this document rather than softened.
- What is frozen above binds the 1.x line. Work that requires breaking it is
  2.0 work, and is listed as such in [ROADMAP.md](../../ROADMAP.md).
