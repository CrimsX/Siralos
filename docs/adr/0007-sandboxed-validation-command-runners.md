# ADR 0007: Sandboxed validation-command runners

Status: accepted

## Context

Solaris needs to run development commands (type checking, linting, tests, validation, build diagnostics, and later Godot command-line operation) but must not expose a general shell. The sandbox and permission foundation (ADR 0004), approved workspace mutations (ADR 0005), and Git inspection/checkpoints (ADR 0006) exist, but no provider-accessible command execution does. This milestone adds `process.run` with two Solaris-owned runners — `npm-script` and `node-script` — and proves the full workflow: structured request, validated plan, digest-bound one-time approval, sandboxed execution, streamed bounded output, structured result.

## Decision

- **Solaris does not expose a general shell yet.** No Bash, PowerShell, Command Prompt, arbitrary-executable, or raw-command runner exists. The provider may select only a Solaris-owned runner and its validated fields: runner id, npm script name or JavaScript file path, bounded argument array, workspace-relative working directory, and bounded timeout. Solaris never passes provider-controlled text to `shell: true`, `exec()`, `cmd.exe /c`, or `sh -c`.
- **Runners use structured inputs.** Arguments are a bounded array of strings preserved as separate argv values; shell-looking characters (`&&`, `|`, `>`, `*`, `;`) remain ordinary arguments. Argument boundaries are displayed explicitly in the approval UI.
- **npm and Node are the first runners because they are the toolchain floor.** Every later validation stage (type checking, linting, tests, build diagnostics, Godot discovery scripts) is expressible as an existing npm script or a JavaScript file. The npm runner executes exactly one existing package script as `npm run <script> -- <args>`; the Node runner executes exactly one `.js`/`.mjs`/`.cjs` file through the trusted `process.execPath`. TypeScript execution, `-e`/`--eval`/`--print`/`--require`/`--import`, custom loaders, inspector flags, and provider-controlled Node flags are unsupported.
- **Command execution is workspace-read-only.** Commands run under the internal `validation-offline` profile: the project workspace is readable but never writable, regardless of the active user profile; sandbox-private home, temp, and npm cache are writable. Writable command execution, command checkpoints, and multi-file command rollback are explicitly deferred; provider-accessible file changes continue to use the approved mutation tools and checkpoints. Command changes do not use file checkpoints because commands cannot modify the workspace.
- **Every command requires explicit one-time approval.** The approval shows the exact immutable plan — runner, package/script, working directory, every argument boundary, the complete repository npm script body (bounded at preparation; never truncated before approval), the effective execution boundaries (read-only workspace, denied network, minimal environment, closed stdin, timeout, output limits), the npm script-shell notice, the disabled-hooks notice, and the digest prefix. Approval is bound to a SHA-256 digest over the full canonical plan (runner, executable identity and version, script/package hash, repository script body, arguments, working directory, profile, environment policy, timeout, output limits, stdin and network policy). Approval never persists, never applies to later or different commands, and defaults to denial.
- **npm's exact repository script is shown in full** because the script body is part of what the user approves. The script value is bounded at preparation (32 KiB), so a valid command always shows completely; sanitized rendering prevents repository text from executing terminal sequences.
- **npm pre/post hooks are disabled.** `NPM_CONFIG_IGNORE_SCRIPTS=true` prevents automatically associated `pre<script>`/`post<script>` lifecycle scripts while the explicitly requested script still runs (npm documents that `npm run`-style invocations keep working under `--ignore-scripts`). This is shown in the approval.
- **Solaris invokes npm through trusted Node.** The npm CLI JavaScript file (`npm-cli.js`) is resolved next to the trusted Node installation and invoked as `node <npm-cli.js> run <script> -- <args>` with separate arguments. Raw `npm.cmd` execution and `shell: true` are prohibited at the Solaris process layer; npm may still use its normal platform script shell internally to run the exact repository-defined script, and that shell remains inside the OS sandbox.
- **Network is denied.** All built-in profiles deny outbound network; command execution never receives registry tokens, proxies, or credentials, and no package downloading (`npm install`/`ci`/`update`/`exec`/`npx`) is possible through the tool.
- **stdin is closed.** Child stdin is ignored; commands that require input must fail or time out. No PTY, no terminal forwarding, no hidden password prompts. The approval prompt happens before launch and is separate from child stdin.
- **Outputs are bounded and streamed.** The sandbox backend emits decoded UTF-8 output events (handling split multi-byte sequences and invalid bytes safely); the CLI renders line-oriented sanitized text distinguishing stdout and stderr. Hard limits (1 MiB per stream) terminate the process (`output-limit`); the provider receives at most 256 KiB per stream with explicit truncation flags and an omitted-bytes note. Output is never interpreted as Solaris instructions or parsed as tool calls.
- **Provider credentials are removed.** Command environments come from the allowlisted minimal builder with an extended case-insensitive deny set (`NODE_OPTIONS`, `BASH_ENV`, `ENV`, `CDPATH`, Git overrides, npm user-config and script-shell overrides, proxies) plus fixed safe values; `HOME`/`USERPROFILE` and temp variables point at the per-run sandbox-private directory.
- **A nonzero exit is a valid command result.** A process that runs and exits 2 is `{ status: "completed", exitCode: 2 }` — a normal test/compiler failure, never an infrastructure failure. Infrastructure outcomes are classified separately (denied, conflict, cancelled, timed out, output limit, sandbox denied, sandbox unavailable, workspace violation, failed to start).
- **Execution is serialized and cancellable.** Commands share the approved-mutation lock, so a mutation cannot begin while a command runs and vice versa; a second command waits. Timeouts (default 120 s, maximum 10 minutes) and user cancellation terminate the complete process tree, including descendants that ignore normal termination; timers and listeners are removed, the lock is released, and the run directory is cleaned up. Solaris remains running after cancellation.
- **Sandbox enforcement fails closed.** Commands execute only through the `SandboxBackend`; the backend must report available plus full read-only-workspace, network-denial, and process-tree confinement capabilities, or the command does not run. No weaker isolation, no host-process fallback, no Docker/SSH/browser exposure. The pinned Sandbox Runtime version (`0.0.70`) is unchanged.
- **Run directories are Solaris-owned and disposable.** Each run lives at `~/.solaris/runs/<workspace-fingerprint>/<run-id>/` (outside the workspace, not provider-selectable, verified non-symlinked), contains private `home/`, `tmp/`, and `npm-cache/`, and is removed after completion; cleanup never follows links, never deletes outside the verified root, and reports failure truthfully.
- **Workspace immutability is verified, not assumed.** Git structured status is recorded before and after execution. A detected workspace change marks `workspace_violation`, disables further command execution for the session, and instructs the user to inspect the workspace; Solaris never auto-repairs and never claims Git status detects every filesystem mutation. The OS sandbox remains the security boundary.
- **Writable command execution is deferred.** Commands intentionally cannot modify the project; the next milestone keeps this read-only model and adds Godot executable discovery, exact-version profiling, project detection, and read-only engine capability probes through a dedicated Godot runner.

