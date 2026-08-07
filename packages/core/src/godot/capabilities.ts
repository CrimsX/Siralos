/**
 * Command capabilities advertised by a Godot executable's `--help` output.
 *
 * Presence here means advertised support, not operationally verified
 * support: the two states are kept distinct (see `verifiedCapabilities` on
 * the engine profile).
 */
export interface GodotCommandCapabilities {
  readonly editor: boolean;
  readonly projectManager: boolean;
  readonly recoveryMode: boolean;
  readonly headless: boolean;
  readonly projectPath: boolean;
  readonly scene: boolean;
  readonly script: boolean;
  readonly checkOnly: boolean;
  readonly import: boolean;
  readonly quit: boolean;
  readonly quitAfter: boolean;
  readonly lsp: boolean;
  readonly dap: boolean;
  readonly debugServer: boolean;
  readonly buildSolutions: boolean;
  readonly extensionApiDump: boolean;
  readonly extensionApiWithDocsDump: boolean;
  readonly extensionApiValidation: boolean;
  readonly docTool: boolean;
  readonly movieWriting: boolean;
}

export function createEmptyGodotCommandCapabilities(): GodotCommandCapabilities {
  return {
    editor: false,
    projectManager: false,
    recoveryMode: false,
    headless: false,
    projectPath: false,
    scene: false,
    script: false,
    checkOnly: false,
    import: false,
    quit: false,
    quitAfter: false,
    lsp: false,
    dap: false,
    debugServer: false,
    buildSolutions: false,
    extensionApiDump: false,
    extensionApiWithDocsDump: false,
    extensionApiValidation: false,
    docTool: false,
    movieWriting: false,
  };
}

/** Immutable, bounded option set recognized by the help capability parser. */
export const GODOT_KNOWN_OPTIONS: readonly {
  readonly option: string;
  readonly capability: keyof GodotCommandCapabilities;
}[] = [
  { option: "--editor", capability: "editor" },
  { option: "--project-manager", capability: "projectManager" },
  { option: "--recovery-mode", capability: "recoveryMode" },
  { option: "--headless", capability: "headless" },
  { option: "--path", capability: "projectPath" },
  { option: "--scene", capability: "scene" },
  { option: "--script", capability: "script" },
  { option: "--check-only", capability: "checkOnly" },
  { option: "--import", capability: "import" },
  { option: "--quit", capability: "quit" },
  { option: "--quit-after", capability: "quitAfter" },
  { option: "--lsp-port", capability: "lsp" },
  { option: "--dap-port", capability: "dap" },
  { option: "--debug-server", capability: "debugServer" },
  { option: "--build-solutions", capability: "buildSolutions" },
  { option: "--dump-extension-api", capability: "extensionApiDump" },
  { option: "--dump-extension-api-with-docs", capability: "extensionApiWithDocsDump" },
  { option: "--validate-extension-api", capability: "extensionApiValidation" },
  { option: "--doctool", capability: "docTool" },
  { option: "--write-movie", capability: "movieWriting" },
];

/**
 * Option tokens that must never be passed to a Godot probe executable.
 * Fixed Solaris probes pass only `--version`, `--help`, or
 * `--dump-extension-api`; these project-affecting tokens are prohibited in
 * probe invocation code and used by the architecture guardrail.
 */
export const FORBIDDEN_GODOT_PROJECT_ARGUMENTS: readonly string[] = [
  "--path",
  "--upwards",
  "--import",
  "--scene",
  "--script",
];
