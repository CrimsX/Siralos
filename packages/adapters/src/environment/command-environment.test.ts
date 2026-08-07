import { describe, expect, it } from "vitest";
import { buildCommandEnvironment, type CommandEnvironmentPaths } from "./command-environment.js";

const PATHS: CommandEnvironmentPaths = {
  home: "/run/home",
  temp: "/run/tmp",
  npmCache: "/run/npm-cache",
  npmUserConfig: "/run/npmrc",
};

function sampleParent(): Record<string, string> {
  return {
    PATH: "/usr/bin:/bin",
    SystemRoot: "C:\\Windows",
    COMSPEC: "C:\\Windows\\System32\\cmd.exe",
    OPENROUTER_API_KEY: "sk-fake",
    NPM_TOKEN: "npm-fake",
    NODE_OPTIONS: "--inspect",
    HTTP_PROXY: "http://proxy",
    GIT_DIR: "/evil/.git",
    SSH_AUTH_SOCK: "/run/ssh-agent",
    HOME: "/home/user",
    TEMP: "/tmp",
    RANDOM_USER_VARIABLE: "x",
  };
}

describe("buildCommandEnvironment", () => {
  it("applies the allowlist and drops secrets and overrides", () => {
    const environment = buildCommandEnvironment(sampleParent(), PATHS, { npm: false });
    expect(environment["PATH"]).toBe("/usr/bin:/bin");
    expect(environment["OPENROUTER_API_KEY"]).toBeUndefined();
    expect(environment["NPM_TOKEN"]).toBeUndefined();
    expect(environment["NODE_OPTIONS"]).toBeUndefined();
    expect(environment["HTTP_PROXY"]).toBeUndefined();
    expect(environment["GIT_DIR"]).toBeUndefined();
    expect(environment["SSH_AUTH_SOCK"]).toBeUndefined();
    expect(environment["RANDOM_USER_VARIABLE"]).toBeUndefined();
  });

  it("sets safe fixed values", () => {
    const environment = buildCommandEnvironment(sampleParent(), PATHS, { npm: false });
    expect(environment["NO_COLOR"]).toBe("1");
    expect(environment["FORCE_COLOR"]).toBe("0");
    expect(environment["TERM"]).toBe("dumb");
    expect(environment["GIT_TERMINAL_PROMPT"]).toBe("0");
  });

  it("redirects home and temp to the run directories", () => {
    const environment = buildCommandEnvironment(sampleParent(), PATHS, { npm: false });
    const homeVariables = Object.keys(environment).filter(
      (name) => name === "HOME" || name === "USERPROFILE",
    );
    expect(homeVariables.length).toBeGreaterThan(0);
    for (const name of homeVariables) {
      expect(environment[name]).toBe("/run/home");
    }
    expect(environment["TEMP"]).toBe("/run/tmp");
    expect(environment["TMP"]).toBe("/run/tmp");
    expect(environment["TMPDIR"]).toBe("/run/tmp");
  });

  it("adds npm-specific configuration only for the npm runner", () => {
    const plain = buildCommandEnvironment(sampleParent(), PATHS, { npm: false });
    expect(plain["NPM_CONFIG_IGNORE_SCRIPTS"]).toBeUndefined();
    expect(plain["NPM_CONFIG_CACHE"]).toBeUndefined();
    expect(plain["NPM_CONFIG_USERCONFIG"]).toBeUndefined();
    const npm = buildCommandEnvironment(sampleParent(), PATHS, { npm: true });
    expect(npm["NPM_CONFIG_IGNORE_SCRIPTS"]).toBe("true");
    expect(npm["NPM_CONFIG_AUDIT"]).toBe("false");
    expect(npm["NPM_CONFIG_FUND"]).toBe("false");
    expect(npm["NPM_CONFIG_UPDATE_NOTIFIER"]).toBe("false");
    expect(npm["NPM_CONFIG_COLOR"]).toBe("false");
    expect(npm["NPM_CONFIG_CACHE"]).toBe("/run/npm-cache");
    expect(npm["NPM_CONFIG_USERCONFIG"]).toBe("/run/npmrc");
    expect(npm["NPM_CONFIG_SCRIPT_SHELL"]).toBeUndefined();
  });

  it("never inherits parent npm user configuration", () => {
    const parent = {
      ...sampleParent(),
      NPM_CONFIG_USERCONFIG: "/home/user/.npmrc",
      npm_config_userconfig: "/home/user/.npmrc",
      NPM_CONFIG_SCRIPT_SHELL: "/bin/evil",
    };
    const environment = buildCommandEnvironment(parent, PATHS, { npm: true });
    expect(environment["NPM_CONFIG_USERCONFIG"]).toBe("/run/npmrc");
    expect(environment["npm_config_userconfig"]).toBeUndefined();
    expect(environment["NPM_CONFIG_SCRIPT_SHELL"]).toBeUndefined();
  });

  it("works with an empty parent environment", () => {
    const environment = buildCommandEnvironment({}, PATHS, { npm: true });
    expect(environment["HOME"]).toBe("/run/home");
    expect(environment["TEMP"]).toBe("/run/tmp");
    expect(environment["NO_COLOR"]).toBe("1");
    expect(environment["NPM_CONFIG_CACHE"]).toBe("/run/npm-cache");
  });
});
