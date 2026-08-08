import {
  lstat,
  mkdir,
  mkdtemp,
  open,
  readdir,
  realpath,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import type { Dirent, Stats } from "node:fs";
import type { FileHandle } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createGodotProjectInspector } from "./project-inspector.js";
import { scanProjectFile } from "./project-scanner.js";
import { containsCodeToken } from "./lexical.js";
import { inventoryExecutableContent } from "./content-inventory.js";
import { scanProjectFiles } from "./bounded-scan.js";
import type { GodotProjectFsOps } from "./traversal-limits.js";
import { isWithinPathIdentity, samePathIdentity } from "../../fs-path-identity.js";

const tempDirectories: string[] = [];

async function withWorkspace(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-godot-project-"));
  tempDirectories.push(directory);
  return directory;
}

async function writeFiles(root: string, files: Record<string, string>): Promise<void> {
  for (const [path, content] of Object.entries(files)) {
    const full = join(root, path);
    await mkdir(join(full, ".."), { recursive: true });
    await writeFile(full, content);
  }
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

/** Records every filesystem path passed to the seam's operations. */
function recordingFsOps(touched: string[]): GodotProjectFsOps {
  return {
    lstat: async (path: string): Promise<Stats> => {
      touched.push(path);
      return lstat(path);
    },
    realpath: async (path: string): Promise<string> => {
      touched.push(path);
      return realpath(path);
    },
    readdir: async (path: string, options: { readonly withFileTypes: true }): Promise<Dirent[]> => {
      touched.push(path);
      return readdir(path, options);
    },
    open: async (path: string): Promise<FileHandle> => {
      touched.push(path);
      return open(path, "r");
    },
  };
}

/** Asserts no recorded filesystem path ever points outside the workspace. */
function assertNoOutsideAccess(touched: readonly string[], workspace: string): void {
  const outsideDirectory = join(workspace, "..", "solaris-outside-sentinel");
  for (const path of touched) {
    expect(isWithinPathIdentity(outsideDirectory, path)).toBe(false);
    expect(path.split(/[\\/]/).includes("..")).toBe(false);
  }
}

const GDSCRIPT_PROJECT = [
  "config_version=5",
  "",
  "[application]",
  'config/name="GDScript Game"',
  'config/features=PackedStringArray("4.7", "GL Compatibility")',
  'run/main_scene="res://src/main/Main.tscn"',
  "",
  "[autoload]",
  'GameState="*res://src/autoload/game_state.gd"',
].join("\n");

describe("createGodotProjectInspector", () => {
  it("reports no project in a non-Godot workspace", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "README.md": "not a godot project" });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.detected).toBe(false);
    expect(profile.mainScene).toBeNull();
    expect(profile.languageProfile).toBe("unknown");
  });

  it("detects a GDScript project and extracts metadata", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": GDSCRIPT_PROJECT,
      "src/main/Main.tscn": "[gd_scene format=3]",
      "src/autoload/game_state.gd": "extends Node\nvar score = 0\n",
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.detected).toBe(true);
    expect(profile.name).toBe("GDScript Game");
    expect(profile.configVersion).toBe(5);
    expect(profile.declaredEngineVersion).toEqual({ major: 4, minor: 7, patch: null, raw: "4.7" });
    expect(profile.mainScene).toBe("res://src/main/Main.tscn");
    expect(profile.mainSceneExists).toBe(true);
    expect(profile.languageProfile).toBe("gdscript");
    expect(profile.autoloads.length).toBe(1);
    expect(profile.projectFileSha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it("reports a missing main scene", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": GDSCRIPT_PROJECT,
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.mainSceneExists).toBe(false);
  });

  it("rejects a symbolic-linked project.godot", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "real.godot": "config_version=5\n" });
    try {
      await symlink(join(workspace, "real.godot"), join(workspace, "project.godot"));
    } catch {
      return;
    }
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.detected).toBe(false);
  });

  it("handles an empty project file as minimal", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "project.godot": "" });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.detected).toBe(true);
    expect(profile.configVersion).toBeNull();
    expect(profile.name).toBeNull();
  });

  it("detects a .NET project from root project files and settings", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": ["config_version=5", "[dotnet]", 'project/assembly_name="DotnetGame"'].join(
        "\n",
      ),
      "DotnetGame.csproj": "<Project></Project>",
      "src/Game.cs": "public class Game {}",
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.languageProfile).toBe("dotnet");
    expect(profile.executableContent.dotnetProjectFiles).toEqual(["DotnetGame.csproj"]);
  });

  it("detects a mixed project", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": "config_version=5\n",
      "src/a.gd": "extends Node\n",
      "src/b.cs": "public class B {}",
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.languageProfile).toBe("mixed");
  });

  it("leaves an empty project language-unknown", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "project.godot": "" });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.languageProfile).toBe("unknown");
  });

  it("inventories tool scripts, editor plugins, and GDExtensions statically", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": [
        "config_version=5",
        "[editor_plugins]",
        'enabled=PackedStringArray("res://addons/example")',
      ].join("\n"),
      "src/tool.gd": "@tool\nextends Node\n",
      "src/plain.gd": "extends Node\n# @tool is commented\n",
      "addons/example/plugin.cfg":
        '[plugin]\nname="Example"\ndescription="Example plugin"\nauthor="Solaris"\nversion="1.0"\nscript="example.gd"\n',
      "addons/example/example.gd": "@tool\nextends EditorPlugin\n",
      "addons/importer/plugin.cfg": '[plugin]\nname="Importer"\nscript="importer.gd"\n',
      "addons/importer/importer.gd": "@tool\nextends EditorImportPlugin\n",
      "lib/godot_ext.gdextension":
        '[configuration]\ncompatibility_minimum="4.3"\n[entry]\nWindows.64="bin/godot_ext.dll"\n',
      "lib/bin/godot_ext.dll": "fake native library",
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.executableContent.toolScripts).toEqual([
      "addons/example/example.gd",
      "addons/importer/importer.gd",
      "src/tool.gd",
    ]);
    const example = profile.executableContent.editorPlugins.find(
      (plugin) => plugin.path === "addons/example",
    );
    expect(example?.enabled).toBe(true);
    expect(example?.name).toBe("Example");
    expect(example?.scriptPath).toBe("example.gd");
    const importer = profile.executableContent.editorPlugins.find(
      (plugin) => plugin.path === "addons/importer",
    );
    expect(importer?.enabled).toBe(false);
    expect(importer?.importPluginHeuristic).toBe(true);
    expect(profile.executableContent.importPlugins).toEqual(["addons/importer"]);
    expect(profile.executableContent.gdextensionDescriptors.length).toBe(1);
    const extension = profile.executableContent.gdextensionDescriptors[0];
    expect(extension?.compatibilityMinimum).toBe("4.3");
    expect(extension?.libraryFilesExist).toBe(true);
    expect(extension?.escapesThroughSymlinks).toBe(false);
    expect(profile.executableContent.autoloadCount).toBe(0);
  });

  it("reports scan truncation truthfully", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = { "project.godot": "config_version=5\n" };
    for (let index = 0; index < 60; index += 1) {
      files[`src/gen/${index}.gd`] = "extends Node\n";
    }
    await writeFiles(workspace, files);
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.executableContent.scanTruncated).toBe(false);
  });

  it("returns workspace-relative paths only", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": GDSCRIPT_PROJECT,
      "src/tool.gd": "@tool\n",
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    const allPaths = [
      ...profile.executableContent.toolScripts,
      ...profile.executableContent.dotnetProjectFiles,
      ...profile.executableContent.gdextensionDescriptors.map((entry) => entry.path),
    ];
    for (const path of allPaths) {
      expect(path.startsWith("/")).toBe(false);
      expect(path).not.toMatch(/^[a-zA-Z]:/);
      expect(path).not.toContain("..");
    }
  });

  it("is deterministic across repeated inspection", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "project.godot": GDSCRIPT_PROJECT });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const first = await inspector.inspect();
    const second = await inspector.inspect();
    expect(second).toEqual(first);
  });
});

