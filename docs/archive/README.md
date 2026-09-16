# Archive

Historical material that must be retained but is **not active guidance**
lives here (ADR 0023 Part R). Archives are excluded from normal
executor-context discovery by the documentation selector
(`isArchivedDocumentationPath`); a document in this directory grants
nothing, even if read as historical data.

Rules:

- Only obsolete material belongs here — never move an accepted ADR into
  the archive merely because it is old. Superseded ADRs stay in
  `docs/adr/` with `status: superseded` and `supersededBy` frontmatter.
- An archived document never carries authority: it is history, not
  policy. The security contract, ENGINEERING.md, and accepted ADRs are
  the active sources.
- Archived here: the completed TypeScript-to-Rust migration record
  (`RUST_MIGRATION.md`), the TypeScript behavior-extraction record
  (`R7_BEHAVIOR_EXTRACTION.md`), the TypeScript-era architecture document
  (`architecture-typescript-era.md`, replaced by [ARCHITECTURE.md](../../ARCHITECTURE.md)
  when the live document was rewritten for the Rust-only tree), and the completed
  pre-Stage-4 assurance gate
  (`stage4-entry-gate.md`, whose 17 criteria were frozen evidence and held). Each
  records work that is finished, or a tree that has been removed, so each is
  history rather than live documentation.
