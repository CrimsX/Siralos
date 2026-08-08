import { describe, expect, it } from "vitest";
import { parseQuotedString, scanProjectFile } from "./project-scanner.js";

const GODOT_4_PROJECT = [
  "; Engine configuration file.",
  "config_version=5",
  "",
  "[application]",
  "",
  'config/name="Counter-Strafe Manager"',
  'config/version="0.1.0"',
  'config/features=PackedStringArray("4.7", "GL Compatibility")',
  'run/main_scene="res://src/main/Main.tscn"',
  "",
  "[rendering]",
  'renderer/rendering_method="forward_plus"',
  'renderer/rendering_method.mobile="gl_compatibility"',
  "",
  "[dotnet]",
  'project/assembly_name="CounterStrafe"',
  "",
  "[editor_plugins]",
  'enabled=PackedStringArray("res://addons/example", "res://addons/other")',
  "",
  "[autoload]",
  'GameState="*res://src/autoload/game_state.gd"',
  'Events="res://src/autoload/events.gd"',
].join("\n");

describe("scanProjectFile", () => {
  it("parses config version, name, features, main scene, and rendering", () => {
    const result = scanProjectFile(GODOT_4_PROJECT);
    expect(result.configVersion).toBe(5);
    expect(result.name).toBe("Counter-Strafe Manager");
    expect(result.applicationVersion).toBe("0.1.0");
    expect(result.declaredFeatures).toEqual(["4.7", "GL Compatibility"]);
    expect(result.mainScene).toBe("res://src/main/Main.tscn");
    expect(result.renderingMethods).toEqual(["forward_plus", "gl_compatibility"]);
  });

  it("warns on autoload targets that are not contained project paths", () => {
    const result = scanProjectFile(
      [
        "[autoload]",
        'Good="*res://src/autoload/game_state.gd"',
        'Escape="*res://../../outside.gd"',
        'Absolute="C:\\\\outside\\\\evil.gd"',
        'Unc="\\\\\\\\server\\\\share\\\\evil.gd"',
      ].join("\n"),
    );
    expect(result.autoloads).toHaveLength(4);
    expect(result.warnings.some((warning) => warning.message.includes("Escape"))).toBe(true);
    expect(result.warnings.some((warning) => warning.message.includes("Absolute"))).toBe(true);
    expect(result.warnings.some((warning) => warning.message.includes("Unc"))).toBe(true);
    expect(result.warnings.some((warning) => warning.message.includes("Good"))).toBe(false);
  });

  it("warns on enabled plugin entries that are not contained", () => {
    const result = scanProjectFile(
      '[editor_plugins]\nenabled=PackedStringArray("res://addons/ok", "res://../../evil", "C:\\\\evil")\n',
    );
    expect(result.enabledPlugins).toEqual(["res://addons/ok"]);
    expect(result.warnings.some((warning) => warning.message.includes("res://../../evil"))).toBe(
      true,
    );
    expect(result.warnings.some((warning) => warning.message.includes("C:\\evil"))).toBe(true);
  });

  it("bounds autoload declarations", () => {
    const lines = ["[autoload]"];
    for (let index = 0; index < 300; index += 1) {
      lines.push(`"Auto${index}"="res://src/a.gd"`);
    }
    const result = scanProjectFile(lines.join("\n"));
    expect(result.autoloads).toHaveLength(256);
    expect(result.warnings.some((warning) => warning.message.includes("maxProjectAutoloads"))).toBe(
      true,
    );
  });

  it("parses dotnet settings", () => {
    const result = scanProjectFile(GODOT_4_PROJECT);
    expect(result.dotnetAssemblyName).toBe("CounterStrafe");
  });

  it("parses enabled plugins", () => {
    const result = scanProjectFile(GODOT_4_PROJECT);
    expect(result.enabledPlugins).toEqual(["res://addons/example", "res://addons/other"]);
  });

  it("parses autoloads with singleton markers", () => {
    const result = scanProjectFile(GODOT_4_PROJECT);
    expect(result.autoloads).toEqual([
      { name: "GameState", target: "*res://src/autoload/game_state.gd", isSingleton: true },
      { name: "Events", target: "res://src/autoload/events.gd", isSingleton: false },
    ]);
  });

  it("ignores comments", () => {
    const result = scanProjectFile("; comment\n# other comment\nconfig_version=5\n");
    expect(result.configVersion).toBe(5);
    expect(result.warnings.length).toBe(0);
  });

  it("parses escaped quoted strings", () => {
    const result = scanProjectFile('[application]\nconfig/name="a\\"b\\\\c\\n"\n');
    expect(result.name).toBe('a"b\\c\n');
  });

  it("preserves unknown values as raw with a warning", () => {
    const result = scanProjectFile("[application]\nconfig/name=SomeObject(1, 2)\n");
    expect(result.name).toBeNull();
    expect(result.warnings.some((warning) => warning.message.includes("unsupported form"))).toBe(
      true,
    );
  });

  it("does not reject the project for one unsupported value", () => {
    const result = scanProjectFile(
      'config_version=5\n[application]\nconfig/name="Fine"\nrun/main_scene=SomeObject()\n',
    );
    expect(result.configVersion).toBe(5);
    expect(result.name).toBe("Fine");
  });

  it("handles quoted keys", () => {
    const result = scanProjectFile('[application]\n"config/name"="Quoted Key"\n');
    expect(result.name).toBe("Quoted Key");
  });

  it("records source line numbers", () => {
    const result = scanProjectFile('\n\n[application]\nconfig/name="x"\n');
    const property = result.properties.find((entry) => entry.key === "config/name");
    expect(property?.lineNumber).toBe(4);
  });

  it("is deterministic", () => {
    expect(scanProjectFile(GODOT_4_PROJECT)).toEqual(scanProjectFile(GODOT_4_PROJECT));
  });

  it("handles an empty project file as a minimal project with missing metadata", () => {
    const result = scanProjectFile("");
    expect(result.configVersion).toBeNull();
    expect(result.name).toBeNull();
    expect(result.properties).toEqual([]);
  });

  it("bounds lines and reports truncation", () => {
    const longLine = "a".repeat(70 * 1024);
    const result = scanProjectFile(`config_version=5\n${longLine}\n`);
    expect(result.truncated).toBe(true);
  });

  it("does not evaluate expressions", () => {
    const result = scanProjectFile('[application]\nconfig/name="hello".to_upper()\n');
    expect(result.name).toBeNull();
  });
});

describe("parseQuotedString", () => {
  it("parses simple strings", () => {
    expect(parseQuotedString('"hello"')).toEqual({ ok: true, value: "hello", consumed: 7 });
  });

  it("parses escapes", () => {
    expect(parseQuotedString('"a\\nb\\t\\"c\\\\d"')).toEqual({
      ok: true,
      value: 'a\nb\t"c\\d',
      consumed: 14,
    });
  });

  it("rejects unterminated strings", () => {
    expect(parseQuotedString('"unterminated')).toEqual({ ok: false });
  });

  it("rejects trailing content", () => {
    expect(parseQuotedString('"a" extra')).toEqual({ ok: false });
  });
});
