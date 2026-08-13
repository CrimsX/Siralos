# Siralos handoff traceability register

Status: authoritative intake register for the external master handoff.

The master handoff introduced stable requirement and work-product ranges that
must not be silently renamed, collapsed, or treated as complete merely because
similar repository concepts exist. The source attachment is not tracked in
this repository, so its individual requirement text cannot yet be reproduced
or mapped without inventing content.

## Reserved source ranges

| Source range                  | Intended role                                               | Repository status                      |
| ----------------------------- | ----------------------------------------------------------- | -------------------------------------- |
| `CORE-001` through `CORE-020` | Domain-neutral host/runtime requirements                    | Source text awaiting checked-in import |
| `HAR-001` through `HAR-055`   | Harness, authority, security, and verification requirements | Source text awaiting checked-in import |
| `RFC-0001` through `RFC-0020` | Architecture decision backlog                               | Source text awaiting checked-in import |
| `AP-001` through `AP-016`     | Permanent anti-pattern register                             | Source text awaiting checked-in import |
| `GT-001` through `GT-018`     | Golden behavioral/replay traces                             | Source text awaiting checked-in import |

These identifiers are reservations, not evidence of implementation. Until the
source text is committed, their status is **unverifiable** and no milestone may
claim them as satisfied.

## Existing repository evidence classes

The following repository-owned identifiers remain authoritative for their own
scope and are candidates for future mapping, not substitutes for the reserved
source requirements:

- `CORE.*` rules in the versioned Execution Contract;
- `S3M*` and `S3R*` milestone acceptance IDs;
- accepted ADR identifiers and frontmatter;
- typed TaskContract acceptance criteria;
- differential scenario IDs and canonical outcome records;
- host-attached evidence IDs and artifact digests.

## Import and mapping procedure

When the source handoff is available as a tracked UTF-8 document:

1. preserve every source ID and its exact normative text;
2. assign an owner, authority class, and applicable migration milestone;
3. map each item to executable evidence, an accepted intentional deviation,
   or a future milestone;
4. map RFC/AP/GT entries to a tracked artifact or explicitly retain them as
   backlog;
5. reject duplicate IDs and dangling evidence references in a deterministic
   repository check;
6. keep source requirements separate from repository acceptance IDs so a
   similarly named rule cannot satisfy them accidentally.

The mapping must use the status vocabulary `verified`, `partial`, `absent`,
`blocked`, `intentional_deviation`, `not_due`, `not_applicable`, or
`unverifiable`. Only host-observed executable evidence can produce `verified`.
