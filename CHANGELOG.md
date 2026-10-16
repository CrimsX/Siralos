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

- **Default mutation and execution tools** (owner-directed, 2026-10-13,
  [ADR 0038](docs/adr/0038-default-mutable-execution-tools.md)).
  `read`/`write`/`edit` are live behind their gates (containment,
  protected paths, atomic writers, auto-checkpoint, one-time
  digest-bound per-action approval with a live interactive prompt);
  `bash` reports typed `sandbox_unavailable` (no enforcing backend is
  pinned). File deletion stays unavailable and no undo surface is
  wired. The corpus moved to v85 with seven new `default-tools`
  subjects pinned by candidate-authored expectation records and a
  printed audit; the frozen oracle bundle is byte-identical.

- **Modular terminal UI components**
  ([ADR 0039](docs/adr/0039-modular-terminal-ui-components.md)).
  Declarative panels declared in workspace `.siralos/ui.toml`
  (protected behavioral configuration, host-written through the
  observed-byte + atomic-writer path) or contributed by installed
  modules (`[ui]` manifest section, lifecycle- and authority-gated
  with user ids winning conflicts). The host projects them into a
  tab bar plus 2x2 split drawn through the terminal sanitizer
  (`F2` toggles, `[`/`]` switch tabs; `/ui-list`, `/ui-add`,
  `/ui-edit`, `/ui-remove`, `/ui-preview`). Feeds reuse existing
  read-only surfaces and grant no new authority. The corpus moved
  to v86 with twelve new `ui-components` subjects pinned by
  candidate-authored expectation records and a printed audit; the
  frozen oracle bundle is byte-identical.

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

- **The `godot-*` differential subjects are retired and the external
  plugin coupling is gone** (owner-directed, 2026-10-10, ADR 0037 option
  A executed). A new digest-bound retired-subjects list
  (`tests/differential/evidence/post-freeze/retirements.json`, sibling of
  the supersession list with its own adversarial guards) retires the 34
  pinned `godot-*` records; their scenario files, harness runners, the
  `siralos-godot` path dependency, and the CI plugin provisioning are
  deleted. The frozen oracle bundle is byte-identical. Frozen residue
  stays by design: the `godot` user-config keys, the `siralos:domain-abi`
  wire identity, and the fail-closed `/godot` CLI vocabulary.
- **The domain/plugin vocabulary is one noun: module** (owner-directed,
  2026-10-10, recorded in [ADR 0037](docs/adr/0037-module-composition-and-godot-decoupling.md)).
  The CLI speaks `/modules`, `/modules-add`, `/modules-enable`, and
  `/modules-activate`; install records live under `[modules.<id>]` with a
  folder-root `module-manifest.toml`. Workspace `siralos.toml` files still
  declaring `[plugins.<id>]` tables or a `[profile] plugins` selection must
  rename those keys — the old keys are refused as unknown fields.
  Versioned protocol identifiers, differential subject names, and the
  `godot-*` execution surface are unchanged; the corpus moved to v83 with
  two superseded CLI-vocabulary records, printed in the audit.
- **The `corpus_scenario` fuzz target is rewired to the extracted harness**
  (owner-directed, 2026-10-10). It now fuzzes the harness's own strict
  scenario gate through a minimal public entry point, and the scheduled
  assurance job provisions the pinned plugin checkout the harness build
  needs.
- **The duplicated freeze candidate record is gone** (owner-approved pinned-record
  amendment, 2026-10-10). The freeze evidence's `candidate.json` was
  byte-identical to its sibling `oracle.json`; the evidence loader now confirms
  both recorded digests from the retained copy alone, so the parity claim survives
  as the digests and the frozen audit.
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
