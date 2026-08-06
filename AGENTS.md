# AGENTS.md

## Current state

- npm TypeScript workspace monorepo (npm workspaces, ESM, strict TypeScript, project references).
- `@solaris/core`: application + provider port + tool contracts and bounded tool loop. `@solaris/adapters`: deterministic fake provider (with tool scenarios) and read-only workspace tools (`workspace.list`, `workspace.read`, `workspace.search`). `@solaris/cli`: interactive terminal with `/help`, `/status`, `/clear`, `/tools`, `/exit`.
- Workspace root is the canonicalized launch directory; all tool paths are contained within it.
- No Godot integration yet; the harness is at the foundation stage.

## Verify

- `npm run check` — full non-mutating validation: format check, lint, typecheck, tests, architecture check.
- `npm run solaris` — build and launch the interactive CLI.
- `npm run check:architecture` — workspace dependency-boundary enforcement.

## Intended direction

- Godot engine agent harness: a harness for an agent driving/running the Godot engine.
- When Godot scaffolding begins, prefer a real Godot project (with `project.godot`) and record new run/verify commands here as they land.

## Gotchas

- Nothing engine-related can run until the Godot project scaffold is in place; `godot` binary must be on `PATH` (or `GODOT` env var set) before claiming anything runs headless.
- Do not add real provider integrations, persistence, multi-agent functionality, or `/evolve` while the foundation stage is unfinished.
