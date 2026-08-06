export const WORKSPACE_LIMITS = {
  maxDirectoryEntries: 200,
  maxReadFileSizeBytes: 512 * 1024,
  maxReadContentChars: 64_000,
  maxSearchFileSizeBytes: 512 * 1024,
  maxSearchFiles: 500,
  maxSearchMatches: 100,
  maxSearchLineLengthChars: 400,
} as const;
