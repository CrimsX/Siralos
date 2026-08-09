import { describe, expect, it } from "vitest";
import {
  buildWorkspaceSummary,
  computeWorkspaceRevisionHandle,
  createWorkspaceRevisionRegistry,
  extractGDScriptStructure,
} from "../index.js";

/** Deterministic 64-hex test identity (the registry treats SHA-256 as an
 * opaque string; only the format matters for these fixtures). */
/** Deterministic 64-hex test identity (the registry treats SHA-256 as an
 * opaque string; only the format matters for these fixtures). */
function sha256(text: string): string {
  let hash = 0;
  for (const character of text) {
    hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  }
  const hex: string = hash.toString(16);
  return hex.padStart(64, "0");
}

describe("workspace revision registry", () => {
  it("issues opaque handles backed by the identity tuple", () => {
    const registry = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-A" });
    const handle = registry.issue("player.gd", "a".repeat(64));
    expect(handle).toMatch(/^rev_[0-9a-f]{32}$/);
    const identity = registry.resolve(handle);
    expect(identity).toEqual({
      workspaceFingerprint: "ws-A",
      path: "player.gd",
      sha256: "a".repeat(64),
    });
  });

  it("deduplicates identical states and distinguishes changed states", () => {
    const registry = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-A" });
    const first = registry.issue("player.gd", "a".repeat(64));
    expect(registry.issue("player.gd", "a".repeat(64))).toBe(first);
    const second = registry.issue("player.gd", "b".repeat(64));
    expect(second).not.toBe(first);
    expect(registry.currentRevision("player.gd")).toBe(second);
  });

  it("never resolves across workspaces", () => {
    const registryA = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-A" });
    const registryB = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-B" });
    const handle = registryA.issue("player.gd", "a".repeat(64));
    expect(registryB.resolve(handle)).toBeNull();
    expect(computeWorkspaceRevisionHandle("ws-A", "player.gd", "a".repeat(64))).not.toBe(
      computeWorkspaceRevisionHandle("ws-B", "player.gd", "a".repeat(64)),
    );
  });

  it("does not self-evict with a limit of one entry", () => {
    const registry = createWorkspaceRevisionRegistry({
      workspaceFingerprint: "ws-A",
      maxEntries: 1,
    });
    const handle = registry.issue("player.gd", "a".repeat(64));
    expect(registry.resolve(handle)).not.toBeNull();
    const second = registry.issue("health.gd", "b".repeat(64));
    expect(registry.resolve(second)).not.toBeNull();
    expect(registry.resolve(handle)).toBeNull(); // evicted after overflow
  });

  it("invalidates the current binding while keeping the identity historical", () => {
    const registry = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-A" });
    const handle = registry.issue("player.gd", "a".repeat(64));
    registry.invalidatePath("player.gd");
    expect(registry.currentRevision("player.gd")).toBeNull();
    expect(registry.resolve(handle)).not.toBeNull(); // historical evidence
  });

  it("tracks observed reads with a bounded session-local window", () => {
    const registry = createWorkspaceRevisionRegistry({ workspaceFingerprint: "ws-A" });
    const handle = registry.issue("player.gd", "a".repeat(64));
    registry.observeRead("player.gd", handle, "exact");
    const reads = registry.observedReads();
    expect(reads).toHaveLength(1);
    expect(reads[0]).toMatchObject({ path: "player.gd", revision: handle, mode: "exact" });
  });
});

