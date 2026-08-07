import { tmpdir } from "node:os";
import { join } from "node:path";

export interface SandboxDirectories {
  readonly home: string;
  readonly temp: string;
}

export function getSandboxDirectories(): SandboxDirectories {
  const base = join(tmpdir(), "solaris-sandbox");
  return {
    home: join(base, "home"),
    temp: join(base, "temp"),
  };
}
