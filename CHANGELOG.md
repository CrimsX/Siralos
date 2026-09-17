# Changelog

Notable changes to Siralos, newest first. The workspace carries **one version
identity**: the manifest version, the release tag, and `siralos --version` are the
same number, and the release workflow refuses a tag that disagrees with it.

## Unreleased — the 1.0 surface

The version identity is not stamped yet. The standalone Godot plugin repository
pins the core crate at `0.0.0`, so the workspace version cannot move until that
independent project updates its pin; the release workflow refuses to publish a
mislabelled tag in the meantime. Everything below is implemented and gated today,
and the number does not change any of it.

### Added

- **Headless mode.** `siralos --print "<prompt>" [--json] [--cwd <dir>]` answers
  one prompt with no interactive frontend, which is what makes scripted and CI
  use possible. The grammar refuses a repeated flag instead of silently taking
  the last one.
- **A documentation-truth gate** (`npm run check:docs-truth`): every
  repo-relative path token in a live document must resolve, every cited
  `npm run` script must exist, and milestone status may live only in the
  canonical status file.
- **A digest-bound supersession list** for the differential corpus, so a frozen
  reference record can be retired for a value that legitimately changed — with
  the reason, the decision, both record digests, and the retired value printed
  in the audit rather than edited out of the evidence.
- **An adversarial suite for that mechanism**
  (`npm run check:supersessions`): seven deliberately broken lists, each of
  which must stop the run with its named refusal.

### Changed

- **The live repository is Rust-only.** The TypeScript implementation is archived
  as digest-bound historical evidence, and the differential harness runs in
  pinned mode against the frozen reference records.
- **The differential harness moved into its own excluded workspace**, so a bare
  `git clone` of the product builds without the external Godot plugin.
- **Documentation was rewritten for its readers.** `README.md` is the front door,
  `ARCHITECTURE.md` owns dependency layout, superseded documents moved to
  `docs/archive/`, and a completed entry-gate record moved there with them.

### Security

- The secret-hygiene gate now scans git-ignored files. The one exemption is named
  inside the gate rather than implied by ignore rules.
- Effects that cannot be enforced keep reporting a typed `unavailable` before
  approval, checkpoint creation, or spawn — the posture is unchanged, and the
  [stability contract](docs/development/STABILITY.md) freezes it.

## What a 1.x release does not promise

See the [stability contract](docs/development/STABILITY.md). It lists the frozen
surfaces and, just as deliberately, the things this project refuses to claim.
