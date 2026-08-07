# Solaris security

This document describes the Solaris security model: how provider-driven capabilities are gated, how sandboxed processes are confined, and what is deliberately not implemented yet.

## Threat model

Solaris will eventually run development commands, tests, formatters, package managers, Git, and Godot processes on the user's machine. Those operations may execute untrusted repository code. The threat model assumes:

- The **repository being developed is untrusted** — its scripts and build steps may attempt to read, write, or exfiltrate anything the harness can reach.
- The **model provider is untrusted** — it may request arbitrary capabilities; every request is gated and enforced.
- The **user is trusted** but may misconfigure Solaris; defaults are conservative.
- The **host machine outside the configured workspace is sensitive** — credentials, SSH keys, browser profiles, and user documents must not be reachable from sandboxed processes.

The current implementation contains no provider-accessible process or write capability; the security boundary described here is the foundation those future tools will execute under.

## Sandbox versus approvals

A permission decision determines whether Solaris permits an operation to proceed. A sandbox profile determines the technical restrictions under which it executes. Approval never means "run the command unrestricted" — at most it will mean "run the command using a broader predefined sandbox profile". This task establishes the profile and enforcement machinery but no approval workflow, because no model-accessible capability exists yet to approve.

## Fail closed

When the requested policy cannot be enforced the process does not run:

- No silent host-process fallback.
- No weakened profile.
- No network enablement.
- No fallback to an unrestricted backend.
- An actionable capability error is returned instead.

The sandbox backend reports `setup-required`, `dependency-missing`, `unsupported`, `degraded`, or `failed` states; none of them are treated as secure. Live conformance probes run only when the backend reports `available`, and an unavailable backend is reported loudly rather than passed as secure.

## Provider network separation

The model-provider adapter may contact its configured API endpoint from the Solaris host process. Sandboxed child processes never inherit that permission. All built-in sandbox profiles deny outbound network access (`network.outbound: deny`); there is no networked profile. Selecting any provider must never allow a child process to reach the internet.

## Credential isolation

Provider credentials and Solaris internal secrets stay in the provider adapter or host process. They must never be:

- added to child environments,
- written into sandbox configuration,
- included in command arguments,
- added to logs or application events,
- exposed through `/sandbox`, `/permissions`, or the sandbox doctor,
- sent to future tools,
- available to project scripts, Godot, or delegated agents.

