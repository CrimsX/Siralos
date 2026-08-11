---
id: ADR-0002
status: accepted
domains: [provider, tool-loop]
paths: [packages/core/src/ports/**, packages/adapters/src/providers/**]
supersedes: []
---
# ADR 0002: Provider-neutral tool loop

Status: accepted

## Context

Solaris must let its model provider take actions inside the user's workspace while the harness stays provider-neutral and secure. The foundation slice proved a single provider request per prompt; the next step is a bounded loop where the provider can request tool execution and receive results.

Key constraints:

- No provider-specific message formats in core (no OpenAI-, Anthropic-, Gemini-, or Copilot-specific concepts).
- Tool input originates from a model and must be treated as untrusted.
- The harness must not be able to modify the workspace or read outside it.
- No real model provider exists yet; behaviour must be provable with the deterministic fake provider.

## Decision

Core owns the tool model and the execution loop; adapters own concrete tools:

- Core defines `ToolDefinition`, the `Tool` contract, `ToolExecutionResult`, and an immutable `ToolRegistry` created during composition.
- Core owns the bounded provider/tool loop inside `sendPrompt`: collect a provider turn, execute requested tools sequentially, append results, request another turn, and finish when a turn completes without tool calls.
- Tool calls and tool results are distinct `ConversationItem` types (`assistant_tool_call`, `tool_result`); file content stays classified as tool data and never becomes system or developer instructions.
- Concrete tools live in `@solaris/adapters` behind the core contract. The composition root is the only place that constructs them.
- The loop is bounded by a maximum tool-round count (default 8). Unknown tools, duplicate call ids, malformed input, and cancellation produce typed, safe failures.
- The workspace is the canonicalized launch directory; all tool paths resolve relative to it and are canonicalized and containment-checked.

## Consequences

Positive:

- A real provider adapter can later translate the neutral conversation items into its native tool-call API without core changes.
- The fake provider can demonstrate tool calls end to end, proving the loop before any real provider exists.
- Security properties (containment, read-only operation, no shell, bounded output) are enforced in adapters and tested mechanically.

Negative:

- The loop serializes tool execution; parallel execution is deferred until a requirement justifies its complexity.
- The workspace is a single fixed root; multi-workspace support is deferred.
- Tool output limits are fixed constants; exposing configuration through the CLI is deferred.

## Alternatives rejected

- A separate `@solaris/tools` workspace package: no independent runtime, release cycle, or dependency boundary justifies it yet.
- Dynamic tool discovery or plugin loading: explicit registration in the composition root keeps the capability surface auditable.
- A full JSON Schema validation engine: the three tools validate their own `unknown` input with small typed parsers.
- Executing tools in the CLI: the application layer must remain headless-testable and UI-neutral.
- Allowing the provider to influence execution policy: the registry and limits are immutable after startup.