describe("scanProjectFile integration", () => {
  it("marks results as static and non-authoritative via warnings", () => {
    const result = scanProjectFile(
      "config_version=5\n[application]\nconfig/name=SomeObject(1,2,3)\n",
    );
    expect(result.name).toBeNull();
  });
});

describe("containsCodeToken", () => {
  it("detects @tool in code but not in comments or strings", () => {
    expect(containsCodeToken("@tool\nextends Node\n", "@tool")).toBe(true);
    expect(containsCodeToken("# @tool\n", "@tool")).toBe(false);
    expect(containsCodeToken('var x = "@tool"\n', "@tool")).toBe(false);
    expect(containsCodeToken('print("has @tool inside")\n', "@tool")).toBe(false);
    expect(containsCodeToken("@tool # with comment\n", "@tool")).toBe(true);
  });
});

describe("project path containment", () => {
  it("warns on an escaping main scene without touching the filesystem outside", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": [
        "config_version=5",
        "[application]",
        'run/main_scene="res://../../solaris-outside-sentinel/main.tscn"',
      ].join("\n"),
    });
    const touched: string[] = [];
    const inspector = createGodotProjectInspector({
      workspaceRoot: workspace,
      fsOps: recordingFsOps(touched),
    });
    const profile = await inspector.inspect();
    expect(profile.mainSceneExists).toBe(false);
    expect(
      profile.warnings.some((warning) =>
        warning.message.includes("res://../../solaris-outside-sentinel/main.tscn"),
      ),
    ).toBe(true);
    assertNoOutsideAccess(touched, workspace);
  });

  it("warns on an absolute main scene without constructing a host path", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": [
        "config_version=5",
        "[application]",
        'run/main_scene="C:\\\\outside\\\\main.tscn"',
      ].join("\n"),
    });
    const touched: string[] = [];
    const inspector = createGodotProjectInspector({
      workspaceRoot: workspace,
      fsOps: recordingFsOps(touched),
    });
    const profile = await inspector.inspect();
    expect(
      profile.warnings.some((warning) => warning.message.includes("not a contained project path")),
    ).toBe(true);
    assertNoOutsideAccess(touched, workspace);
  });

  it("skips plugin scripts with repeated ../ and absolute spellings, warning without outside access", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": "config_version=5\n",
      "addons/escape/plugin.cfg":
        '[plugin]\nname="Escape"\nscript="..\\..\\..\\solaris-outside-sentinel\\escape.gd"\n',
      "addons/absolute/plugin.cfg": '[plugin]\nname="Absolute"\nscript="C:\\outside\\evil.gd"\n',
      "addons/unc/plugin.cfg": '[plugin]\nname="Unc"\nscript="\\\\server\\share\\evil.gd"\n',
      "addons/clean/plugin.cfg": '[plugin]\nname="Clean"\nscript="clean.gd"\n',
      "addons/clean/clean.gd": "extends EditorPlugin\n",
    });
    const touched: string[] = [];
    const inspector = createGodotProjectInspector({
      workspaceRoot: workspace,
      fsOps: recordingFsOps(touched),
    });
    const profile = await inspector.inspect();
    expect(profile.executableContent.editorPlugins.map((plugin) => plugin.path)).toEqual([
      "addons/absolute",
      "addons/clean",
      "addons/escape",
      "addons/unc",
    ]);
    expect(profile.warnings.some((warning) => warning.message.includes("addons/escape"))).toBe(
      true,
    );
    expect(profile.warnings.some((warning) => warning.message.includes("addons/absolute"))).toBe(
      true,
    );
    expect(profile.warnings.some((warning) => warning.message.includes("addons/unc"))).toBe(true);
    const resolvedOutside = join(workspace, "..", "solaris-outside-sentinel");
    expect(profile.warnings.some((warning) => warning.message.includes(resolvedOutside))).toBe(
      false,
    );
    assertNoOutsideAccess(touched, workspace);
  });

  it("warns on GDExtension outside targets without touching them", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": "config_version=5\n",
      "lib/escape.gdextension":
        '[configuration]\ncompatibility_minimum="4.3"\n[entry]\nWindows.64="..\\..\\solaris-outside-sentinel\\evil.dll"\n',
      "lib/absolute.gdextension":
        '[entry]\nWindows.64="C:\\outside\\evil.dll"\nLinux.64="\\\\server\\share\\evil.dll"\n',
      "lib/ok.gdextension": '[entry]\nWindows.64="bin/godot_ext.dll"\n',
      "lib/bin/godot_ext.dll": "fake native library",
    });
    const touched: string[] = [];
    const inspector = createGodotProjectInspector({
      workspaceRoot: workspace,
      fsOps: recordingFsOps(touched),
    });
    const profile = await inspector.inspect();
    const escape = profile.executableContent.gdextensionDescriptors.find(
      (descriptor) => descriptor.path === "lib/escape.gdextension",
    );
    expect(escape?.escapesThroughSymlinks).toBe(true);
    expect(escape?.libraryFilesExist).toBe(false);
    const absolute = profile.executableContent.gdextensionDescriptors.find(
      (descriptor) => descriptor.path === "lib/absolute.gdextension",
    );
    expect(absolute?.escapesThroughSymlinks).toBe(true);
    const ok = profile.executableContent.gdextensionDescriptors.find(
      (descriptor) => descriptor.path === "lib/ok.gdextension",
    );
    expect(ok?.libraryFilesExist).toBe(true);
    expect(ok?.escapesThroughSymlinks).toBe(false);
    expect(profile.warnings.some((warning) => warning.message.includes("evil.dll"))).toBe(true);
    const resolvedOutside = join(workspace, "..", "solaris-outside-sentinel");
    expect(profile.warnings.some((warning) => warning.message.includes(resolvedOutside))).toBe(
      false,
    );
    assertNoOutsideAccess(touched, workspace);
  });

  it("skips a symlinked addon directory with a warning", async () => {
    const workspace = await withWorkspace();
    const outside = await withWorkspace();
    await writeFile(join(outside, "plugin.cfg"), '[plugin]\nscript="x.gd"\n');
    try {
      await symlink(outside, join(workspace, "addons", "linked"), "junction");
    } catch {
      return;
    }
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.executableContent.editorPlugins).toEqual([]);
    expect(
      profile.warnings.some((warning) =>
        warning.message.includes("addons/linked is a symbolic link"),
      ),
    ).toBe(true);
  });

  it("skips a symlinked plugin.cfg with a warning", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "real.cfg": '[plugin]\nscript="x.gd"\n' });
    await mkdir(join(workspace, "addons", "linked"), { recursive: true });
    try {
      await symlink(join(workspace, "real.cfg"), join(workspace, "addons", "linked", "plugin.cfg"));
    } catch {
      return;
    }
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.executableContent.editorPlugins).toEqual([]);
    expect(profile.warnings.some((warning) => warning.message.includes("not a regular file"))).toBe(
      true,
    );
  });

  it("skips a symlinked gdextension descriptor", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "real.gdextension": '[entry]\nWindows.64="x.dll"\n' });
    try {
      await symlink(join(workspace, "real.gdextension"), join(workspace, "lib.gdextension"));
    } catch {
      return;
    }
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const profile = await inspector.inspect();
    expect(profile.executableContent.gdextensionDescriptors).toEqual([]);
  });

  it("treats a tool script swapped during inspection as a warning and skips it", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": "config_version=5\n",
      "src/tool.gd": "@tool\nextends Node\n",
    });
    const targetFile = join(workspace, "src", "tool.gd");
    const outsideSentinel = join(workspace, "..", "solaris-swap-sentinel.gd");
    const base = recordingFsOps([]);
    const fsOps: GodotProjectFsOps = {
      ...base,
      realpath: async (path: string): Promise<string> => {
        if (samePathIdentity(path, targetFile)) {
          return outsideSentinel;
        }
        return base.realpath(path);
      },
    };
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace, fsOps });
    const profile = await inspector.inspect();
    expect(profile.executableContent.toolScripts).toEqual([]);
    expect(
      profile.warnings.some((warning) => warning.message.includes("changed during inspection")),
    ).toBe(true);
  });
});