Child environments are constructed from an explicit allowlist (`buildChildEnvironment` in `packages/adapters/src/environment/`); `process.env` is never forwarded verbatim. Variables matching `*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `AWS_*`, `AZURE_*`, `GOOGLE_*`, `GITHUB_TOKEN`, `GH_TOKEN`, `SSH_AUTH_SOCK`, `NPM_TOKEN`, `NODE_AUTH_TOKEN`, `OPENROUTER_API_KEY`, `DEEPSEEK_API_KEY`, `OPENCODE_API_KEY`, and `SOLARIS_CONFIG` are denied. Sandbox `HOME`/`USERPROFILE` and `TEMP`/`TMP`/`TMPDIR` are controlled by Solaris. The architecture check prohibits `process.env` inspection in package source.

## Built-in profiles

- **`inspect`** (default): workspace read-only, no writes, no process execution, no network. This is the profile under which the read-only workspace tools operate.
- **`develop-offline`**: workspace read/write, sandbox temporary directories read/write, process execution enabled, network denied, minimal environment, protected metadata paths, timeouts, output limits, cancellation.

There is no networked, unrestricted, or "full access" profile.

## Why network is denied

No current Solaris operation needs outbound network from a sandboxed process. Provider API access happens in the host process. Denying network by default is the conservative baseline; a future profile that enables network would be a new, explicit, security-reviewed decision.

## Why project configuration cannot broaden access

Sandbox configuration is user-level only (`~/.solaris/config.json`). An untrusted repository must not be able to enable network access, add writable roots, select full access, disable environment filtering, change the backend, or disable protected paths. Project-specific restrictions may narrow user-level permissions later, never broaden them.

## Why Sandbox Runtime is an adapter

`@anthropic-ai/sandbox-runtime` (pinned exactly at `0.0.70`, Apache-2.0) is the first concrete OS-sandbox backend, isolated behind the core-owned `SandboxBackend` port. Core knows nothing about it; only the adapter module may import it (enforced by the architecture check). Its API is a beta research preview and may change; Solaris wraps all calls, normalizes its errors into `SandboxError` codes, and never exposes its configuration structures or command-string API through core.

## Platform support

- **Linux / WSL2**: Bubblewrap-based. The adapter reports missing dependencies (`bwrap` etc.) as `dependency-missing` and never disables AppArmor, changes sysctls, installs packages, or falls back to host execution.
- **macOS**: Seatbelt-based. Apple Events are never enabled; `open`/`osascript` escape behaviour is not allowed; network isolation is never weakened.
- **Windows**: The native backend is an **alpha** backend. It requires a one-time elevated setup (`npx sandbox-runtime windows-install`, one UAC prompt). Solaris detects incomplete setup, reports `setup-required` with the exact package-supported command, never runs the elevated install automatically, never triggers UAC, and never stores or inspects the sandbox account password. Native Windows is not called fully supported or security-equivalent until Solaris's own conformance suite passes.

## Protected paths

The `develop-offline` profile denies writes to `.git/`, `.solaris/`, `.env`, `.env.*`, `*.pem`, and `*.key` inside the workspace even though the workspace is writable. Environment filtering remains mandatory regardless; filename protection is defense in depth, not the primary safeguard. Where a backend cannot protect creation of a previously nonexistent matching file (a documented Windows glob-expansion limitation), that platform difference is documented and the backend's own behaviour governs.

## Environment filtering

See "Credential isolation" above. The allowlist keeps only variables required to run ordinary development tools (`PATH`, `SystemRoot`, `WINDIR`, `COMSPEC`, `PATHEXT`, `TEMP`, `TMP`, `TMPDIR`, `LANG`, `LC_ALL`, `TERM`), with sandbox-controlled home and temp values.

## Approved workspace mutations

Write tools (`workspace.create_file`, `workspace.edit_file`, `workspace.delete_file`) run through the full stack: capability policy, a complete reviewable diff, one-time user approval, workspace path safety, protected-path enforcement, hash-based conflict detection, a serialized mutation lock, and post-write verification.

- Approval means **apply this exact prepared mutation once**. It never means future edits, process execution, network access, broader workspace roots, disabled protected paths, or another sandbox profile.
- Providers cannot approve their own actions, set approval defaults, hide diffs, modify policy, or retry a denied request without producing a new proposal that requires a new approval.
- A mutation does not execute when: the policy denies it, the profile forbids writes, no reviewer is available, the user rejects, input is invalid, the target changed after preparation, a path is protected or escapes the workspace, a target or parent is a symbolic link, the file is unsupported, the preview is oversized, preconditions cannot be revalidated, the write fails, or post-write verification fails.
- `workspace.write` is `ask` under `develop-offline` and `deny` under `inspect`; under `inspect` write tools are not sent to the provider at all. There is no `autoApproveEdits` setting and no workspace-write `allow` option in the public configuration.
- Exact SHA-256 hashes are the conflict precondition; `workspace.read` returns the complete-file hash. Timestamps and file sizes are never conflict preconditions.
- Protected paths (`.git/`, `.solaris/`, `.env`, `.env.*`, `*.pem`, `*.key`, Solaris configuration, sandbox metadata, anything outside the workspace) cannot be overridden by approval or provider requests, and are applied again immediately before mutation.
- Mutations are serialized in-process; temp files use unpredictable names, stay in the target directory, are hidden from listings, and are cleaned up after success or failure. Cancellation before the commit section prevents mutation; the short final replacement section is not interrupted midway.

## Git inspection and recovery checkpoints

Git access is read-only and narrowly executed. Solaris runs only fixed inspection commands (`git --version`, `git rev-parse`, `git status`, `git diff`, `git check-ignore`) through a dedicated adapter that uses no shell, fixed argument arrays, an allowlisted subcommand set, disabled pagers/aliases/external diff helpers/textconv (`--no-pager`, self-mapping alias overrides, `--no-ext-diff`, `--no-textconv`), a sanitized allowlist environment with credential patterns removed, `GIT_TERMINAL_PROMPT=0` and `GIT_OPTIONAL_LOCKS=0`, bounded output, timeouts, and cancellation. Providers may only select schema-validated high-level options (diff scope, workspace-relative paths) — never raw Git arguments. There is no Git write capability: staging, commits, reset, restore, checkout, clean, stash, branches, worktrees, and remotes are absent, and the architecture check rejects Git mutation command strings in runtime code. Repository roots must equal the workspace root; a parent repository never broadens the workspace.

Every approved workspace mutation first records a durable Solaris-owned checkpoint: versioned metadata plus the exact pre-change bytes, stored outside the workspace at `~/.solaris/checkpoints/<workspace-fingerprint>/<checkpoint-id>/` (fingerprint = SHA-256 of the canonical workspace path). Checkpoints never depend on Git, the Git index, or the working tree; non-Git workspaces get identical protection.

- **Checkpoints contain local source-code copies.** Preimages may be source code or configuration text. They stay on the local machine: they are never sent to providers, never returned to providers (the store's `loadPreimage` is internal to the undo service), never listed by `/checkpoints`, never indexed by `workspace.search`, and never included in workspace listings. Metadata is bounded and validated on load; preimages are hash-verified against their metadata on read.
- **Storage is fail-closed and tamper-resistant.** Checkpoint metadata is written atomically; checkpoint directories use restrictive permissions where supported (Windows mode bits alone are not equivalent to POSIX ACL enforcement); symbolic links inside the store are rejected; the store root must not resolve inside the workspace; metadata whose fingerprint or relative path does not match the active workspace is rejected; sizes are bounded. Retention is bounded (100 checkpoints / 100 MiB per workspace) and never prunes `prepared`, `uncertain`, or `conflicted` checkpoints; if space cannot be freed, checkpoint creation fails and the mutation does not run.
- **A mutation never runs without its checkpoint.** If checkpoint recording fails, the workspace is not modified. If finalization fails after a successful apply, Solaris reports an uncertain recovery state instead of success, and startup reconciliation resolves it from the recorded before/after hashes.
- **Undo is strictly hash-gated.** `/undo` (and `/undo <checkpoint-id>`) requires the current file to match the checkpoint's recorded post-state exactly; any later user or external change is a `conflict` and nothing is overwritten. Undo shows a complete reverse diff, requires one-time approval, uses the mutation lock and all mutation safety checks, verifies restored bytes against the preimage hash, and marks the checkpoint `undone` only after verification. There is no force undo, no redo, and no provider-accessible undo; providers can never list checkpoint preimages.

## Conformance testing

`npm run test:sandbox` runs fixed internal conformance probes against the real backend: inside-workspace reads and writes, outside-workspace write denial, denied-secret reads, loopback and DNS network denial, provider-secret absence, descendant-process confinement, output limits, timeout, and cancellation. The probes use temporary directories and fake secrets only. The command makes no public internet request, never elevates, and returns nonzero when an available backend violates a required boundary. Unavailable backends produce a loud skipped result, never a passing one.

## Why arbitrary command execution remains deferred

No provider-accessible shell, process, or write tool exists yet. The sandbox, profiles, policy evaluator, environment filtering, and conformance suite exist so those tools — when added — execute under enforcement from the first day.

## Why full-access mode does not exist yet

No legitimate current use case justifies an unrestricted profile. It would defeat the security boundary this document describes, and it is deliberately not implemented.
