# Contributing to Siralos

Siralos is in active development and an evidence-gated TypeScript-to-Rust
migration. Focused issues and pull requests are welcome, but confirm substantial
scope in a GitHub issue before investing in it; milestone order and security
boundaries are deliberate.

## Start with repository context

1. Read [AGENTS.md](AGENTS.md).
2. Read the [project context](docs/development/PROJECT_CONTEXT.md).
3. Follow the scoped `AGENTS.md` files, accepted ADRs, and the
   [Rust style guide](docs/development/RUST_STYLE.md) relevant to your change.
4. Verify the current milestone from repository evidence. R3 is complete and
   R4 is next; do not pull later migration or product work forward implicitly.

## Development setup

Prerequisites and bootstrap commands are in [README.md](README.md). Install the
locked npm dependencies with `npm ci`; use the checked-in Rust toolchain and
lockfiles.

Keep changes cohesive and preserve the main boundaries:

- TypeScript remains the behavioral reference until its evidence-backed R12
  disposition.
- Rust dependency direction is `siralos-cli -> siralos-adapters ->
siralos-core`; core remains domain-neutral.
- Missing security enforcement fails closed. Do not make an unavailable effect
  appear available with another pathname check, hash window, warning, or
  documentation claim.
- Do not commit credentials, private data, generated output, raw conversation
  exports, or local-machine state.
- Use small Conventional Commit-style commits that pass the checks relevant to
  each boundary.

## Verification

Run the complete local gate before requesting review:

```text
npm run check
```

Useful focused gates include:

```text
npm run check:public
npm run check:docs
npm run check:context
npm run check:differential
npm run check:rust-format
npm run check:rust-clippy
npm run test:rust
```

Opt-in live sandbox or Godot probes require their documented setup. A skipped
probe is never evidence that the boundary passed.

## Pull requests

A pull request should state its scope, linked requirement/ADR where applicable,
tests run, observable behavior changes, security consequences, and any remaining
limitations. Keep generated evidence out of Git unless the repository explicitly
defines it as a deterministic fixture or protocol artifact.

## Security and license

Do not disclose vulnerabilities in a public issue. Follow the private reporting
instructions in [SECURITY.md](SECURITY.md).

No Siralos project license has been published. Do not add or imply a project
license without an explicit repository-owner decision; third-party license and
attribution files do not select the Siralos license.
