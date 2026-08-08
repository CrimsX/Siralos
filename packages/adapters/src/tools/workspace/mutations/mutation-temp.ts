import { randomUUID } from "node:crypto";
import { lstat, rm, unlink } from "node:fs/promises";
import { join } from "node:path";

export const MUTATION_TEMP_PREFIX = ".solaris-mutation-";

export function createMutationTempPath(targetDirectory: string): string {
  return join(targetDirectory, `${MUTATION_TEMP_PREFIX}${randomUUID()}.tmp`);
}

export interface StagedTempIdentity {
  readonly dev: number;
  readonly ino: number;
}

/**
 * Removes a staged temp file. When the identity of the created object is
 * known (captured from the staging handle), the removal is identity-bound:
 * the path is unlinked only while it still resolves to exactly that object,
 * and a substituted path (a same-user swap) is preserved instead of being
 * deleted. Without an identity the removal is leaf-safe by construction
 * (`rm` with `force` never follows a symbolic link planted at the leaf and
 * never descends into directories; after a link-based commit the temp path
 * is a hard link to the committed object, so removing the temp link leaves
 * the committed target intact).
 */
export async function removeMutationTemp(
  tempPath: string,
  expectedIdentity?: StagedTempIdentity,
): Promise<void> {
  if (expectedIdentity !== undefined) {
    let stats;
    try {
      stats = await lstat(tempPath);
    } catch {
      return;
    }
    if (stats.dev !== expectedIdentity.dev || stats.ino !== expectedIdentity.ino) {
      return;
    }
    await unlink(tempPath).catch(() => undefined);
    return;
  }
  await rm(tempPath, { force: true });
}