describe("inventoryExecutableContent bounds", () => {
  it("exhausts the entry budget deterministically on a wide directory", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = {};
    for (let index = 0; index < 30; index += 1) {
      files[`gen/${index}.txt`] = "x";
    }
    await writeFiles(workspace, files);
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxEntries: 10,
    });
    expect(result.inventory.scanTruncated).toBe(true);
    expect(result.inventory.scanTruncationReason).toBe("entry-limit");
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectEntriesExamined")),
    ).toBe(true);
  });

  it("consumes the directory budget on deep empty trees", async () => {
    const workspace = await withWorkspace();
    const nested = ["d"];
    for (let index = 0; index < 12; index += 1) {
      nested.push(`d${index}`);
    }
    await mkdir(join(workspace, nested.join("/")), { recursive: true });
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxDirectories: 5,
    });
    expect(result.inventory.scanTruncationReason).toBe("directory-limit");
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectDirectoriesVisited")),
    ).toBe(true);
  });

  it("hits the plugin-directory bound on huge addon fanout", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = {};
    for (let index = 0; index < 10; index += 1) {
      files[`addons/p${index}/plugin.cfg`] = `[plugin]\nname="P${index}"\nscript="p${index}.gd"\n`;
      files[`addons/p${index}/p${index}.gd`] = "extends EditorPlugin\n";
    }
    await writeFiles(workspace, files);
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxPluginDirectories: 3,
    });
    expect(result.inventory.scanTruncationReason).toBe("plugin-limit");
    expect(result.inventory.editorPlugins).toHaveLength(3);
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectPluginDirectories")),
    ).toBe(true);
  });

  it("reports the file-limit truncation reason exactly", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = {};
    for (let index = 0; index < 8; index += 1) {
      files[`f${index}.gd`] = "extends Node\n";
    }
    await writeFiles(workspace, files);
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxFiles: 3,
    });
    expect(result.inventory.scanTruncationReason).toBe("file-limit");
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectFilesScanned")),
    ).toBe(true);
  });

  it("reports the surfaced-limit truncation reason exactly", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = {};
    for (let index = 0; index < 5; index += 1) {
      files[`f${index}.gd`] = "extends Node\n";
    }
    await writeFiles(workspace, files);
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxSurfaced: 2,
    });
    expect(result.inventory.scanTruncationReason).toBe("surfaced-limit");
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectFilesSurfaced")),
    ).toBe(true);
  });

  it("reports the inventory-item bound truncation reason exactly", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "a.gd": "@tool\n",
      "b.gd": "@tool\n",
      "c.gd": "@tool\n",
    });
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxInventoryItems: 2,
    });
    expect(result.inventory.scanTruncationReason).toBe("inventory-limit");
    expect(result.inventory.toolScripts).toHaveLength(2);
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectInventoryItems")),
    ).toBe(true);
  });

  it("reports the descriptor bound truncation reason exactly", async () => {
    const workspace = await withWorkspace();
    const files: Record<string, string> = {};
    for (let index = 0; index < 3; index += 1) {
      files[`addons/p${index}/plugin.cfg`] = `[plugin]\nname="P${index}"\nscript="p${index}.gd"\n`;
      files[`addons/p${index}/p${index}.gd`] = "extends EditorPlugin\n";
    }
    await writeFiles(workspace, files);
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxDescriptorsParsed: 1,
    });
    expect(result.inventory.scanTruncationReason).toBe("descriptor-limit");
    expect(result.inventory.editorPlugins).toHaveLength(1);
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectDescriptorsParsed")),
    ).toBe(true);
  });

  it("counts raw file bytes for the source byte budget on multibyte content", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "src/unicode.gd": `@tool\n${"é".repeat(4)}\n`,
    });
    // Raw bytes are 6 + 8 + 1 = 15; decoded characters are only 10.
    const exhausted = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxSourceBytesInspected: 10,
    });
    expect(exhausted.inventory.scanTruncationReason).toBe("bytes-limit");
    expect(exhausted.inventory.toolScripts).toEqual([]);
    expect(
      exhausted.warnings.some((warning) => warning.message.includes("maxSourceBytesInspected")),
    ).toBe(true);
    const complete = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
      maxSourceBytesInspected: 20,
    });
    expect(complete.inventory.scanTruncated).toBe(false);
    expect(complete.inventory.scanTruncationReason).toBe("none");
    expect(complete.inventory.toolScripts).toEqual(["src/unicode.gd"]);
  });

  it("skips an oversized plugin descriptor with a warning", async () => {
    const workspace = await withWorkspace();
    const big = "x".repeat(300 * 1024);
    await writeFiles(workspace, {
      "addons/big/plugin.cfg": `[plugin]\nname="Big"\ndescription="${big}"\n`,
    });
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
    });
    expect(result.inventory.editorPlugins).toEqual([]);
    expect(
      result.warnings.some((warning) => warning.message.includes("exceeds the size limit")),
    ).toBe(true);
  });

  it("skips an oversized GDExtension descriptor with a warning", async () => {
    const workspace = await withWorkspace();
    const big = "x".repeat(1100 * 1024);
    await writeFiles(workspace, {
      "lib/big.gdextension": `[entry]\nWindows.64="${big}"\n`,
    });
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
    });
    expect(result.inventory.gdextensionDescriptors).toEqual([]);
    expect(
      result.warnings.some((warning) => warning.message.includes("exceeds the size limit")),
    ).toBe(true);
  });

  it("skips a plugin descriptor with an over-long value with a warning", async () => {
    const workspace = await withWorkspace();
    const long = "y".repeat(20 * 1024);
    await writeFiles(workspace, {
      "addons/long/plugin.cfg": `[plugin]\nname="Long"\ndescription="${long}"\n`,
    });
    const result = await inventoryExecutableContent({
      workspaceRoot: workspace,
      enabledPlugins: [],
      autoloadCount: 0,
    });
    expect(result.inventory.editorPlugins).toEqual([]);
    expect(
      result.warnings.some((warning) => warning.message.includes("maxProjectDescriptorValueBytes")),
    ).toBe(true);
  });
});

