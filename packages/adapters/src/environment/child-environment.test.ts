import { describe, expect, it } from "vitest";
import { buildChildEnvironment, isDeniedVariable } from "./child-environment.js";

const PATHS = {
  home: "/sandbox/home",
  temp: "/sandbox/temp",
};

function sampleParent(): Record<string, string> {
  return {
    PATH: "/usr/bin:/bin",
    SystemRoot: "C:\\Windows",
    COMSPEC: "C:\\Windows\\System32\\cmd.exe",
    LANG: "en_US.UTF-8",
    TERM: "xterm-256color",
    OPENROUTER_API_KEY: "sk-fake-openrouter",
    DEEPSEEK_API_KEY: "sk-fake-deepseek",
    OPENCODE_API_KEY: "sk-fake-opencode",
    GITHUB_TOKEN: "ghp-fake",
    GH_TOKEN: "gho-fake",
    NPM_TOKEN: "npm-fake",
    NODE_AUTH_TOKEN: "node-fake",
    AWS_ACCESS_KEY_ID: "AKIA-fake",
    AWS_SECRET_ACCESS_KEY: "fake-secret",
    AZURE_CLIENT_SECRET: "azure-fake",
    GOOGLE_API_KEY: "google-fake",
    SSH_AUTH_SOCK: "/run/user/1000/ssh-agent",
    SOLARIS_CONFIG: "/home/user/.solaris/config.json",
    MY_CUSTOM_TOKEN: "custom-fake",
    DATABASE_PASSWORD: "db-fake",
    MY_SECRET_VALUE: "secret-fake",
    HOME: "/home/user",
    USERPROFILE: "C:\\Users\\user",
    TEMP: "/tmp",
    TMP: "/tmp",
    TMPDIR: "/tmp",
    RANDOM_USER_VARIABLE: "irrelevant",
  };
}

describe("buildChildEnvironment", () => {
  it("preserves minimal safe variables", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["PATH"]).toBe("/usr/bin:/bin");
    expect(environment["SystemRoot"]).toBe("C:\\Windows");
    expect(environment["LANG"]).toBe("en_US.UTF-8");
    expect(environment["TERM"]).toBe("xterm-256color");
  });

  it("removes provider API keys", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["OPENROUTER_API_KEY"]).toBeUndefined();
    expect(environment["DEEPSEEK_API_KEY"]).toBeUndefined();
    expect(environment["OPENCODE_API_KEY"]).toBeUndefined();
  });

  it("removes generic token variables", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["GITHUB_TOKEN"]).toBeUndefined();
    expect(environment["GH_TOKEN"]).toBeUndefined();
    expect(environment["NODE_AUTH_TOKEN"]).toBeUndefined();
    expect(environment["MY_CUSTOM_TOKEN"]).toBeUndefined();
  });

  it("removes SSH agent variables", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["SSH_AUTH_SOCK"]).toBeUndefined();
  });

  it("removes cloud credentials", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["AWS_ACCESS_KEY_ID"]).toBeUndefined();
    expect(environment["AWS_SECRET_ACCESS_KEY"]).toBeUndefined();
    expect(environment["AZURE_CLIENT_SECRET"]).toBeUndefined();
    expect(environment["GOOGLE_API_KEY"]).toBeUndefined();
  });

  it("removes registry tokens and passwords", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["NPM_TOKEN"]).toBeUndefined();
    expect(environment["DATABASE_PASSWORD"]).toBeUndefined();
    expect(environment["MY_SECRET_VALUE"]).toBeUndefined();
  });

  it("removes Solaris configuration overrides", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["SOLARIS_CONFIG"]).toBeUndefined();
  });

  it("drops unrelated user variables that are not on the allowlist", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["RANDOM_USER_VARIABLE"]).toBeUndefined();
  });

  it("controls sandbox home and temp values", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    const homeVariables = Object.keys(environment).filter(
      (name) => name === "HOME" || name === "USERPROFILE",
    );
    expect(homeVariables.length).toBeGreaterThan(0);
    for (const name of homeVariables) {
      expect(environment[name]).toBe("/sandbox/home");
    }
    expect(environment["TEMP"]).toBe("/sandbox/temp");
    expect(environment["TMP"]).toBe("/sandbox/temp");
    expect(environment["TMPDIR"]).toBe("/sandbox/temp");
  });

  it("never returns the parent environment verbatim", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    const parent = sampleParent();
    expect(Object.keys(environment).sort()).not.toEqual(Object.keys(parent).sort());
  });

  it("handles an empty parent environment", () => {
    const environment = buildChildEnvironment({}, PATHS);
    expect(environment["HOME"]).toBe("/sandbox/home");
    expect(environment["TEMP"]).toBe("/sandbox/temp");
  });
});

describe("isDeniedVariable", () => {
  it("classifies the required deny patterns", () => {
    expect(isDeniedVariable("MY_API_KEY")).toBe(true);
    expect(isDeniedVariable("SOME_TOKEN")).toBe(true);
    expect(isDeniedVariable("A_SECRET")).toBe(true);
    expect(isDeniedVariable("PASSWORD")).toBe(true);
    expect(isDeniedVariable("AWS_REGION")).toBe(true);
    expect(isDeniedVariable("AZURE_TENANT")).toBe(true);
    expect(isDeniedVariable("GOOGLE_CLOUD_PROJECT")).toBe(true);
    expect(isDeniedVariable("NPM_TOKEN")).toBe(true);
    expect(isDeniedVariable("PATH")).toBe(false);
    expect(isDeniedVariable("TOKENIZED")).toBe(false);
    expect(isDeniedVariable("SECRETARY")).toBe(false);
  });
});
