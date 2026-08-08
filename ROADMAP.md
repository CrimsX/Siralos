# Roadmap

Six agreed product stages. Each stage is a milestone, not a feature catalogue.

## 1. Harness foundation

An executable, provider-neutral foundation: interactive CLI, provider port, deterministic fake provider, in-process sessions, engineering standards, mechanically enforced architecture, a secure bounded tool loop with read-only workspace inspection tools, a sandbox/permission boundary (capability policy, built-in profiles, an isolated Sandbox Runtime backend with an enforced host-read allowlist, allowlist child environments, fixed conformance probes), approved workspace mutations (create, exact-replacement edit, delete — each with complete previews, one-time approval, hash conflict detection, identity-bound quarantine commits, and post-write verification), read-only Git inspection plus Solaris-owned recovery checkpoints and conflict-safe undo (trusted Git execution, `git.status`/`git.diff`, durable per-file checkpoints, crash reconciliation, `/git-status`, `/diff`, `/checkpoints`, `/undo`), and sandboxed validation-command execution (`process.run` with the `node-script` runner — immutable private script execution, structured arguments only, read-only workspace, denied network, minimal environment, closed stdin, bounded streamed output, digest-bound one-time approval, timeouts, process-tree cancellation, `validation-offline` profile, command conformance probes, `/commands` and `/cancel`; the `npm-script` runner is defined but fails closed as unavailable until npm execution can be bound to the approved package bytes under the pinned runtime). **Stage complete.**

## 2. Godot script-development MVP

Godot project detection and understanding, GDScript-first programming workflows, and file-scoped edits validated against the project.

**Milestone: Godot executable discovery and static project profiling (complete).** Engine discovery happens before any project execution: trusted user-configured installations (absolute paths only, optional edition hints) plus fixed-name PATH search — no broad filesystem scanning, no registry/Spotlight/package-database searches, no shell. Every candidate is fingerprinted exactly (canonical path, size, mtime, SHA-256) and revalidated before each project-independent probe (`--version`, `--help`, `--dump-extension-api`) through the sandbox backend under the internal `godot-probe-offline` profile, with fixed arguments, private run directories, strict timeouts and output bounds, and fail-closed behavior. Version parsing is adversarial and preserves release channels; edition classification is conservative; selection follows a deterministic ranking with recorded rationale (explicit selections never fall back silently); engine profiles cache under `~/.solaris/godot/engine-profiles`. Static project detection reads only the root `project.godot` (never parents/children, nothing evaluated), and the executable-content inventory covers tool scripts, editor plugins, GDExtension descriptors, autoloads, and C# project files without loading or running anything. The surface: `godot.inspect_engine` / `godot.inspect_project` provider tools, `/godot`, `/godot-installations`, `/godot-project`, `/godot-doctor`, the `--godot-path` / `--godot-installation` / `--godot-doctor` startup flags, `SOLARIS_GODOT` / `SOLARIS_GODOT_INSTALLATION` overrides, and the opt-in live conformance (`npm run test:godot` with `SOLARIS_TEST_GODOT`).

The intended sequence for stage 2: Godot executable and project discovery → safe recovery-mode project probe → version-matched Godot knowledge profile → GDScript project intelligence → read-only import and parse diagnostics → Godot script-development MVP. **Next narrow task: an explicit trusted-project decision and a read-only recovery-mode project probe that opens Godot headlessly, disables tool scripts/plugins/GDExtensions, isolates generated `.godot` and editor state where technically possible, captures startup/import diagnostics without modifying authored project files, and fails closed when safe isolation cannot be proven.**

## 3. Godot-native development MVP

Godot editor and runtime integration: scene/resource manipulation, editor-aware workflows, and engine-validated changes.

## 4. Runtime and visual QA

Automated testing, debugging, visual gameplay verification, and performance profiling against a running Godot project.

## 5. Extensibility and optional agents

Skills, game-development-specific agent profiles, and optional user-invoked multi-agent review and comparison.

## 6. Controlled evolution and stable release

Controlled `/evolve` self-improvement workflows, hardening, and a stable release.

Stages 3–6 are not started. Stage 2 is the current stage; the next milestone is its recovery-mode project probe.
