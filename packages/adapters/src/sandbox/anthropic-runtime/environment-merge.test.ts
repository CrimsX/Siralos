import { describe, expect, it } from "vitest";
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

  it("never lets wrapper values override Solaris-controlled variables", () => {
    const merged = mergeWrapperEnvironment(BASE, {
      HOME: "/real/host/home",
      TEMP: "/real/host/tmp",
      PATH: "/real/host/bin",
    });
    expect(merged["HOME"]).toBe("/sandbox/home");
    expect(merged["TEMP"]).toBe("/sandbox/temp");
    expect(merged["PATH"]).toBe("C:\\host\\bin");
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
      { PATH: "C:\\solaris\\bin", Home: "/sandbox/home" },
      { path: "/evil/path", HOME: "/evil/home", PathExt: ".COM" },
      "win32",
    );
    expect(merged["PATH"]).toBe("C:\\solaris\\bin");
    expect(merged["Home"]).toBe("/sandbox/home");
    expect(merged["PathExt"]).toBe(".COM");
  });

  it("emits canonical keys without aliases on Windows", () => {
    const merged = mergeWrapperEnvironment(
      { PATH: "C:\\solaris\\bin", Path: "C:\\duplicate" },
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
