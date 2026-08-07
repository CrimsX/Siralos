# Roadmap

Six agreed product stages. Each stage is a milestone, not a feature catalogue.

## 1. Harness foundation

An executable, provider-neutral foundation: interactive CLI, provider port, deterministic fake provider, in-process sessions, engineering standards, mechanically enforced architecture, a secure bounded tool loop with read-only workspace inspection tools, a sandbox/permission boundary (capability policy, built-in profiles, an isolated Sandbox Runtime backend, allowlist child environments, fixed conformance probes), and approved workspace mutations (create, exact-replacement edit, delete — each with complete previews, one-time approval, hash conflict detection, and post-write verification). **Current stage — in progress: approved mutations are complete; the next narrow task is adding Git status, diff, Solaris-owned checkpoints, and safe undo before adding shell or Godot execution.**

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