describe("scanProjectFiles bounds", () => {
  it("stops at the shared deadline with the timeout truncation reason", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "a.gd": "extends Node\n" });
    const result = await scanProjectFiles({ workspaceRoot: workspace, timeoutMs: -1 });
    expect(result.truncated).toBe(true);
    expect(result.truncationReason).toBe("timeout");
    expect(
      result.warnings.some((warning) => warning.message.includes("staticProjectScanTimeoutMs")),
    ).toBe(true);
  });

  it("throws AbortError when the scan is cancelled mid-walk", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "a.gd": "extends Node\n" });
    const controller = new AbortController();
    controller.abort();
    await expect(
      scanProjectFiles({ workspaceRoot: workspace, signal: controller.signal }),
    ).rejects.toMatchObject({ name: "AbortError" });
  });

  it("throws AbortError when the inventory is cancelled", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, { "a.gd": "extends Node\n" });
    const controller = new AbortController();
    controller.abort();
    await expect(
      inventoryExecutableContent({
        workspaceRoot: workspace,
        enabledPlugins: [],
        autoloadCount: 0,
        signal: controller.signal,
      }),
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});

describe("project inspection cache invalidation", () => {
  it("reflects create, edit, and delete cycles across in-session inspections", async () => {
    const workspace = await withWorkspace();
    await writeFiles(workspace, {
      "project.godot": 'config_version=5\n[application]\nconfig/name="V1"\n',
    });
    const inspector = createGodotProjectInspector({ workspaceRoot: workspace });
    const first = await inspector.inspect();
    expect(first.detected).toBe(true);
    expect(first.name).toBe("V1");
    const second = await inspector.inspect();
    expect(second).toBe(first);
    await writeFiles(workspace, {
      "project.godot": 'config_version=5\n[application]\nconfig/name="V2"\n',
    });
    const third = await inspector.inspect();
    expect(third.detected).toBe(true);
    expect(third.name).toBe("V2");
    await rm(join(workspace, "project.godot"));
    const fourth = await inspector.inspect();
    expect(fourth.detected).toBe(false);
    const stillMissing = await inspector.inspect();
    expect(stillMissing).toBe(fourth);
    await writeFiles(workspace, {
      "project.godot": 'config_version=5\n[application]\nconfig/name="V3"\n',
    });
    const fifth = await inspector.inspect();
    expect(fifth.detected).toBe(true);
    expect(fifth.name).toBe("V3");
    const unchanged = await inspector.inspect();
    expect(unchanged).toBe(fifth);
  });
});
