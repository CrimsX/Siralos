# AGENTS.md

## Current state

- npm TypeScript workspace monorepo (npm workspaces, ESM, strict TypeScript, project references).
- `@solaris/core`: application + provider port + tool contracts and bounded tool loop (strict provider stream protocol, tool-round accounting, transcript pairing invariants) + security model (capability policy, sandbox profiles, backend port, one-time approval port with digest binding). `@solaris/adapters`: deterministic fake provider (with read and write scenarios), read-only workspace tools (`workspace.list`, `workspace.read`, `workspace.search` — the search has independent global traversal bounds), approved mutation tools (`workspace.create_file`, `workspace.edit_file`, `workspace.delete_file` — commit-time revalidation, digest-bound approval, quarantine-based safe replacement on Windows), user config, allowlist child environments, secure filesystem checkpoints with no-follow validation, safe undo, sandboxed command runners (`npm-script`, `node-script`), and the Anthropic Sandbox Runtime backend (pinned `0.0.70`) with an explicit deny-by-default host-read boundary. `@solaris/cli`: interactive terminal with `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/git-status`, `/diff`, `/checkpoints`, `/undo`, `/commands`, `/cancel`, `/exit`, an interactive approval reviewer backed by a cancellable input queue, a single terminal-output sanitization boundary, plus `--sandbox-doctor` with trustworthy exit codes.
- Workspace root is the canonicalized launch directory; all tool paths are contained within it. Sandbox config is user-level (`~/.solaris/config.json`); write tools and commands require `develop-offline` and one-time approval.
- No Godot integration yet; the harness is at the foundation stage. Sandboxed validation command execution (`process.run`) exists; Godot discovery/profiling, real provider integrations, multi-agent functionality, and `/evolve` are not started.

## Verify

- `npm run check` — full non-mutating validation: format check, lint, typecheck, tests, architecture check.
- `npm run test:sandbox` — live sandbox conformance probes (loudly skips when the backend is unavailable, e.g. Windows without the one-time `npx sandbox-runtime windows-install` setup); skipped or unavailable is never treated as passed.
- `npm run solaris` — build and launch the interactive CLI.
- `npm run check:architecture` — workspace dependency-boundary enforcement (structural TypeScript parsing; a developer guardrail, not an OS boundary).

## Intended direction

- Godot engine agent harness: a harness for an agent driving/running the Godot engine.
- When Godot scaffolding begins, prefer a real Godot project (with `project.godot`) and record new run/verify commands here as they land.

## Gotchas

- Nothing engine-related can run until the Godot project scaffold is in place; `godot` binary must be on `PATH` (or `GODOT` env var set) before claiming anything runs headless.
- Do not add real provider integrations, persistence, multi-agent functionality, or `/evolve` while the foundation stage is unfinished.
- Security boundaries are mechanically enforced, not documented: do not solve a security finding with docs, comments, naming, UI text, or mock-only tests. Approval `ask` never executes without the exact one-time protocol; mutations revalidate immediately before their irreversible commit point; the terminal sanitizer is the single output boundary; the input queue is the single terminal read owner.
