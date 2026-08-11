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
- There is no archived material at this time; this directory documents
  the convention.
