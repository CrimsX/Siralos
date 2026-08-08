export const WORKSPACE_LIMITS = {
  maxDirectoryEntries: 200,
  maxReadFileSizeBytes: 512 * 1024,
  maxReadContentChars: 64_000,
  maxSearchFileSizeBytes: 512 * 1024,
  maxSearchFiles: 500,
  maxSearchMatches: 100,
  maxSearchLineLengthChars: 400,
  maxTextFileSizeBytes: 1024 * 1024,
  maxCreatedContentBytes: 512 * 1024,
  maxReplacements: 32,
  maxReplacementTextBytes: 64 * 1024,
  maxCompleteDiffBytes: 256 * 1024,
  maxDiffLines: 10_000,
  /** Independent global traversal bounds for recursive workspace.search. */
  maxSearchDirectories: 2_000,
  maxSearchEntries: 25_000,
  maxSearchFilesConsidered: 2_000,
  maxSearchInputBytes: 64 * 1024 * 1024,
  maxSearchOutputBytes: 200_000,
  maxSearchDurationMs: 10_000,
  /** Maximum directory depth for recursive workspace.search (root counts). */
  maxSearchDepth: 64,
} as const;
