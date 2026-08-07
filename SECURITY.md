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

## Conformance testing

`npm run test:sandbox` runs fixed internal conformance probes against the real backend: inside-workspace reads and writes, outside-workspace write denial, denied-secret reads, loopback and DNS network denial, provider-secret absence, descendant-process confinement, output limits, timeout, and cancellation. The probes use temporary directories and fake secrets only. The command makes no public internet request, never elevates, and returns nonzero when an available backend violates a required boundary. Unavailable backends produce a loud skipped result, never a passing one.

## Why arbitrary command execution remains deferred

No provider-accessible shell, process, or write tool exists yet. The sandbox, profiles, policy evaluator, environment filtering, and conformance suite exist so those tools — when added — execute under enforcement from the first day.

## Why full-access mode does not exist yet

No legitimate current use case justifies an unrestricted profile. It would defeat the security boundary this document describes, and it is deliberately not implemented.
