import { describe, expect, it } from "vitest";
import { DEFAULT_USER_CONFIG, type UserGodotConfig } from "../../config/user-config.js";
import { resolveGodotSelection } from "./selection-request.js";

function withActive(activeInstallation: string): UserGodotConfig {
  return { ...DEFAULT_USER_CONFIG.godot, activeInstallation };
}

function withDiscovery(discoverOnPath: boolean): UserGodotConfig {
  return { ...DEFAULT_USER_CONFIG.godot, discoverOnPath };
}

describe("resolveGodotSelection", () => {
  const config: UserGodotConfig = DEFAULT_USER_CONFIG.godot;

  it("prefers the CLI path above everything", () => {
    const result = resolveGodotSelection({
      cliPath: "C:\\explicit.exe",
      cliInstallationId: null,
      environmentPath: "C:\\env.exe",
      environmentInstallationId: "env-id",
      config: withActive("primary"),
    });
    expect(result).toEqual({
      ok: true,
      preference: { kind: "path", path: "C:\\explicit.exe" },
    });
  });

  it("prefers the CLI installation id over environment and config", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: "cli-id",
      environmentPath: "C:\\env.exe",
      environmentInstallationId: null,
      config: withActive("primary"),
    });
    expect(result).toEqual({
      ok: true,
      preference: { kind: "installation-id", installationId: "cli-id" },
    });
  });

  it("prefers environment overrides over the configured active installation", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: null,
      environmentPath: "C:\\env.exe",
      environmentInstallationId: null,
      config: withActive("primary"),
    });
    expect(result).toEqual({ ok: true, preference: { kind: "path", path: "C:\\env.exe" } });
  });

  it("prefers the configured active installation over automatic selection", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: null,
      environmentPath: null,
      environmentInstallationId: null,
      config: withActive("primary"),
    });
    expect(result).toEqual({ ok: true, preference: { kind: "config-active" } });
  });

  it("falls back to automatic selection when nothing is explicit", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: null,
      environmentPath: null,
      environmentInstallationId: null,
      config,
    });
    expect(result).toEqual({ ok: true, preference: { kind: "auto" } });
  });

  it("selects none when PATH discovery is disabled and nothing is explicit", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: null,
      environmentPath: null,
      environmentInstallationId: null,
      config: withDiscovery(false),
    });
    expect(result).toEqual({ ok: true, preference: { kind: "none" } });
  });

  it("rejects CLI path and CLI installation id together", () => {
    const result = resolveGodotSelection({
      cliPath: "C:\\a.exe",
      cliInstallationId: "primary",
      environmentPath: null,
      environmentInstallationId: null,
      config,
    });
    expect(result).toEqual({
      ok: false,
      message: "--godot-path and --godot-installation are mutually exclusive.",
    });
  });

  it("rejects environment path and environment installation id together", () => {
    const result = resolveGodotSelection({
      cliPath: null,
      cliInstallationId: null,
      environmentPath: "C:\\a.exe",
      environmentInstallationId: "primary",
      config,
    });
    expect(result).toEqual({
      ok: false,
      message: "SIRALOS_GODOT and SIRALOS_GODOT_INSTALLATION are mutually exclusive.",
    });
  });
});
