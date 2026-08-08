import { beforeEach, describe, expect, it } from "vitest";
import { posix, win32 } from "node:path";
import {
  resolveGitExecutableFromPath,
  type GitExecutableCandidateInfo,
} from "./git-cli-adapter.js";

const win = (left: string, right: string): string => win32.join(left, right);
const pos = (left: string, right: string): string => posix.join(left, right);

/**
 * Registry of the candidate paths probed by the resolver, so tests can
 * assert the bounded inspection order deterministically.
 */
function statRegistry(): {
  readonly stat: (candidate: string) => Promise<GitExecutableCandidateInfo | null>;
  readonly probed: () => readonly string[];
} {
  const probed: string[] = [];
  return {
    stat: (candidate: string): Promise<GitExecutableCandidateInfo | null> => {
      probed.push(candidate);
      return Promise.resolve(registryLookup(candidate));
    },
    probed: (): readonly string[] => probed,
  };
}

const registry = new Map<string, GitExecutableCandidateInfo | null>();

function registryLookup(candidate: string): GitExecutableCandidateInfo | null {
  if (!registry.has(candidate)) {
    return null;
  }
  return registry.get(candidate) ?? null;
}

function fileAt(candidate: string): void {
  registry.set(candidate, { isRegularNonLinkFile: true });
}

function symlinkAt(candidate: string): void {
  registry.set(candidate, { isRegularNonLinkFile: false });
}

beforeEach(() => {
  registry.clear();
});

describe("resolveGitExecutableFromPath (isolated platform-parameterized resolver)", () => {
  it("resolves through the Windows `;` delimiter", async () => {
    const first = "C:\\tools";
    const second = "C:\\Program Files\\Git\\bin";
    fileAt(win(first, "git.exe"));
    fileAt(win(second, "git.exe"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: `${first};${second}`,
      delimiter: ";",
      platform: "win32",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(win(first, "git.exe"));
  });

  it("resolves through the Linux/macOS `:` delimiter", async () => {
    fileAt(pos("/usr/local/bin", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/usr/bin:/bin:/usr/local/bin",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe("/usr/local/bin/git");
  });

  it("regression: a POSIX `:` PATH is never parsed with the Windows `;` delimiter", async () => {
    fileAt(pos("/opt/git/bin", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/usr/bin:/opt/git/bin",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe("/opt/git/bin/git");
    // The resolver splits on `:` only: the unsplit path never appears as a
    // candidate prefix.
    expect(registry_.probed()).not.toContain("/usr/bin:/opt/git/bin/git");
  });

  it("drops empty entries safely (they must never resolve as the current directory)", async () => {
    const dir = "C:\\git";
    fileAt(win(dir, "git.exe"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: `;;;${dir};;;;`,
      delimiter: ";",
      platform: "win32",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(win(dir, "git.exe"));
    // Empty entries were never joined into a relative candidate.
    expect(registry_.probed()).toEqual([win(dir, "git.exe")]);
  });

  it("skips relative entries entirely", async () => {
    fileAt(pos("/usr/local/bin", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "./bin:/usr/local/bin",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(pos("/usr/local/bin", "git"));
    // The relative candidate was rejected before any stat.
    expect(registry_.probed()).toEqual([pos("/usr/local/bin", "git")]);
  });

  it("rejects symlinked executable candidates", async () => {
    symlinkAt(pos("/usr/bin", "git"));
    fileAt(pos("/usr/local/bin", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/usr/bin:/usr/local/bin",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(pos("/usr/local/bin", "git"));
  });

  it("returns null when git is missing from PATH", async () => {
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/usr/bin:/bin",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBeNull();
  });

  it("handles duplicate entries deterministically (first match wins)", async () => {
    fileAt(pos("/first", "git"));
    fileAt(pos("/second", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/first:/second:/first",
      delimiter: ":",
      platform: "linux",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(pos("/first", "git"));
    // The duplicate trailing entry is never inspected.
    expect(registry_.probed()).toEqual([pos("/first", "git")]);
  });

  it("bounds the number of PATH entries inspected", async () => {
    fileAt(pos("/late", "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: "/a:/b:/c:/d:/late",
      delimiter: ":",
      platform: "linux",
      maxEntries: 3,
      stat: registry_.stat,
    });
    expect(resolved).toBeNull();
    // Only the first three entries were probed (one candidate each).
    expect(registry_.probed()).toHaveLength(3);
    expect(registry_.probed()).not.toContain(pos("/late", "git"));
  });

  it("tries the `.exe` spelling before the bare name on Windows", async () => {
    const dir = "C:\\git";
    fileAt(win(dir, "git"));
    const registry_ = statRegistry();
    const resolved = await resolveGitExecutableFromPath({
      pathValue: dir,
      delimiter: ";",
      platform: "win32",
      maxEntries: 64,
      stat: registry_.stat,
    });
    expect(resolved).toBe(win(dir, "git"));
    expect(registry_.probed()).toEqual([win(dir, "git.exe"), win(dir, "git")]);
  });
});
