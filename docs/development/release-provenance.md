# Siralos release provenance and artifact identity design

Status: design (pre-Stage-4 assurance, contract Parts 22–24). No
release system is implemented; this records the identity chain a future
official release must be able to bind, and where each identity comes
from and what verifies it.

## Release identity chain

A future official release must be able to bind:

| Identity                | Producer                                                                            | Verifier                                     |
| ----------------------- | ----------------------------------------------------------------------------------- | -------------------------------------------- |
| Source commit           | Git repository (tagged release commit)                                              | `git rev-parse` / signed Git tag             |
| Toolchain identity      | `rust-toolchain.toml` (channel + components)                                        | rustup; recorded in the build manifest       |
| `Cargo.lock`            | committed lockfile                                                                  | `--locked` builds fail on drift              |
| Build workflow identity | pinned workflow SHA (`.github/workflows/`)                                          | repository audit; workflow digests           |
| Binary digest           | SHA-256 over the release artifact bytes                                             | `sha256sum`; recorded alongside the artifact |
| Domain artifact digest  | SHA-256 over each optional domain component                                         | independent digest per domain artifact       |
| SBOM                    | CycloneDX generated from the actual resolved build (not merely workspace manifests) | SBOM verification tooling (Stage 6)          |
| Provenance attestation  | future attestation service (SLSA-style, Stage 6)                                    | attestation verifier                         |
| Release version         | workspace version (currently 0.0.0) + release tag                                   | `siralos --version` parity check             |

## Guarantees and non-claims

- "Same dependencies" and "byte-identical binary" are different
  guarantees. Siralos does not claim reproducible binaries until
  independently demonstrated on at least two clean environments.
- A Siralos binary attestation never implies trust in an unrelated
  domain artifact: Godot domain components are signed/attested
  independently when distributed independently.
- Every released artifact carries a SHA-256 digest regardless of later
  attestation mechanism.

## SBOM readiness

- Intended format: CycloneDX (JSON), generated from the actual resolved
  dependency graph of the built artifact (e.g. `cargo cyclonedx` over
  the locked build), so the SBOM corresponds to the produced artifact
  rather than merely listing workspace manifests.
- Producer: the release workflow; artifact binding: SBOM digest is
  recorded beside the binary digest; release storage: alongside the
  release artifacts; verification path: SBOM verifier compares the
  locked graph at build time.
- No SBOM tooling is introduced in this milestone; the decision is
  recorded so Stage 6 adds tooling deliberately.

## Attestation readiness (GitHub Releases/Actions)

- Publication authority is isolated from normal CI: ordinary validation
  jobs hold `contents: read` only; a future release workflow is the
  sole job class with publication authority (scoped, minimal).
- Release artifacts are produced with SHA-256 digests from day one.
- Optional Godot packages, when distributed, are signed/attested
  independently of the core executable.
