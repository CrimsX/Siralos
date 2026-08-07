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

A permission decision determines whether Solaris permits an operation to proceed. A sandbox profile determines the technical restrictions under which it executes. Approval never means "run the command unrestricted" — at most it will mean "run the command using a broader predefined sandbox profile". Every command runs under the internal `validation-offline` profile regardless of the active user profile, so approval can never make command execution workspace-writable.

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

Child environments are constructed from an explicit allowlist (`buildChildEnvironment` in `packages/adapters/src/environment/`); `process.env` is never forwarded verbatim. Variables matching `*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `AWS_*`, `AZURE_*`, `GOOGLE_*`, `GITHUB_TOKEN`, `GH_TOKEN`, `SSH_AUTH_SOCK`, `NPM_TOKEN`, `NODE_AUTH_TOKEN`, `OPENROUTER_API_KEY`, `DEEPSEEK_API_KEY`, `OPENCODE_API_KEY`, `SOLARIS_CONFIG`, `NODE_OPTIONS`, `BASH_ENV`, `ENV`, `CDPATH`, `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_CONFIG*`, `NPM_CONFIG_USERCONFIG`, `NPM_CONFIG_SCRIPT_SHELL`, and proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) are denied, case-insensitively (Windows environment names are case-insensitive). Sandbox `HOME`/`USERPROFILE` and `TEMP`/`TMP`/`TMPDIR` are controlled by Solaris. Command environments additionally fix `NO_COLOR=1`, `FORCE_COLOR=0`, `TERM=dumb`, and `GIT_TERMINAL_PROMPT=0`, and the npm runner adds safe npm configuration (`NPM_CONFIG_IGNORE_SCRIPTS=true` disables pre/post hooks while the explicitly requested script still runs, plus audit/fund/update-notifier/color off) with the npm cache and user configuration pointed at sandbox-private run paths. The architecture check prohibits `process.env` inspection in package source.

## Built-in profiles

- **`inspect`** (default): workspace read-only, no writes, no process execution, no network. This is the profile under which the read-only workspace tools operate; write and process tools are not sent to the provider.
- **`develop-offline`**: workspace reads allowed, workspace writes require one-time approval, process execution requires one-time approval, network denied, minimal environment, protected metadata paths, timeouts, output limits, cancellation.
- **`validation-offline`** (internal, never user-selectable): the effective profile under which every provider-accessible command executes. Workspace reads allowed, workspace writes denied, sandbox-private home/temp/npm-cache writable, network denied, minimal environment, closed stdin, process-tree confinement required. The active user profile may narrow command execution further, never broaden it. Notably, `develop-offline` permitting approved file edits does not make command execution workspace-writable.

There is no networked, unrestricted, or "full access" profile. No public configuration can set `process.execute` to unconditional `allow` in this milestone; a missing process rule fails closed.

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

`npm run test:sandbox` runs fixed internal conformance probes against the real backend: inside-workspace reads and writes, outside-workspace write denial, denied-secret reads, loopback and DNS network denial, provider-secret absence, descendant-process confinement, output limits, timeout, and cancellation — plus validation-command probes under `validation-offline`: Node and npm scripts read fixtures but cannot write the workspace (root, child, grandchild, and npm script shell all denied), cannot reach outbound or loopback network, receive no provider credentials and no `NODE_OPTIONS`, npm pre/post hooks do not run, stdin is closed, output limits terminate the process, timeout and cancellation terminate descendants that ignore normal termination, no sandbox files appear in the workspace, and sandbox-private run-directory cleanup succeeds. The probes use temporary directories and fake secrets only. The command makes no public internet request, never elevates, and returns nonzero when an available backend violates a required boundary. Unavailable backends produce a loud skipped result, never a passing one.

## Sandboxed development-command execution

`process.run` runs a predefined Solaris runner with structured fields — never a provider-supplied shell string, executable path, environment, network permission, writable path, or stdin. Only two runners exist: `npm-script` (one existing npm package script, executed as `npm run <script> -- <args>` through the trusted Node executable invoking the resolved `npm-cli.js`; pre/post hooks disabled via `NPM_CONFIG_IGNORE_SCRIPTS`) and `node-script` (one `.js`/`.mjs`/`.cjs` file through the exact `process.execPath`; no provider-controlled Node flags).

- Every command requires explicit one-time approval of the exact immutable plan. The approval shows the full repository npm script body, every argument boundary, all execution boundaries (read-only workspace, denied network, minimal environment, closed stdin, timeout, output limits), the npm script-shell notice, the disabled-hooks notice, and the digest prefix. The digest is a SHA-256 over the runner, trusted executable identity and version, script/package hash, repository script body, arguments, working directory, effective profile, environment policy, timeout, output limits, stdin policy, and network policy.
- The approved plan may execute exactly once. Immediately before execution every precondition is revalidated (working directory, script/package file and hash, trusted executable identity, capability policy, sandbox availability, and the recomputed digest); any change is a `conflict` and nothing runs under an earlier approval.
- Commands execute only through the `SandboxBackend` under `validation-offline`; the backend must report full read-only workspace, network-denial, and process-tree confinement capability or the command does not run (fail closed). There is no host-process fallback.
- Each run gets a verified Solaris-owned directory beneath `~/.solaris/runs/<workspace-fingerprint>/<run-id>/` with private `home/`, `tmp/`, and `npm-cache/`. It is removed after completion; cleanup never follows links and never deletes outside the verified root, and cleanup failures are reported truthfully.
- Output streams to the CLI as bounded decoded UTF-8 events (16 KiB per event, line-oriented rendering, terminal sequences sanitized on display). Hard limits are 1 MiB per stream; exceeding them terminates the process (`output-limit`). The provider receives at most 256 KiB per stream with explicit truncation markers and an omitted-bytes note. Nonzero exits are normal completed commands (`exitCode: 2`), never infrastructure failures.
- Timeouts (default 120 s, provider-bounded to 10 minutes) and cancellation terminate the complete process tree, including descendants that ignore normal termination. Cancellation removes timers and listeners, releases the execution lock, attempts run-directory cleanup, and returns the prompt.
- Commands are serialized with the same in-process lock as approved file mutations: a mutation cannot begin while a command runs and vice versa.
- Git structured status is recorded before and after execution as a verification signal. A detected workspace change marks the result `workspace_violation`, disables further command execution for the session, and instructs the user to inspect the workspace; Solaris never auto-repairs. The OS sandbox remains the security boundary; Git status is a signal, not the boundary.
- Bounded in-memory session metadata is kept for completed commands (command id, runner, safe summary, digest, start time, duration, exit code, outcome, truncation flags); full output is never persisted, and `/commands` shows the latest five records.

## Why arbitrary command execution remains deferred

No general shell, arbitrary executable runner, writable command execution, package installation (`npm install`/`ci`/`update`/`exec`/`npx`), interactive stdin, background process, or remote execution exists. The sandbox, profiles, policy evaluator, environment filtering, and conformance suite exist so those tools — when added — execute under enforcement from the first day. The two current runners are deliberately narrow: validated packages and scripts, read-only workspace, offline, approval-gated.

## Why full-access mode does not exist yet

No legitimate current use case justifies an unrestricted profile. It would defeat the security boundary this document describes, and it is deliberately not implemented.
