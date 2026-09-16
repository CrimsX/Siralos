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
  (`RUST_MIGRATION.md`) and the TypeScript behavior-extraction record
  (`R7_BEHAVIOR_EXTRACTION.md`). Both describe work that is finished, or whose
  tree has been removed, so both are history rather than live documentation.
