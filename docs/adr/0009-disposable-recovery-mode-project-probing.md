# ADR 0009: Disposable recovery-mode project probing

Status: accepted

## Context

Stage 2 (Godot script-development MVP) needs to know what a project looks
like when Godot actually opens it: startup diagnostics, parser errors,
resource/import warnings, and the generated `.godot` editor/import cache.
ADR 0008 deliberately stopped at static, project-independent probing: the
engine was never pointed at a project, `--path`/`--upwards`/`--import`/
`--scene`/`--script` were prohibited in probe invocation code, and the
architecture check enforced that boundary.

Opening a project is the first moment Godot can execute project-authored
code (`@tool` scripts, editor plugins, GDExtensions) and generate cache
state. The source workspace is untrusted, so Solaris must not hand it to
Godot as a project. Recovery mode reduces editor-side execution risk, but
it is not itself a sandbox: project data can still be arbitrary. The
security boundary for project probing is therefore a combination of user
approval, a disposable project mirror, the OS sandbox, network denial,
credential removal, Godot recovery mode, and workspace-integrity
verification.

## Decision

- **The source workspace is never opened by Godot.** It is never passed as
  a working directory, `--path`, project file, resource path, environment
  variable, editor-data path, or temporary path. The disposable mirror is
  the only project directory visible to the probed engine.
- **A disposable project mirror is constructed before every probe** at a
  Solaris-generated path beneath the verified run root
  (`~/.solaris/runs/<workspace-fingerprint>/<run-id>/project/`). The
  provider cannot select the location, and the project cannot influence
  the copy policy. Only regular files and regular directories are copied;
  symbolic links, junctions, and special files are rejected
  (`mirror_unsupported`), never silently dereferenced or preserved.
  Generated and metadata directories (`.git`, `.godot`, `node_modules`,
  `dist`, `coverage`, `.solaris`, Solaris temp prefixes) are never copied;
  `.gitignore` is never interpreted as a security policy.
- **The mirror is bounded and verified.** Fixed limits: 100,000 files,
  4 GiB total, 512 MiB per file, 1024 UTF-8 bytes per relative path, 64
  levels of directory depth, 120 s preparation deadline. Exceeding any
  bound is `probe_too_large`; the partial mirror is cleaned and the source
  workspace is never opened as a fallback. Every copied byte is
  hash-verified against its source, the source is rechecked during and
  after the copy (a change is a `conflict`), and the mirror is reverified
  (hashes, unexpected files, no symlinks) immediately before Godot starts.
- **Recovery mode is required, not optional.** The selected engine must
  advertise `--recovery-mode`, `--editor`, `--headless`, and `--path`;
  otherwise the probe is `unsupported` and no weaker mode is ever
  substituted. The fixed Solaris-owned invocation is
  `<godot> --headless --editor --recovery-mode --path <mirror>
--quit-after <bounded-count>`, executed with a separate executable and
  argument array (no shell), an external wall-clock timeout in addition to
  `--quit-after`, and never with `--script`, `--scene`, `--import`,
  export, LSP/DAP, debug-server, movie, benchmark, or user arguments.
- **The recovery probe runs under a dedicated internal sandbox profile**
  (`godot-recovery-probe-offline`, never user-selectable): the source
  workspace is never writable and is excluded from the host-read allowlist
  where the backend can enforce it; the mirror and sandbox-private
  home/temp are the only writable roots; network and loopback are denied;
  stdin is closed; the process tree is confined; the environment is the
  minimal allowlist plus removal of Godot editor-path overrides and
  `LD_PRELOAD`/`LD_LIBRARY_PATH`/`DYLD_*` library-injection variables.
- **Every probe requires explicit one-time approval** of a fresh static
  risk manifest (project file hash, engine identity and version, tool
  scripts, enabled editor plugins, GDExtension descriptors and referenced
  libraries, autoloads, .NET projects, and a bounded authored-file
  manifest digest). The approval binds to a prepared-probe digest that
  also covers the fixed recovery command, the mirror-copy policy version,
  the sandbox profile, and the probe limits. If the project or the engine
  changes after approval, the probe is a `conflict` and a new approval is
  required; approvals are never reused, never persisted, and never grant
  normal project execution, plugin/GDExtension loading, or network.
