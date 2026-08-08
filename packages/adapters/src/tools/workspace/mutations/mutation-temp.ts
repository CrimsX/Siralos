import { randomUUID } from "node:crypto";
import { rm } from "node:fs/promises";
import { join } from "node:path";

export const MUTATION_TEMP_PREFIX = ".solaris-mutation-";

export function createMutationTempPath(targetDirectory: string): string {
  return join(targetDirectory, `${MUTATION_TEMP_PREFIX}${randomUUID()}.tmp`);
}

/**
 * Removes a staged temp file by unlink (rm force never follows links and
 * never descends into directories). After a link-based commit the temp path
 * is a hard link to the committed object: removing the temp link leaves the
 * committed target intact, so callers can always remove the temp after the
 * outcome is known.
 */
export async function removeMutationTemp(tempPath: string): Promise<void> {
  await rm(tempPath, { force: true });
}
