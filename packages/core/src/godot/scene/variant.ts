import { GODOT_SCENE_LIMITS } from "./limits.js";
import type { GodotVariantValue } from "./models.js";
import { scanBalanced } from "./text.js";

/**
 * Conservative Godot Variant value parser (Stage 3 milestone 8).
 *
 * Supports the value forms needed to understand common structural
 * relationships: null, booleans, integers, floats, strings, StringName,
 * NodePath, arrays, dictionaries, bounded vector/color values, packed
 * arrays, and the Godot resource reference forms (`ExtResource(...)`,
 * `SubResource(...)`, `Resource(...)`). Unknown valid syntax is preserved
 * as a bounded opaque/raw representation instead of rejected, so the
 * surrounding resource can still be inspected safely.
 *
 * This is NOT a Variant runtime: expressions are never evaluated, and
 * nested structures are bounded (depth, items, components, raw length).
 */

export interface VariantParseResult {
  readonly value: GodotVariantValue;
  /** True when a bound (depth/items/raw length) stopped full interpretation. */
  readonly truncated: boolean;
}

const NUMBER_PATTERN =
  /^[+-]?(?:[0-9]+\.[0-9]*(?:[eE][+-]?[0-9]+)?|\.[0-9]+(?:[eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+)$/;
const INTEGER_PATTERN = /^[+-]?[0-9]+$/;

/** Parse one Godot Variant value from its exact raw text. */
export function parseGodotVariant(raw: string): VariantParseResult {
  return parseVariant(raw.trim(), 0);
}

function parseVariant(text: string, depth: number): VariantParseResult {
  if (text.length === 0) {
    return {
      value: { kind: "opaque", typeName: "unknown", raw: boundedRaw(text) },
      truncated: false,
    };
  }
  if (depth > GODOT_SCENE_LIMITS.maxVariantDepth) {
    return {
      value: { kind: "opaque", typeName: "unknown", raw: boundedRaw(text) },
      truncated: true,
    };
  }
  if (text === "null") {
    return { value: { kind: "null" }, truncated: false };
  }
  if (text === "true" || text === "false") {
    return { value: { kind: "boolean", value: text === "true" }, truncated: false };
  }
  if (INTEGER_PATTERN.test(text)) {
    const numeric = Number(text);
    return Number.isSafeInteger(numeric)
      ? { value: { kind: "integer", value: numeric }, truncated: false }
      : { value: { kind: "opaque", typeName: "unknown", raw: boundedRaw(text) }, truncated: false };
  }
  if (NUMBER_PATTERN.test(text)) {
    const numeric = Number(text);
    return Number.isFinite(numeric)
      ? { value: { kind: "float", value: numeric }, truncated: false }
      : { value: { kind: "opaque", typeName: "unknown", raw: boundedRaw(text) }, truncated: false };
  }
  if (text.startsWith('"')) {
    const parsed = parseQuotedString(text);
    if (parsed.ok) {
      return { value: { kind: "string", value: parsed.value }, truncated: false };
    }
    return {
      value: { kind: "opaque", typeName: "string", raw: boundedRaw(text) },
      truncated: false,
    };
  }
  if (text.startsWith("&")) {
    const rest = text.slice(1).trim();
    if (rest.startsWith('"')) {
      const parsed = parseQuotedString(rest);
      if (parsed.ok) {
        return { value: { kind: "string_name", value: parsed.value }, truncated: false };
      }
    }
    return {
      value: { kind: "opaque", typeName: "StringName", raw: boundedRaw(text) },
      truncated: false,
    };
  }
  if (text.startsWith("[") && text.endsWith("]")) {
    return parseArray(text.slice(1, -1), depth + 1);
  }
  if (text.startsWith("{") && text.endsWith("}")) {
    return parseDictionary(text.slice(1, -1), depth + 1);
  }
  // Constructor forms: TypeName(arg, arg, ...)
  const constructorMatch = /^([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(text);
  if (constructorMatch !== null) {
    return parseConstructor(text, constructorMatch[1] as string, depth);
  }
  // Fall back to a raw bounded representation of anything else (e.g.
  // `Path2D` bare names, unquoted tokens, or future syntax).
  return {
    value: { kind: "opaque", typeName: "unknown", raw: boundedRaw(text) },
    truncated: false,
  };
}

function parseConstructor(text: string, typeName: string, depth: number): VariantParseResult {
  const openIndex = text.indexOf("(");
  const scan = scanBalanced(text, openIndex);
  if (!scan.balanced || scan.endIndex !== text.length) {
    return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
  }
  const inner = text.slice(openIndex + 1, -1).trim();
  const args = splitTopLevelArguments(inner);

  if (typeName === "ExtResource" || typeName === "SubResource") {
    const id = args.length > 0 ? unquote(args[0]) : null;
    if (id === null || id.length === 0) {
      return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
    }
    return {
      value:
        typeName === "ExtResource" ? { kind: "ext_resource", id } : { kind: "sub_resource", id },
      truncated: false,
    };
  }
  if (typeName === "NodePath") {
    const path = args.length > 0 ? unquote(args[0]) : null;
    return path === null
      ? { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false }
      : { value: { kind: "node_path", value: path }, truncated: false };
  }
  if (typeName === "Resource") {
    const uid = args.length > 0 ? unquote(args[0]) : null;
    const second = args.length > 1 ? unquote(args[1]) : null;
    if (uid === null) {
      return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
    }
    if (uid.startsWith("uid://")) {
      return {
        value: {
          kind: "resource",
          ...(second !== null && second.length > 0 ? { type: second } : {}),
          uid,
        },
        truncated: false,
      };
    }
    return {
      value: {
        kind: "resource",
        ...(second !== null && second.length > 0 ? { type: second } : {}),
        path: uid,
      },
      truncated: false,
    };
  }
  if (typeName === "Color") {
    const components = parseNumberComponents(args, 8);
    if (components === null) {
      return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
    }
    return { value: { kind: "color", components }, truncated: false };
  }
  if (isVectorType(typeName)) {
    const components = parseNumberComponents(args, GODOT_SCENE_LIMITS.maxVectorComponents);
    if (components === null) {
      return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
    }
    return { value: { kind: "vector", typeName, components }, truncated: false };
  }
  if (typeName === "Object") {
    // `Object(InputEventKey, ...)` — the first argument names the actual
    // type; preserve it as the opaque type name so structural scanners can
    // classify the value without evaluating it.
    const innerType = args.length > 0 ? unquote(args[0]) : null;
    return {
      value: { kind: "opaque", typeName: innerType ?? "Object", raw: boundedRaw(text) },
      truncated: false,
    };
  }
  if (typeName.startsWith("Packed")) {
    const items: GodotVariantValue[] = [];
    let truncated = false;
    for (const argument of args) {
      if (items.length >= GODOT_SCENE_LIMITS.maxArrayItems) {
        truncated = true;
        break;
      }
      items.push(parseVariant(argument, depth + 1).value);
    }
    return { value: { kind: "packed_array", typeName, items }, truncated };
  }
  // Unknown constructor form: preserve as bounded opaque raw.
  return { value: { kind: "opaque", typeName, raw: boundedRaw(text) }, truncated: false };
}

function parseArray(inner: string, depth: number): VariantParseResult {
  const items: GodotVariantValue[] = [];
  let truncated = false;
  for (const item of splitTopLevelArguments(inner)) {
    if (items.length >= GODOT_SCENE_LIMITS.maxArrayItems) {
      truncated = true;
      break;
    }
    const parsed = parseVariant(item, depth);
    items.push(parsed.value);
    truncated = truncated || parsed.truncated;
  }
  return { value: { kind: "array", items }, truncated };
}

function parseDictionary(inner: string, depth: number): VariantParseResult {
  const entries: { readonly key: GodotVariantValue; readonly value: GodotVariantValue }[] = [];
  let truncated = false;
  const pairs = splitTopLevelPairs(inner);
  for (const pair of pairs) {
    if (entries.length >= GODOT_SCENE_LIMITS.maxDictionaryEntries) {
      truncated = true;
      break;
    }
    const parsedKey = parseVariant(pair.key, depth);
    const parsedValue = parseVariant(pair.value, depth);
    entries.push({ key: parsedKey.value, value: parsedValue.value });
    truncated = truncated || parsedKey.truncated || parsedValue.truncated;
  }
  return { value: { kind: "dictionary", entries }, truncated };
}

/**
 * Split comma-separated top-level arguments (respecting strings and
 * nested brackets). Empty segments are dropped.
 */
export function splitTopLevelArguments(text: string): readonly string[] {
  const parts: string[] = [];
  let start = 0;
  let index = 0;
  const depth: string[] = [];
  let inString = false;
  let quote = "";
  while (index < text.length) {
    const character = text[index] as string;
    if (inString) {
      if (character === "\\") {
        index += 2;
        continue;
      }
      if (character === quote) {
        inString = false;
      }
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      inString = true;
      quote = character;
      index += 1;
      continue;
    }
    if (character === "(" || character === "[" || character === "{") {
      depth.push(character);
      index += 1;
      continue;
    }
    if (character === ")" || character === "]" || character === "}") {
      depth.pop();
      index += 1;
      continue;
    }
    if (character === "," && depth.length === 0) {
      const part = text.slice(start, index).trim();
      if (part.length > 0) {
        parts.push(part);
      }
      start = index + 1;
    }
    index += 1;
  }
  const tail = text.slice(start).trim();
  if (tail.length > 0) {
    parts.push(tail);
  }
  return parts;
}

/**
 * Split top-level `key: value` pairs of a dictionary body (Godot writes
 * `{"key": value, "other": value}`).
 */
function splitTopLevelPairs(
  inner: string,
): readonly { readonly key: string; readonly value: string }[] {
  const pairs: { readonly key: string; readonly value: string }[] = [];
  let index = 0;
  while (index < inner.length) {
    while (index < inner.length && (inner[index] === " " || inner[index] === ",")) {
      index += 1;
    }
    if (index >= inner.length) {
      break;
    }
    // Scan the key: a quoted string or a bare token up to `:`.
    const keyStart = index;
    let inString = false;
    let quote = "";
    let keyEnd = -1;
    while (index < inner.length) {
      const character = inner[index] as string;
      if (inString) {
        if (character === "\\") {
          index += 2;
          continue;
        }
        if (character === quote) {
          inString = false;
        }
        index += 1;
        continue;
      }
      if (character === '"' || character === "'") {
        inString = true;
        quote = character;
        index += 1;
        continue;
      }
      if (character === ":") {
        keyEnd = index;
        break;
      }
      index += 1;
    }
    if (keyEnd < 0) {
      break;
    }
    const key = inner.slice(keyStart, keyEnd).trim();
    index = keyEnd + 1;
    // Value runs to the next top-level comma (or end).
    const valueStart = index;
    const depth: string[] = [];
    inString = false;
    quote = "";
    let valueEnd = inner.length;
    while (index < inner.length) {
      const character = inner[index] as string;
      if (inString) {
        if (character === "\\") {
          index += 2;
          continue;
        }
        if (character === quote) {
          inString = false;
        }
        index += 1;
        continue;
      }
      if (character === '"' || character === "'") {
        inString = true;
        quote = character;
        index += 1;
        continue;
      }
      if (character === "(" || character === "[" || character === "{") {
        depth.push(character);
        index += 1;
        continue;
      }
      if (character === ")" || character === "]" || character === "}") {
        depth.pop();
        index += 1;
        continue;
      }
      if (character === "," && depth.length === 0) {
        valueEnd = index;
        break;
      }
      index += 1;
    }
    pairs.push({ key, value: inner.slice(valueStart, valueEnd).trim() });
    index = valueEnd + 1;
  }
  return pairs;
}

function parseNumberComponents(
  args: readonly string[],
  maxComponents: number,
): readonly number[] | null {
  if (args.length === 0 || args.length > maxComponents) {
    return null;
  }
  const components: number[] = [];
  for (const argument of args) {
    const numeric = Number(argument);
    if (!Number.isFinite(numeric)) {
      return null;
    }
    components.push(numeric);
  }
  return components;
}

function isVectorType(typeName: string): boolean {
  return (
    typeName === "Vector2" ||
    typeName === "Vector2i" ||
    typeName === "Vector3" ||
    typeName === "Vector3i" ||
    typeName === "Vector4" ||
    typeName === "Vector4i" ||
    typeName === "Rect2" ||
    typeName === "Rect2i" ||
    typeName === "Transform2D" ||
    typeName === "Transform3D" ||
    typeName === "Basis" ||
    typeName === "Quaternion" ||
    typeName === "Plane" ||
    typeName === "AABB" ||
    typeName === "Projection"
  );
}

function unquote(text: string | undefined): string | null {
  if (text === undefined) {
    return null;
  }
  const trimmed = text.trim();
  if (trimmed.length >= 2 && trimmed.startsWith('"') && trimmed.endsWith('"')) {
    const parsed = parseQuotedString(trimmed);
    return parsed.ok ? parsed.value : null;
  }
  return trimmed.length > 0 ? trimmed : null;
}

/** Parse a double-quoted string with Godot escape sequences. */
export function parseQuotedString(
  raw: string,
): { readonly ok: true; readonly value: string } | { readonly ok: false } {
  if (!raw.startsWith('"')) {
    return { ok: false };
  }
  let value = "";
  let index = 1;
  let closed = false;
  while (index < raw.length) {
    const character = raw[index] as string;
    if (character === '"') {
      closed = true;
      index += 1;
      break;
    }
    if (character === "\\") {
      const next = raw[index + 1];
      if (next === "n") {
        value += "\n";
      } else if (next === "t") {
        value += "\t";
      } else if (next === "r") {
        value += "\r";
      } else if (next === "b") {
        value += "\b";
      } else if (next === "f") {
        value += "\f";
      } else if (next === "u" || next === "U") {
        // Unicode escape \uXXXX / \UXXXXXXXX — preserved as raw when malformed.
        const length = next === "u" ? 4 : 8;
        const digits = raw.slice(index + 2, index + 2 + length);
        if (/^[0-9a-fA-F]{4}$/.test(digits) && next === "u") {
          value += String.fromCodePoint(Number.parseInt(digits, 16));
          index += 2 + length;
          continue;
        }
        value += `\\${next}`;
      } else if (next === '"' || next === "\\" || next === "'" || next === "/") {
        value += next;
      } else if (next === undefined) {
        break;
      } else {
        value += `\\${next}`;
      }
      index += 2;
      continue;
    }
    value += character;
    index += 1;
  }
  if (!closed) {
    return { ok: false };
  }
  if (raw.slice(index).trim().length > 0) {
    return { ok: false };
  }
  return { ok: true, value };
}

function boundedRaw(text: string): { readonly text: string; readonly truncated: boolean } {
  if (text.length <= GODOT_SCENE_LIMITS.maxRawValueLength) {
    return { text, truncated: false };
  }
  return { text: text.slice(0, GODOT_SCENE_LIMITS.maxRawValueLength), truncated: true };
}
