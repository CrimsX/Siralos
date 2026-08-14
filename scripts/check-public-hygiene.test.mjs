import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { runCheck, scanPublicText, scanTrackedPath } from "./check-public-hygiene.mjs";

const tempDirectories = [];

function writeFixture(files) {
  const root = mkdtempSync(join(tmpdir(), "siralos-public-hygiene-"));
  tempDirectories.push(root);
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return root;
}

afterEach(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe("scanTrackedPath", () => {
  it("rejects tracked local output, environment files, and raw conversation exports", () => {
    expect(scanTrackedPath("target/debug/app")).toEqual([
      { category: "tracked_generated_or_local_artifact", path: "target/debug/app" },
    ]);
    expect(scanTrackedPath(".env.local")).toEqual([
      { category: "tracked_private_or_local_file", path: ".env.local" },
    ]);
    expect(scanTrackedPath("notes/raw-handoff.txt")).toEqual([
      { category: "tracked_raw_conversation_export", path: "notes/raw-handoff.txt" },
    ]);
  });

  it("allows deliberate examples and deterministic corpus files", () => {
    expect(scanTrackedPath(".env.example")).toEqual([]);
    expect(scanTrackedPath("tests/differential/corpus/manifest.json")).toEqual([]);
    expect(scanTrackedPath("packages/core/src/domain/conversation.ts")).toEqual([]);
  });
});

describe("scanPublicText", () => {
  it("reports only redacted categories and locations", () => {
    const privateKey = ["-----BEGIN ", "PRIVATE", " KEY-----"].join("");
    const privateEmail = ["person", "@", "personal.test"].join("");
    const privateHome = ["C:", "\\", "Users", "\\", "RealPerson", "\\", "repo"].join("");
    const violations = scanPublicText(
      "docs/private.txt",
      [privateKey, privateEmail, privateHome].join("\n"),
    );

    expect(violations).toEqual([
      { category: "private_key_block", path: "docs/private.txt", line: 1 },
      { category: "non_public_email", path: "docs/private.txt", line: 2 },
      { category: "non_synthetic_home_path", path: "docs/private.txt", line: 3 },
    ]);
    expect(JSON.stringify(violations)).not.toContain("RealPerson");
    expect(JSON.stringify(violations)).not.toContain("personal.test");
  });

  it("allows unmistakably synthetic fixtures and public platform attribution", () => {
    expect(
      scanPublicText(
        "tests/fixture.txt",
        [
          "example@example.com",
          "fixture@example.invalid",
          "123+CrimsX@users.noreply.github.com",
          "/home/user/project",
          "C:\\Users\\TestUser\\project",
        ].join("\n"),
      ),
    ).toEqual([]);
  });
});

describe("runCheck", () => {
  it("scans only the explicit tracked set", () => {
    const root = writeFixture({
      "README.md": "# Public\n",
      "notes/raw-handoff.txt": "not tracked\n",
    });

    expect(runCheck(root, ["README.md"])).toEqual({ ok: true, violations: [] });
  });

  it("fails closed when a tracked path is missing", () => {
    const root = writeFixture({ "README.md": "# Public\n" });

    expect(runCheck(root, ["missing.txt"])).toEqual({
      ok: false,
      violations: [{ category: "tracked_file_unreadable", path: "missing.txt" }],
    });
  });
});
