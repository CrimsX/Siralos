# AGENTS.md

## Current state

- Blank-slate repo: only `README.md` exists; initial commit only. No Godot project files (`project.godot`, `.gd`, `.csproj`) and no build/test/CI config yet.
- Do not hunt for build, test, or lint commands — they do not exist yet.

## Intended direction

- Godot engine agent harness: a harness for an agent driving/running the Godot engine.
- When scaffolding begins, prefer creating a real Godot project (with `project.godot`) over a loose collection of scripts, and keep run/verify commands documented here as they land.

## Gotchas

- Nothing can be run or verified until the Godot project scaffold is in place; `godot` binary must be on `PATH` (or `GODOT` env var set) before claiming anything runs headless.
