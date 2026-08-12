import { tmpdir } from "node:os";
import { join } from "node:path";

export interface SandboxDirectories {
  readonly home: string;
  readonly temp: string;
}

/**
 * Siralos-owned private home/temp for host-side tooling (e.g. the git CLI
 * adapter's subprocesses). These shared directories are NOT granted to
 * sandboxed commands: every sandboxed command gets only its own per-run
 * directory (runDirectory/home and runDirectory/tmp) via the backend's
 * per-execution configuration.
 */
export function getSandboxDirectories(): SandboxDirectories {
  const base = join(tmpdir(), "siralos-sandbox");
  return {
    home: join(base, "home"),
    temp: join(base, "temp"),
  };
}
