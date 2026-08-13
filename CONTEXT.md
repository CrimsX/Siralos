# Siralos

Siralos is a deterministic, security-first development and QA harness. Its
host owns authority and state; optional domains contribute intelligence
without acquiring host capabilities.

## Language

**Optional Domain**:
An explicitly installed specialization that contributes domain intelligence
without acquiring host authority. Godot is the first and currently only
Optional Domain.
_Avoid_: Plugin, marketplace extension, built-in engine core

**Behavioral Reference**:
The implementation whose observable behavior defines migration parity until
its evidence-backed retirement or retention disposition. TypeScript is the
current Behavioral Reference.
_Avoid_: Legacy implementation, when it remains authoritative

**Rust Successor**:
The implementation being migrated under differential verification against the
Behavioral Reference. It becomes authoritative only through the migration
gates.
_Avoid_: Rewrite, replacement, when authority has not transferred

**Controlled Runtime Execution**:
A host-authorized, bounded runtime operation that yields structured runtime
evidence without conferring unrestricted authority. It is independent of any
particular engine or optional domain.
_Avoid_: Controlled Godot Execution, when referring to the generic host
boundary

**Godot Runtime Adapter**:
The optional Godot-specific specialization of Controlled Runtime Execution.
It interprets Godot runtime intent and evidence but is not the runtime
authority itself.
_Avoid_: Godot runtime core

**Runtime Evidence**:
Bounded observations and artifacts attributed to one Controlled Runtime
Execution.
_Avoid_: Runtime output, when provenance and bounds matter
