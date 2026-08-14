# RFC and decision backlog index

Status: authoritative work-item ownership map.

This index preserves the stable `RFC-0001` through `RFC-0020` work-item
identities without manufacturing duplicate RFC documents. Accepted ADRs own
decisions; backlog rows identify work that remains deliberately not due.

| ID       | Work item                         | Ownership status      | Current owner or next decision point                                                                                                                 |
| -------- | --------------------------------- | --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| RFC-0001 | Core Architecture                 | OWNED BY ACCEPTED ADR | [ADR 0001](../adr/0001-modular-monolith.md), [ADR 0032](../adr/0032-rust-migration-and-siralos-rename.md)                                            |
| RFC-0002 | State and Identity Model          | OWNED BY ACCEPTED ADR | [ADR 0014](../adr/0014-task-runtime-foundation.md), [ADR 0028](../adr/0028-canonical-artifact-identity-and-semantic-deltas.md)                       |
| RFC-0003 | Delta Protocol                    | OWNED BY ACCEPTED ADR | [ADR 0028](../adr/0028-canonical-artifact-identity-and-semantic-deltas.md)                                                                           |
| RFC-0004 | Event Journal                     | PARTIALLY OWNED       | [ADR 0014](../adr/0014-task-runtime-foundation.md) and [ADR 0029](../adr/0029-deterministic-execution-and-reproducibility.md); durable journal later |
| RFC-0005 | Deterministic Execution           | OWNED BY ACCEPTED ADR | [ADR 0029](../adr/0029-deterministic-execution-and-reproducibility.md)                                                                               |
| RFC-0006 | Interpretable Context Model       | OWNED BY ACCEPTED ADR | [ADR 0030](../adr/0030-interpretable-context-architecture.md)                                                                                        |
| RFC-0007 | Tool Capability Model             | OWNED BY ACCEPTED ADR | [ADR 0002](../adr/0002-provider-neutral-tool-loop.md), [ADR 0004](../adr/0004-sandbox-and-permission-boundary.md), ADR 0015                          |
| RFC-0008 | Domain API                        | PARTIALLY OWNED       | [ADR 0034](../adr/0034-godot-domain-host-boundary.md); package-facing API is Stage 3R R6                                                             |
| RFC-0009 | Domain Package Format             | BACKLOG / NOT DUE     | Stage 3R R6; no marketplace or package ecosystem before that milestone                                                                               |
| RFC-0010 | Runtime Protocol                  | PARTIALLY OWNED       | [ADR 0031](../adr/0031-runtime-readiness-and-operational-resilience.md), [ADR 0035](../adr/0035-domain-neutral-controlled-runtime-boundary.md)       |
| RFC-0011 | Error and Diagnostic Model        | PARTIALLY OWNED       | Existing typed result contracts; consolidation follows subsystem migration                                                                           |
| RFC-0012 | Persistence and Recovery          | PARTIALLY OWNED       | ADR 0006 and ADR 0031 own bounded pieces; durable task persistence is deferred                                                                       |
| RFC-0013 | Versioning and Compatibility      | PARTIALLY OWNED       | [Protocol versioning](../development/PROTOCOL_VERSIONING.md) and ADR 0034                                                                            |
| RFC-0014 | Security Model                    | OWNED BY ACCEPTED ADR | [ADR 0004](../adr/0004-sandbox-and-permission-boundary.md) and [SECURITY.md](../../SECURITY.md)                                                      |
| RFC-0015 | Model Provider Interface          | OWNED BY ACCEPTED ADR | [ADR 0002](../adr/0002-provider-neutral-tool-loop.md)                                                                                                |
| RFC-0016 | Replay and Reproduction           | OWNED BY ACCEPTED ADR | ADR 0029 and [ADR 0033](../adr/0033-differential-behavioral-harness.md)                                                                              |
| RFC-0017 | Evaluation and Conformance        | OWNED BY ACCEPTED ADR | ADR 0013 and ADR 0033                                                                                                                                |
| RFC-0018 | Package Trust and Supply Chain    | PARTIALLY OWNED       | [Supply-chain policy](../development/SUPPLY_CHAIN.md) and ADR 0034; package ecosystem is not due                                                     |
| RFC-0019 | Tasks, Budgets, and Orchestration | OWNED BY ACCEPTED ADR | ADR 0014, ADR 0020, ADR 0022, ADR 0023, ADR 0031                                                                                                     |
| RFC-0020 | Observability and Trace Model     | PARTIALLY OWNED       | ADR 0014, ADR 0029, ADR 0031; durable runtime trace protocol is later                                                                                |

An item moves to a different ownership status only through repository
evidence and, where it changes an accepted decision, a superseding ADR.
