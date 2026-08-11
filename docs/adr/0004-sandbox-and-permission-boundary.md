---
id: ADR-0004
status: accepted
domains: [security, sandbox]
paths: [packages/core/src/security/**]
supersedes: []
---
# ADR 0004: Sandbox and permission boundary

Status: accepted

## Context

Solaris must eventually execute development commands, tests, package managers, Git, and Godot processes. Those operations may run untrusted repository code and must not inherit unrestricted access to the host machine. No model-accessible process or write capability exists yet, but the enforcement boundary must exist before the first such capability lands.

## Decision

Establish a provider-independent security boundary in core with enforcement in adapters:

- Core owns the `Capability` model, `CapabilityPolicy`, the pure permission evaluator, the immutable built-in `SandboxProfile`s (`inspect`, `develop-offline`), the `SandboxBackend` port, and the classified `SandboxError` vocabulary.
- Core owns no OS sandbox, no child-process handling, and no environment inspection. Architecture checks enforce this.
- The first concrete backend is `@anthropic-ai/sandbox-runtime` (pinned exactly at `0.0.70`), isolated behind the `SandboxBackend` port inside a single adapter module. Its beta API is wrapped; its errors are normalized; its configuration and command-string APIs never leak into core.
- Child environments are built from an explicit allowlist (`buildChildEnvironment`) with denied credential patterns; `process.env` is never forwarded.
- User-level configuration (`~/.solaris/config.json`) selects the profile and backend. Project repositories can never broaden it.
- Live conformance probes (`npm run test:sandbox`) prove the backend's enforcement before it is trusted; unavailable backends fail closed.

## Consequences

Positive:

- Future write and process tools execute under enforcement from day one rather than retrofitting it.
- Backend choice is replaceable; the contract tests and conformance suite define what any replacement must satisfy.
- Fail-closed behaviour makes a misconfigured or uninstalled backend loud instead of dangerous.
- Credential isolation is mechanical (allowlist + deny patterns + architecture checks), not aspirational.

Negative:

- The Windows backend requires a one-time elevated install; until then Solaris reports `setup-required` and runs nothing sandboxed.
- The Sandbox Runtime API is a beta research preview; the adapter must be re-verified when the pinned version changes.
- Live conformance takes real time and requires a supported platform; standard tests use a fake backend.

## Alternatives rejected

- A custom native sandbox (Seccomp, Seatbelt, or Windows WFP implementation from scratch): a large, security-critical surface with no reason to exist when an Apache-2.0 backend is available behind an adapter.
- Container or VM isolation: heavyweight for an interactive developer tool and still needs the same filesystem/network policy layer.
- A networked or full-access profile: defeats the boundary; deferred until a reviewed requirement appears.
- Backend selection by project configuration: an untrusted repository must not control its own confinement.
- Running probes against a non-available backend: probes run only when the backend reports `available`.
