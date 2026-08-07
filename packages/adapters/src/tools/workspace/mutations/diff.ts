import { createTwoFilesPatch } from "diff";
import { WORKSPACE_LIMITS } from "../limits.js";

export interface UnifiedDiff {
  readonly unifiedDiff: string;
  readonly addedLines: number;
  readonly removedLines: number;
}

export type DiffBuildResult =
  | { readonly status: "ready"; readonly diff: UnifiedDiff }
  | { readonly status: "too_large"; readonly message: string };

export function buildUnifiedDiff(
  relativePath: string,
  beforeText: string,
  afterText: string,
): DiffBuildResult {
  const beforeLineCount = countLines(beforeText);
  const afterLineCount = countLines(afterText);
  if (
    beforeLineCount > WORKSPACE_LIMITS.maxDiffLines ||
    afterLineCount > WORKSPACE_LIMITS.maxDiffLines
  ) {
    return {
      status: "too_large",
      message: `The change involves more than ${WORKSPACE_LIMITS.maxDiffLines} lines; it cannot be previewed.`,
    };
  }
  const unifiedDiff = createTwoFilesPatch(
    relativePath,
    relativePath,
    beforeText,
    afterText,
    "",
    "",
    { context: 3 },
  );
  if (Buffer.byteLength(unifiedDiff, "utf8") > WORKSPACE_LIMITS.maxCompleteDiffBytes) {
    return {
      status: "too_large",
      message: "The complete diff exceeds the preview limit and cannot be approved.",
    };
  }
  return {
    status: "ready",
    diff: {
      unifiedDiff,
      addedLines: countAddedLines(unifiedDiff),
      removedLines: countRemovedLines(unifiedDiff),
    },
  };
}

export function countLines(text: string): number {
  if (text.length === 0) {
    return 0;
  }
  const normalized = text.endsWith("\n") ? text.slice(0, -1) : text;
  return normalized.split("\n").length;
}

function countAddedLines(unifiedDiff: string): number {
  let count = 0;
  for (const line of unifiedDiff.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) {
      count += 1;
    }
  }
  return count;
}

function countRemovedLines(unifiedDiff: string): number {
  let count = 0;
  for (const line of unifiedDiff.split("\n")) {
    if (line.startsWith("-") && !line.startsWith("---")) {
      count += 1;
    }
  }
  return count;
}
