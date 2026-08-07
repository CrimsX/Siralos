# AGENTS.md

## Current state

- npm TypeScript workspace monorepo (npm workspaces, ESM, strict TypeScript, project references).
- `@solaris/core`: application + provider port + tool contracts and bounded tool loop + security model (capability policy, sandbox profiles, backend port, one-time approval port). `@solaris/adapters`: deterministic fake provider (with read and write scenarios), read-only workspace tools (`workspace.list`, `workspace.read`, `workspace.search`), approved mutation tools (`workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`), user config, allowlist child environments, and the Anthropic Sandbox Runtime backend (pinned `0.0.70`). `@solaris/cli`: interactive terminal with `/help`, `/status`, `/clear`, `/tools`, `/sandbox`, `/permissions`, `/exit`, an interactive approval reviewer, plus `--sandbox-doctor`.
- Workspace root is the canonicalized launch directory; all tool paths are contained within it. Sandbox config is user-level (`~/.solaris/config.json`); write tools require `develop-offline` and one-time approval.
- No Godot integration yet; the harness is at the foundation stage. No provider-accessible process execution exists.

## Verify

- `npm run check` — full non-mutating validation: format check, lint, typecheck, tests, architecture check.
- `npm run test:sandbox` — live sandbox conformance probes (loudly skips when the backend is unavailable, e.g. Windows without the one-time `npx sandbox-runtime windows-install` setup).
- `npm run solaris` — build and launch the interactive CLI.
- `npm run check:architecture` — workspace dependency-boundary enforcement.

## Intended direction

- Godot engine agent harness: a harness for an agent driving/running the Godot engine.
- When Godot scaffolding begins, prefer a real Godot project (with `project.godot`) and record new run/verify commands here as they land.

## Gotchas

- Nothing engine-related can run until the Godot project scaffold is in place; `godot` binary must be on `PATH` (or `GODOT` env var set) before claiming anything runs headless.
- Do not add real provider integrations, persistence, multi-agent functionality, or `/evolve` while the foundation stage is unfinished.
