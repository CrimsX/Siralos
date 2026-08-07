import { randomUUID } from "node:crypto";
import { rm } from "node:fs/promises";
import { join } from "node:path";

export const MUTATION_TEMP_PREFIX = ".solaris-mutation-";

export function createMutationTempPath(targetDirectory: string): string {
  return join(targetDirectory, `${MUTATION_TEMP_PREFIX}${randomUUID()}.tmp`);
}

export async function removeMutationTemp(tempPath: string): Promise<void> {
  await rm(tempPath, { force: true });
}
