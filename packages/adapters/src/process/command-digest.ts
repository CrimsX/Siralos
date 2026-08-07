import { createHash } from "node:crypto";
import {
  canonicalizeCommandDigest,
  type CommandDigestParts,
  type CommandDigestService,
} from "@solaris/core";

export function createSha256CommandDigestService(): CommandDigestService {
  return {
    compute(parts: CommandDigestParts): string {
      return createHash("sha256").update(canonicalizeCommandDigest(parts), "utf8").digest("hex");
    },
  };
}
