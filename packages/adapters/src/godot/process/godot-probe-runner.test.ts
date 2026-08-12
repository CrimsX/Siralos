import { describe, expect, it } from "vitest";
import { createGodotProbeRunner, GODOT_PROBING_UNAVAILABLE_MESSAGE } from "./godot-probe-runner.js";
import type { GodotInstallation } from "@siralos/core";

function validInstallation(): GodotInstallation {
  return {
    id: "probe-test",
    status: "valid",
    source: "user-config",
    sourceLabel: "user config",
    canonicalPath: "C:\\godot\\Godot.exe",
    sizeBytes: 1000,
    modifiedAtMs: 0,
    sha256: "a".repeat(64),
    editionHint: "standard",
  };
}

describe("Godot probe runner fail-closed availability", () => {
  const runner = createGodotProbeRunner({
    backend: {},
    runDirectories: {},
    parentEnvironment: {},
  });

  it("reports probing unavailable", async () => {
    expect(await runner.isAvailable()).toBe(false);
  });

  it("never spawns the executable: every probe reports unavailable", async () => {
    const installation = validInstallation();
    const version = await runner.probeVersion(installation);
    expect(version.status).toBe("unavailable");
    if (version.status === "unavailable") {
      expect(version.message).toBe(GODOT_PROBING_UNAVAILABLE_MESSAGE);
    }
    const help = await runner.probeHelp(installation);
    expect(help.status).toBe("unavailable");
    const api = await runner.dumpExtensionApi(installation);
    expect(api.status).toBe("unavailable");
  });

  it("states the verify-to-launch substitution boundary in the message", () => {
    expect(GODOT_PROBING_UNAVAILABLE_MESSAGE).toContain("substitute");
    expect(GODOT_PROBING_UNAVAILABLE_MESSAGE).toContain("never spawned");
  });
});