describe("GDScript structural extraction", () => {
  it("extracts representative Godot 4.x declarations", () => {
    const source = `@tool
extends CharacterBody2D
class_name PlayerController

signal health_changed(old_value: int, new_value: int)
signal died

enum State { IDLE, RUNNING }
enum Direction {
	LEFT,
	RIGHT,
}

const MAX_HEALTH: int = 100
const BASE_SPEED = 200.0

@export var speed: float = 300.0
@onready var sprite: Sprite2D = $Sprite2D
var velocity: Vector2

static func create() -> PlayerController:
	return PlayerController.new()

func _physics_process(delta: float) -> void:
	velocity = get_velocity()
	move_and_slide()

func take_damage(
	amount: int,
	from: Node2D,
) -> void:
	pass
`;
    const structure = extractGDScriptStructure(source, "player.gd");
    expect(structure.status).toBe("complete");
    expect(structure.extendsType).toBe("CharacterBody2D");
    expect(structure.className).toBe("PlayerController");
    expect(structure.fileAnnotations.map((annotation) => annotation.name)).toEqual(["tool"]);
    expect(structure.signals.map((signal) => signal.name)).toEqual(["health_changed", "died"]);
    expect(structure.signals[0]?.parameters).toEqual([
      { name: "old_value", type: "int" },
      { name: "new_value", type: "int" },
    ]);
    expect(structure.enums.map((entry) => entry.name)).toEqual(["State", "Direction"]);
    expect(structure.enums[1]?.members).toEqual(["LEFT", "RIGHT"]);
    expect(structure.enums[1]?.multiline).toBe(true);
    expect(structure.constants).toEqual([
      { name: "MAX_HEALTH", type: "int", line: 14, multiline: false },
      { name: "BASE_SPEED", type: null, line: 15, multiline: false },
    ]);
    expect(structure.properties.map((property) => property.name)).toEqual([
      "speed",
      "sprite",
      "velocity",
    ]);
    expect(structure.properties[0]?.annotations).toEqual(["export"]);
    expect(structure.properties[0]?.type).toBe("float");
    expect(structure.properties[1]?.annotations).toEqual(["onready"]);
    expect(structure.functions.map((fn) => fn.name)).toEqual([
      "create",
      "_physics_process",
      "take_damage",
    ]);
    expect(structure.functions[0]?.isStatic).toBe(true);
    expect(structure.functions[0]?.returnType).toBe("PlayerController");
    expect(structure.functions[2]?.parameters).toEqual([
      { name: "amount", type: "int" },
      { name: "from", type: "Node2D" },
    ]);
    expect(structure.functions[2]?.multilineSignature).toBe(true);
    expect(structure.functions[2]?.returnType).toBe("void");
  });

  it("ignores keywords inside comments and strings", () => {
    const source = `# func fake_function(): this is a comment
extends Node
var description = "var not_a_property and func not_a_func"
var real_property = 1
func real_function():
	# var inner_fake = true
	var inner_real = 2
	return inner_real
`;
    const structure = extractGDScriptStructure(source, "tricky.gd");
    expect(structure.status).toBe("complete");
    expect(structure.functions.map((fn) => fn.name)).toEqual(["real_function"]);
    expect(structure.properties.map((property) => property.name)).toEqual([
      "description",
      "real_property",
    ]);
    // The indented var inside the function body is not a top-level property.
    expect(structure.properties.some((property) => property.name === "inner_real")).toBe(false);
  });

  it("returns a partial result with parser errors for invalid files", () => {
    const source = [
      "extends Node",
      'var x = """',
      "unterminated multiline string",
      "func never_parsed():",
      "\tpass",
      "",
    ].join("\n");
    const structure = extractGDScriptStructure(source, "broken.gd");
    expect(structure.status).toBe("partial");
    expect(structure.parserErrors.length).toBeGreaterThan(0);
    // No fabricated declarations from inside the unparseable string region.
    expect(structure.functions.some((fn) => fn.name === "never_parsed")).toBe(false);
  });

  it("handles single-quoted multiline strings without fake declarations", () => {
    const source = `extends Node
var doc = '''
func not_a_function():
	pass
'''
var real = 1
`;
    const structure = extractGDScriptStructure(source, "quoted.gd");
    expect(structure.status).toBe("complete");
    expect(structure.functions).toHaveLength(0);
    expect(structure.properties.map((property) => property.name)).toEqual(["doc", "real"]);
  });

  it("deduplicates repeated preload/load references (const and standalone forms)", () => {
    const source = `extends Node
const A = preload("res://scenes/enemy.tscn")
const B = preload("res://scenes/enemy.tscn")
preload("res://scenes/enemy.tscn")
const C = "res://scenes/enemy.tscn"
`;
    const structure = extractGDScriptStructure(source, "deps.gd");
    expect(structure.dependencies).toEqual(["res://scenes/enemy.tscn"]);
  });

  it("handles multiline values and array types", () => {
    const source = `extends Node
const LEVELS = [
	1,
	2,
	3,
]
var map: Dictionary[String, int] = {}
@export var items: Array[String] = ["a", "b"]
`;
    const structure = extractGDScriptStructure(source, "data.gd");
    expect(structure.status).toBe("complete");
    expect(structure.constants[0]).toMatchObject({ name: "LEVELS", multiline: true });
    expect(structure.properties[0]?.type).toBe("Dictionary[String,int]");
    expect(structure.properties[1]?.type).toBe("Array[String]");
    expect(structure.properties[1]?.annotations).toEqual(["export"]);
  });

  it("extracts preload/load dependencies", () => {
    const source = `extends Node
const EnemyScene = preload("res://scenes/enemy.tscn")
var texture = load("res://assets/icon.svg")
`;
    const structure = extractGDScriptStructure(source, "loader.gd");
    expect(structure.dependencies).toEqual(["res://scenes/enemy.tscn", "res://assets/icon.svg"]);
  });

  it("caps declarations deterministically and reports truncation", () => {
    const source = Array.from(
      { length: 300 },
      (_, index) => `var property_${index} = ${index}`,
    ).join("\n");
    const structure = extractGDScriptStructure(source, "big.gd");
    expect(structure.truncated).toBe(true);
    expect(structure.properties.length).toBeLessThanOrEqual(256);
  });
});

