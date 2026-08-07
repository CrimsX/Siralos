# Roadmap

Six agreed product stages. Each stage is a milestone, not a feature catalogue.

## 1. Harness foundation

An executable, provider-neutral foundation: interactive CLI, provider port, deterministic fake provider, in-process sessions, engineering standards, mechanically enforced architecture, a secure bounded tool loop with read-only workspace inspection tools, a sandbox/permission boundary (capability policy, built-in profiles, an isolated Sandbox Runtime backend with an enforced host-read allowlist, allowlist child environments, fixed conformance probes), approved workspace mutations (create, exact-replacement edit, delete — each with complete previews, one-time approval, hash conflict detection, identity-bound quarantine commits, and post-write verification), read-only Git inspection plus Solaris-owned recovery checkpoints and conflict-safe undo (trusted Git execution, `git.status`/`git.diff`, durable per-file checkpoints, crash reconciliation, `/git-status`, `/diff`, `/checkpoints`, `/undo`), and sandboxed validation-command execution (`process.run` with the `node-script` runner — immutable private script execution, structured arguments only, read-only workspace, denied network, minimal environment, closed stdin, bounded streamed output, digest-bound one-time approval, timeouts, process-tree cancellation, `validation-offline` profile, command conformance probes, `/commands` and `/cancel`; the `npm-script` runner is defined but fails closed as unavailable until npm execution can be bound to the approved package bytes under the pinned runtime). **Current stage — in progress: the validation-command milestone is complete; the next narrow task is Godot executable discovery, exact-version profiling, project detection, and read-only engine capability probes using a dedicated Godot runner — still read-only and offline.**

## 2. Godot script-development MVP

Godot project detection and understanding, GDScript-first programming workflows, and file-scoped edits validated against the project.

## 3. Godot-native development MVP

Godot editor and runtime integration: scene/resource manipulation, editor-aware workflows, and engine-validated changes.

## 4. Runtime and visual QA

Automated testing, debugging, visual gameplay verification, and performance profiling against a running Godot project.

## 5. Extensibility and optional agents

Skills, game-development-specific agent profiles, and optional user-invoked multi-agent review and comparison.

## 6. Controlled evolution and stable release

Controlled `/evolve` self-improvement workflows, hardening, and a stable release.

Stages 2–6 are not started. The next milestone is stage 2.
