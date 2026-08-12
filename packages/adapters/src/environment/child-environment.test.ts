import { describe, expect, it } from "vitest";
import {
  buildChildEnvironment,
  environmentKeyOf,
  isDeniedVariable,
  isProtectedEnvironmentKey,
  PROTECTED_ENVIRONMENT_KEYS,
} from "./child-environment.js";

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
    SIRALOS_CONFIG: "/home/user/.siralos/config.json",
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

  it("removes Siralos configuration overrides", () => {
    const environment = buildChildEnvironment(sampleParent(), PATHS);
    expect(environment["SIRALOS_CONFIG"]).toBeUndefined();
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

  it("matches Windows environment keys case-insensitively and emits one canonical spelling", () => {
    // Typical Windows spellings: `Path`, `ComSpec`, `SystemRoot`, `TEMP`.
    const environment = buildChildEnvironment(
      {
        Path: "C:\\Windows\\System32;C:\\tools",
        ComSpec: "C:\\Windows\\System32\\cmd.exe",
        systemroot: "C:\\Windows",
        Temp: "C:\\Users\\test\\AppData\\Local\\Temp",
        Lang: "en_US.UTF-8",
      },
      PATHS,
      "win32",
    );
    // Every allowed variable survives under its canonical allowlist
    // spelling, regardless of the parent's casing.
    expect(environment["PATH"]).toBe("C:\\Windows\\System32;C:\\tools");
    expect(environment["COMSPEC"]).toBe("C:\\Windows\\System32\\cmd.exe");
    expect(environment["SystemRoot"]).toBe("C:\\Windows");
    expect(environment["LANG"]).toBe("en_US.UTF-8");
    // The alternate spellings are deduplicated away.
    expect(environment["Path"]).toBeUndefined();
    expect(environment["ComSpec"]).toBeUndefined();
    expect(environment["systemroot"]).toBeUndefined();
    expect(environment["Temp"]).toBeUndefined();
    expect(environment["Lang"]).toBeUndefined();
  });

  it("deduplicates Path/PATH and ComSpec/COMSPEC aliases with the exact canonical spelling winning", () => {
    const environment = buildChildEnvironment(
      {
        Path: "C:\\alternate",
        PATH: "C:\\canonical",
        comspec: "C:\\lowercase",
        COMSPEC: "C:\\uppercase",
      },
      PATHS,
      "win32",
    );
    expect(environment["PATH"]).toBe("C:\\canonical");
    expect(environment["COMSPEC"]).toBe("C:\\uppercase");
    expect(
      Object.keys(environment).filter((name) => environmentKeyOf(name, "win32") === "path"),
    ).toHaveLength(1);
    expect(
      Object.keys(environment).filter((name) => environmentKeyOf(name, "win32") === "comspec"),
    ).toHaveLength(1);
  });

  it("never lets Siralos-controlled home and temp values be bypassed through alternate casing", () => {
    const environment = buildChildEnvironment(
      {
        home: "/evil/home",
        userprofile: "C:\\evil\\profile",
        temp: "/evil/temp",
        tmp: "/evil/tmp",
        tmpdir: "/evil/tmpdir",
      },
      PATHS,
      "win32",
    );
    // Siralos-controlled values always win; alternate-cased parent values
    // are collapsed into the canonical spellings and overwritten.
    const homeVariables = Object.keys(environment).filter(
      (name) =>
        environmentKeyOf(name, "win32") === "home" ||
        environmentKeyOf(name, "win32") === "userprofile",
    );
    expect(homeVariables.length).toBeGreaterThan(0);
    for (const name of homeVariables) {
      expect(environment[name]).toBe("/sandbox/home");
    }
    expect(environment["TEMP"]).toBe("/sandbox/temp");
    expect(environment["TMP"]).toBe("/sandbox/temp");
    expect(environment["TMPDIR"]).toBe("/sandbox/temp");
  });

  it("emits BOTH HOME and USERPROFILE with the Siralos-controlled home on Windows", () => {
    const environment = buildChildEnvironment(
      { USERPROFILE: "C:\\Users\\host-user" },
      PATHS,
      "win32",
    );
    expect(environment["HOME"]).toBe("/sandbox/home");
    expect(environment["USERPROFILE"]).toBe("/sandbox/home");
    expect(environment["TEMP"]).toBe("/sandbox/temp");
    expect(environment["TMP"]).toBe("/sandbox/temp");
    expect(environment["TMPDIR"]).toBe("/sandbox/temp");
  });

  it("emits HOME only on POSIX, still Siralos-controlled", () => {
    const environment = buildChildEnvironment(
      { HOME: "/home/host-user", USERPROFILE: "C:\\Users\\host-user" },
      PATHS,
      "linux",
    );
    expect(environment["HOME"]).toBe("/sandbox/home");
    expect(environment["USERPROFILE"]).toBeUndefined();
    expect(environment["TEMP"]).toBe("/sandbox/temp");
    expect(environment["TMP"]).toBe("/sandbox/temp");
    expect(environment["TMPDIR"]).toBe("/sandbox/temp");
  });

  it("every emitted protected key carries exactly the Siralos-controlled path", () => {
    for (const platform of ["win32", "linux"] as const) {
      const environment = buildChildEnvironment(
        {
          home: "/evil/home",
          userprofile: "C:\\evil\\profile",
          temp: "/evil/temp",
          tmp: "/evil/tmp",
          tmpdir: "/evil/tmpdir",
          HOME: "/evil/home-upper",
          USERPROFILE: "C:\\evil\\profile-upper",
          TEMP: "/evil/temp-upper",
          TMP: "/evil/tmp-upper",
          TMPDIR: "/evil/tmpdir-upper",
        },
        PATHS,
        platform,
      );
      for (const key of PROTECTED_ENVIRONMENT_KEYS) {
        const value = environment[key];
        if (value === undefined) {
          // On POSIX USERPROFILE is not a home variable and is not emitted.
          expect(platform).toBe("linux");
          expect(key).toBe("USERPROFILE");
          continue;
        }
        const expected =
          key === "HOME" || key === "USERPROFILE" ? "/sandbox/home" : "/sandbox/temp";
        expect(value).toBe(expected);
      }
      // No alternate casing of a protected key can survive.
      const canonical = new Set(PROTECTED_ENVIRONMENT_KEYS.map((key) => key.toLowerCase()));
      for (const [name] of Object.entries(environment)) {
        if (canonical.has(name.toLowerCase()) && !PROTECTED_ENVIRONMENT_KEYS.includes(name)) {
          expect(environment[name]).toBeUndefined();
        }
      }
    }
  });

  it("recognizes protected keys case-insensitively on Windows and case-sensitively on POSIX", () => {
    for (const name of ["HOME", "USERPROFILE", "TEMP", "TMP", "TMPDIR"]) {
      expect(isProtectedEnvironmentKey(name, "win32")).toBe(true);
      expect(isProtectedEnvironmentKey(name, "linux")).toBe(true);
    }
    expect(isProtectedEnvironmentKey("home", "win32")).toBe(true);
    expect(isProtectedEnvironmentKey("UserProfile", "win32")).toBe(true);
    expect(isProtectedEnvironmentKey("temp", "win32")).toBe(true);
    expect(isProtectedEnvironmentKey("TmpDir", "win32")).toBe(true);
    // POSIX keys are case-sensitive: alternate casings are DIFFERENT
    // variables there and are not Siralos-owned.
    expect(isProtectedEnvironmentKey("home", "linux")).toBe(false);
    expect(isProtectedEnvironmentKey("UserProfile", "linux")).toBe(false);
    expect(isProtectedEnvironmentKey("Temp", "linux")).toBe(false);
    expect(isProtectedEnvironmentKey("PATH", "win32")).toBe(false);
    expect(isProtectedEnvironmentKey("PATH", "linux")).toBe(false);
  });

  it("never forwards the host profile or host temp directory values", () => {
    const environment = buildChildEnvironment(
      {
        USERPROFILE: "C:\\Users\\host-user",
        HOME: "/home/host-user",
        TEMP: "C:\\Users\\host-user\\AppData\\Local\\Temp",
        TMP: "/tmp",
        TMPDIR: "/var/tmp",
      },
      PATHS,
      "win32",
    );
    for (const key of ["HOME", "USERPROFILE", "TEMP", "TMP", "TMPDIR"]) {
      expect(environment[key]).not.toMatch(/host-user/);
      expect(environment[key]).not.toBe("/tmp");
      expect(environment[key]).not.toBe("/var/tmp");
    }
  });

  it("preserves case-sensitive matching on POSIX", () => {
    const environment = buildChildEnvironment(
      {
        PATH: "/usr/bin:/bin",
        Path: "/evil/alternate",
        ComSpec: "C:\\evil",
        SystemRoot: "C:\\Windows",
      },
      PATHS,
      "linux",
    );
    // On POSIX `Path` is a DIFFERENT variable from `PATH` and is not on the
    // allowlist, so it is dropped; `PATH` itself is preserved exactly.
    expect(environment["PATH"]).toBe("/usr/bin:/bin");
    expect(environment["Path"]).toBeUndefined();
    expect(environment["ComSpec"]).toBeUndefined();
    expect(environment["SystemRoot"]).toBe("C:\\Windows");
  });

  it("applies deny patterns case-insensitively on Windows casing variants", () => {
    const environment = buildChildEnvironment(
      {
        path: "/usr/bin",
        node_options: "--inspect",
        git_config_count: "99",
        npm_config_userconfig: "/evil/npmrc",
        openrouter_api_key: "sk-fake",
      },
      PATHS,
      "win32",
    );
    // The allowed `path` casing variant survives as canonical PATH; every
    // denied variable is absent regardless of casing.
    expect(environment["PATH"]).toBe("/usr/bin");
    expect(environment["node_options"]).toBeUndefined();
    expect(environment["git_config_count"]).toBeUndefined();
    expect(environment["npm_config_userconfig"]).toBeUndefined();
    expect(environment["openrouter_api_key"]).toBeUndefined();
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
    expect(isDeniedVariable("GODOT_EDITOR_PATH")).toBe(true);
    expect(isDeniedVariable("godot4_editor_path")).toBe(true);
    expect(isDeniedVariable("LD_PRELOAD")).toBe(true);
    expect(isDeniedVariable("LD_LIBRARY_PATH")).toBe(true);
    expect(isDeniedVariable("DYLD_INSERT_LIBRARIES")).toBe(true);
    expect(isDeniedVariable("DYLD_FALLBACK_LIBRARY_PATH")).toBe(true);
    expect(isDeniedVariable("DYLD_VARIABLE")).toBe(true);
    expect(isDeniedVariable("PATH")).toBe(false);
    expect(isDeniedVariable("TOKENIZED")).toBe(false);
    expect(isDeniedVariable("SECRETARY")).toBe(false);
  });

  it("removes Godot and library-injection variables from child environments", () => {
    const environment = buildChildEnvironment(
      {
        GODOT_EDITOR_PATH: "/evil/godot",
        GODOT4_EDITOR_PATH: "C:\\evil\\godot.exe",
        LD_PRELOAD: "/lib/evil.so",
        LD_LIBRARY_PATH: "/lib/evil",
        DYLD_INSERT_LIBRARIES: "/lib/evil.dylib",
        PATH: "/usr/bin",
      },
      { home: "/home", temp: "/tmp" },
    );
    expect(environment["GODOT_EDITOR_PATH"]).toBeUndefined();
    expect(environment["GODOT4_EDITOR_PATH"]).toBeUndefined();
    expect(environment["LD_PRELOAD"]).toBeUndefined();
    expect(environment["LD_LIBRARY_PATH"]).toBeUndefined();
    expect(environment["DYLD_INSERT_LIBRARIES"]).toBeUndefined();
    expect(environment["PATH"]).toBe("/usr/bin");
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
