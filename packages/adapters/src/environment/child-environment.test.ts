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
    expect(isDeniedVariable("DB_PASSWORD")).toBe(true);
    expect(isDeniedVariable("AWS_REGION")).toBe(true);
    expect(isDeniedVariable("AZURE_TENANT")).toBe(true);
    expect(isDeniedVariable("GOOGLE_CLOUD_PROJECT")).toBe(true);
    expect(isDeniedVariable("NPM_TOKEN")).toBe(true);
    expect(isDeniedVariable("NODE_OPTIONS")).toBe(true);
    expect(isDeniedVariable("BASH_ENV")).toBe(true);
    expect(isDeniedVariable("ENV")).toBe(true);
    expect(isDeniedVariable("CDPATH")).toBe(true);
    expect(isDeniedVariable("GIT_DIR")).toBe(true);
    expect(isDeniedVariable("GIT_WORK_TREE")).toBe(true);
    expect(isDeniedVariable("GIT_INDEX_FILE")).toBe(true);
    expect(isDeniedVariable("GIT_CONFIG")).toBe(true);
    expect(isDeniedVariable("GIT_CONFIG_GLOBAL")).toBe(true);
    expect(isDeniedVariable("GIT_CONFIG_SYSTEM")).toBe(true);
    expect(isDeniedVariable("NPM_CONFIG_USERCONFIG")).toBe(true);
    expect(isDeniedVariable("npm_config_userconfig")).toBe(true);
    expect(isDeniedVariable("NPM_CONFIG_SCRIPT_SHELL")).toBe(true);
    expect(isDeniedVariable("npm_config_script_shell")).toBe(true);
    expect(isDeniedVariable("HTTP_PROXY")).toBe(true);
    expect(isDeniedVariable("HTTPS_PROXY")).toBe(true);
    expect(isDeniedVariable("ALL_PROXY")).toBe(true);
    expect(isDeniedVariable("NO_PROXY")).toBe(true);
    expect(isDeniedVariable("PATH")).toBe(false);
    expect(isDeniedVariable("TOKENIZED")).toBe(false);
    expect(isDeniedVariable("SECRETARY")).toBe(false);
  });

  it("removes secret names case-insensitively", () => {
    const environment = buildChildEnvironment(
      {
        openrouter_api_key: "lower-fake",
        OpenRouter_API_Key: "mixed-fake",
        github_token: "lower-gh",
        aws_access_key_id: "lower-aws",
        node_options: "--inspect",
        npm_config_userconfig: "/evil/npmrc",
        npm_config_script_shell: "/bin/evil",
        https_proxy: "http://proxy",
        My_CUSTOM_PASSWORD: "lower-pass",
      },
      PATHS,
    );
    expect(environment["openrouter_api_key"]).toBeUndefined();
    expect(environment["OpenRouter_API_Key"]).toBeUndefined();
    expect(environment["github_token"]).toBeUndefined();
    expect(environment["aws_access_key_id"]).toBeUndefined();
    expect(environment["node_options"]).toBeUndefined();
    expect(environment["npm_config_userconfig"]).toBeUndefined();
    expect(environment["npm_config_script_shell"]).toBeUndefined();
    expect(environment["https_proxy"]).toBeUndefined();
    expect(environment["My_CUSTOM_PASSWORD"]).toBeUndefined();
  });
});
