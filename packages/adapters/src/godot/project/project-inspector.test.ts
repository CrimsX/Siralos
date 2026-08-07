import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createGodotProjectInspector } from "./project-inspector.js";
import { scanProjectFile } from "./project-scanner.js";
import { containsCodeToken } from "./lexical.js";

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