- **Import behavior is reported truthfully.** The probe never passes
  `--import`; opening the editor may still scan and import resources
  inside the mirror. The result distinguishes `project opened`,
  `imports observed`, `imports not observed`, and `import state unknown`
  based on bounded inspection of `.godot` inside the mirror, and reports
  whether `.godot` was generated and its bounded size.
- **Diagnostics are captured conservatively and bounded.** Raw stdout and
  stderr are limited per stream (1 MiB) and by retained line count;
  classified diagnostics recognize well-known Godot markers only (never
  treating arbitrary stdout as structured truth); messages are
  control-character sanitized; counts are capped with explicit truncation.
- **The source workspace integrity is verified before and after.** A
  bounded baseline combines Git status (when available) with a
  deterministic authored-file manifest, because Git alone does not cover
  untracked/ignored state. Any unexpected change yields
  `workspace_changed`; Solaris never auto-reverts external changes.
- **The mirror is destroyed after every probe** (success, diagnostics,
  timeout, cancellation, crash, or preparation failure). Cleanup re-verifies
  containment with no-follow semantics immediately before recursive
  deletion, never accepts a provider-supplied path, never uses broad
  wildcards, never touches other runs, and never touches the source
  workspace. Cleanup failure is reported (`cleanup.completed: false`) and
  never masked as success.
- **Only the approved adapter opens a project.** The architecture check
  allows the `--path` option only in the recovery runner module, requires
  the `--editor`/`--headless`/`--recovery-mode` pairing there, forbids
  literal or workspace-root path values there, and keeps `--path`/
  `--upwards`/`--import`/`--scene`/`--script` prohibited in all other
  probe invocation code. The disposable mirror and the recovery runner are
  reachable only from the probe adapter. Core never copies files or spawns
  Godot; the CLI and provider adapters never spawn Godot.
- **The provider surface is a reviewable tool** (`godot.probe_project`,
  empty input) whose capability is `ask` in the `inspect` and
  `develop-offline` profiles and fails closed elsewhere. No public
  configuration can make it unconditional `allow`. The provider cannot
  choose the executable, arguments, project path, mirror location, limits,
  or environment; the provider result contains no mirror or source
  absolute paths and explicitly states that recovery mode was used and the
  source workspace was not loaded.

## Consequences

Positive:

- The first real Godot project open happens inside a disposable, verified,
  hash-checked copy with the editor in recovery mode under the OS sandbox,
  so project-authored code can only ever run in the mirror context, never
  in the source workspace.
- Approval is meaningful: it names the project state, the engine, the
  isolation properties, and the risks, and it is invalidated by any change
  to the project or engine.
- Import and startup diagnostics become observable without mutating the
  project and without committing to explicit import workflows.
- The source workspace gains a mechanical before/after integrity check
  that does not rely on Git alone.

Negative:

- A full copy of the project is required per probe: local disk space equal
  to the project size (bounded at 4 GiB) is consumed briefly, and large
  projects take longer to mirror.
- Recovery mode suppresses plugin/tool-script execution in most cases but
  the exact behavior depends on the engine build; the probe never claims
  that recovery mode is a sandbox, and the side-effect fixtures in the
  live conformance (`npm run test:godot-recovery`) verify that no
  `@tool`-script or plugin marker appears.
- Real-world diagnostics depend on the engine version; classification is
  deliberately conservative and the raw bounded output is retained
  internally for display.

## Alternatives rejected

- **Opening the source workspace directly under recovery mode.** Rejected:
  Godot could still read, write (`.godot`, editor data), or be influenced
  by project data in the real workspace; recovery mode is not a sandbox.
- **A copy-on-write kernel overlay or VM.** Rejected: no kernel/VM
  dependency at this stage; the copy-then-verify mirror gives the same
  isolation properties for the probe with far less machinery.
- **Using `.gitignore` to select copy content.** Rejected: ignore files are
  author-controlled and not a security policy; exclusions are fixed.
- **Persistent trust ("trust this project forever").** Rejected: approval
  is one-time and digest-bound; a future milestone may add fingerprint-
  bound persistent trust only after the security model matures.
- **Explicit `--import` validation.** Rejected: this milestone is a
  recovery-mode editor startup probe; explicit import-and-quit is a
  separate, later decision.
- **Letting the provider run Godot or choose the mirror path.** Rejected:
  provider input is empty and every execution detail is Solaris-fixed.