## Consequences

Positive:

- The type-check/lint/test/validate loop exists with a real security boundary, and later Godot command-line operation can reuse the same approval and sandbox path.
- Providers receive truthful structured results and can distinguish "ran and failed", "could not run", "denied", "cancelled", and "enforcement failed".
- The fake provider exercises the full command workflow deterministically without executing anything.

Negative:

- Command execution requires interactive approval per exact plan; deliberate until a reviewed policy exists.
- npm's internal script shell is outside Solaris's direct control (though inside the sandbox); documented in the approval prompt.
- The Anthropic Sandbox Runtime backend is a beta research preview (pinned `0.0.70`), so live conformance remains the gate: `npm run test:sandbox` must pass on the current platform before the backend is treated as secure. Windows native support requires the one-time `sandbox-runtime windows-install` setup and is not called secure until conformance passes.
- Cancellation leaves a brief flush window while the CLI waits for the next input line to return control (line-oriented terminal design, no full-screen UI).

## Alternatives rejected

- A generic shell/exec runner with approval: defeats the structured-plan security model; rejected.
- Provider-selected executables: unbounded attack surface; rejected.
- Writable command execution with command checkpoints: broadens the sandbox surface before a demonstrated need; deferred per the roadmap.
- Yarn/pnpm/Bun/Deno/Python/dotnet/Java runners: no current requirement; only npm and Node exist.
- Background processes, session-wide approval, allow-always rules, automatic retries, and remote execution: all rejected for this milestone.
