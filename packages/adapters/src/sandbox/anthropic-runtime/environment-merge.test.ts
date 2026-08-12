import { describe, expect, it } from "vitest";
import { buildChildEnvironment } from "../../environment/child-environment.js";
import { mergeWrapperEnvironment } from "./anthropic-sandbox-runtime-backend.js";

const BASE: Readonly<Record<string, string>> = {
  PATH: "C:\\host\\bin",
  HOME: "/sandbox/home",
  TEMP: "/sandbox/temp",
  TERM: "dumb",
};

describe("mergeWrapperEnvironment", () => {
  it("adds wrapper-only runtime-required variables to the child environment", () => {
    const merged = mergeWrapperEnvironment(BASE, {
      SRT_OBSERVE_SOCK: "/tmp/observe.sock",
      SSL_CERT_FILE: "/tmp/ca.pem",
    });
    expect(merged["SRT_OBSERVE_SOCK"]).toBe("/tmp/observe.sock");
    expect(merged["SSL_CERT_FILE"]).toBe("/tmp/ca.pem");
    expect(merged["PATH"]).toBe("C:\\host\\bin");
  });

  it("never lets wrapper values override Siralos-controlled variables", () => {
    const merged = mergeWrapperEnvironment(BASE, {
      HOME: "/real/host/home",
      TEMP: "/real/host/tmp",
      PATH: "/real/host/bin",
    });
    expect(merged["HOME"]).toBe("/sandbox/home");
    expect(merged["TEMP"]).toBe("/sandbox/temp");
    expect(merged["PATH"]).toBe("C:\\host\\bin");
  });

  it("rejects a wrapper-provided USERPROFILE absent from the base on Windows", () => {
    // BASE has HOME but no USERPROFILE: a wrapper USERPROFILE would point
    // at the host user profile, so the merge must fail closed.
    expect(() =>
      mergeWrapperEnvironment(BASE, { USERPROFILE: "C:\\Users\\host-user" }, "win32"),
    ).toThrow(/controls/);
    expect(() =>
      mergeWrapperEnvironment(BASE, { userprofile: "C:\\Users\\host-user" }, "win32"),
    ).toThrow(/controls/);
    expect(() =>
      mergeWrapperEnvironment(BASE, { UserProfile: "C:\\Users\\host-user" }, "win32"),
    ).toThrow(/controls/);
  });

  it("rejects wrapper-provided temp aliases absent from the base", () => {
    // BASE has TEMP but not TMP/TMPDIR: each missing alias must fail closed
    // rather than introduce a host temporary directory. `Temp`/`temp`
    // collide with the TEMP already in the base and are skipped (base wins).
    expect(() => mergeWrapperEnvironment(BASE, { TMP: "/host/tmp" }, "win32")).toThrow(/controls/);
    expect(() => mergeWrapperEnvironment(BASE, { TMPDIR: "/host/tmpdir" }, "win32")).toThrow(
      /controls/,
    );
    expect(() => mergeWrapperEnvironment(BASE, { Tmp: "/host/tmp" }, "win32")).toThrow(/controls/);
    expect(() => mergeWrapperEnvironment(BASE, { TMPDIR: "/host/tmpdir" }, "linux")).toThrow(
      /controls/,
    );
    const merged = mergeWrapperEnvironment(BASE, { Temp: "/host/tmp" }, "win32");
    expect(merged["TEMP"]).toBe("/sandbox/temp");
    expect(merged["Temp"]).toBeUndefined();
  });

  it("rejects a wrapper HOME absent from the base on every platform", () => {
    expect(() =>
      mergeWrapperEnvironment({ PATH: "/usr/bin" }, { HOME: "/host/home" }, "linux"),
    ).toThrow(/controls/);
    expect(() =>
      mergeWrapperEnvironment({ PATH: "/usr/bin" }, { HOME: "/host/home" }, "win32"),
    ).toThrow(/controls/);
  });

  it("keeps every protected key equal to the Siralos-controlled path through the full build+merge pipeline", () => {
    for (const platform of ["win32", "linux"] as const) {
      const base = buildChildEnvironment(
        { HOME: "/host/home", USERPROFILE: "C:\\host\\profile", TEMP: "/host/tmp" },
        { home: "/sandbox/home", temp: "/sandbox/temp" },
        platform,
      );
      // A base built by buildChildEnvironment contains every protected key,
      // so wrapper attempts to smuggle host values collide with the
      // Siralos-controlled values and are skipped — never introduced.
      // On POSIX USERPROFILE is not emitted, so a wrapper USERPROFILE
      // cannot be introduced either and fails closed.
      const hostile =
        platform === "win32"
          ? {
              home: "/host/home",
              userprofile: "C:\\host\\profile",
              temp: "/host/tmp",
              tmp: "/host/tmp",
              tmpdir: "/host/tmp",
            }
          : {
              HOME: "/host/home",
              USERPROFILE: "C:\\host\\profile",
              TEMP: "/host/tmp",
              TMP: "/host/tmp",
              TMPDIR: "/host/tmp",
            };
      if (platform === "linux") {
        expect(() => mergeWrapperEnvironment(base, hostile, platform)).toThrow(/controls/);
        continue;
      }
      const merged = mergeWrapperEnvironment(base, hostile, platform);
      expect(merged["HOME"]).toBe("/sandbox/home");
      expect(merged["USERPROFILE"]).toBe("/sandbox/home");
      expect(merged["TEMP"]).toBe("/sandbox/temp");
      expect(merged["TMP"]).toBe("/sandbox/temp");
      expect(merged["TMPDIR"]).toBe("/sandbox/temp");
      // No hostile casing variant survived.
      expect(merged["home"]).toBeUndefined();
      expect(merged["userprofile"]).toBeUndefined();
      expect(merged["temp"]).toBeUndefined();
      expect(merged["tmp"]).toBeUndefined();
      expect(merged["tmpdir"]).toBeUndefined();
    }
  });

  it("rejects wrapper-required variables that match the deny patterns", () => {
    expect(() => mergeWrapperEnvironment(BASE, { OPENROUTER_API_KEY: "sk-secret" })).toThrow(
      /denies/,
    );
    expect(() => mergeWrapperEnvironment(BASE, { NODE_OPTIONS: "--inspect" })).toThrow(/denies/);
    expect(() => mergeWrapperEnvironment(BASE, { HTTP_PROXY: "http://proxy" })).toThrow(/denies/);
    expect(() => mergeWrapperEnvironment(BASE, { GIT_DIR: "/evil" })).toThrow(/denies/);
  });

  it("cannot bypass filtering with case variants on Windows", () => {
    expect(() =>
      mergeWrapperEnvironment(BASE, { openrouter_api_key: "sk-secret" }, "win32"),
    ).toThrow(/denies/);
    const merged = mergeWrapperEnvironment(
      { PATH: "C:\\siralos\\bin", Home: "/sandbox/home" },
      { path: "/evil/path", HOME: "/evil/home", PathExt: ".COM" },
      "win32",
    );
    expect(merged["PATH"]).toBe("C:\\siralos\\bin");
    expect(merged["Home"]).toBe("/sandbox/home");
    expect(merged["PathExt"]).toBe(".COM");
  });

  it("emits canonical keys without aliases on Windows", () => {
    const merged = mergeWrapperEnvironment(
      { PATH: "C:\\siralos\\bin", Path: "C:\\duplicate" },
      { path: "C:\\wrapper" },
      "win32",
    );
    const keys = Object.keys(merged).filter((name) => name.toLowerCase() === "path");
    expect(keys).toEqual(["PATH"]);
  });

  it("never passes the host environment through wholesale", () => {
    const wrapper = {
      PATH: "/host/bin",
      SRT_OBSERVE_SOCK: "/tmp/sock",
      SECRET_API_KEY: "sk-host",
    };
    expect(() => mergeWrapperEnvironment(BASE, wrapper)).toThrow(/denies/);
    const cleanWrapper = { PATH: "/host/bin", SRT_OBSERVE_SOCK: "/tmp/sock" };
    const merged = mergeWrapperEnvironment(BASE, cleanWrapper);
    expect(Object.keys(merged).sort()).toEqual(
      ["PATH", "HOME", "TEMP", "TERM", "SRT_OBSERVE_SOCK"].sort(),
    );
  });
});