describe("workspace summary", () => {
  it("is advisory, bounded, and carries the revision", () => {
    const structure = extractGDScriptStructure(
      `extends CharacterBody2D
class_name PlayerController
signal died
@export var speed: float = 300.0
var health: int = 100
func _physics_process(delta: float) -> void:
	pass
func take_damage(amount: int) -> void:
	pass
`,
      "player.gd",
    );
    const summary = buildWorkspaceSummary(structure, "rev_ab12cd34ef56ab12cd34ef56ab12cd34ef");
    expect(summary.mode).toBe("summary");
    expect(summary.advisory).toBe(true);
    expect(summary.text).toContain("@ rev_ab12cd34ef56ab12cd34ef56ab12cd34ef");
    expect(summary.text).toContain("extends CharacterBody2D");
    expect(summary.text).toContain("class_name PlayerController");
    expect(summary.text).toContain("2 functions");
    expect(summary.text).toContain("not authoritative source");
    expect(summary.bytes).toBeLessThan(1024);
    expect(summary.truncated).toBe(false);
  });

  it("always fits the advisory footer even for very small budgets", () => {
    const structure = extractGDScriptStructure("extends Node\nvar x = 1\n", "tiny.gd");
    const summary = buildWorkspaceSummary(structure, null, { maxBytes: 40 });
    expect(summary.text).toContain("not authoritative source");
    expect(summary.text).toContain("[summary truncated]");
  });

  it("never grows beyond its byte budget", () => {
    const structure = extractGDScriptStructure(
      Array.from(
        { length: 200 },
        (_, index) => `func function_${index}(a: int, b: int, c: int) -> void:\n\tpass`,
      ).join("\n"),
      "big.gd",
    );
    const summary = buildWorkspaceSummary(structure, null, { maxBytes: 128 });
    expect(summary.bytes).toBeLessThanOrEqual(128);
    expect(summary.truncated).toBe(true);
    expect(summary.text).toContain("[summary truncated]");
  });
});

describe("revision handle determinism", () => {
  it("the same identity tuple always produces the same handle", () => {
    expect(computeWorkspaceRevisionHandle("ws-A", "player.gd", sha256("x"))).toBe(
      computeWorkspaceRevisionHandle("ws-A", "player.gd", sha256("x")),
    );
  });
});
