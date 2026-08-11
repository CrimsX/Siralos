import { GODOT_SCENE_LIMITS } from "./limits.js";
import type {
  ExternalResourceRef,
  GodotDiagnosticCode,
  GodotResourceModel,
  GodotTextDiagnostic,
  GodotTextDocument,
  ResourceReference,
  SubResourceRef,
} from "./models.js";
import { resolveResPath } from "./resolution.js";
import { isBalancedText, isCommentLine, parseHeaderAttributes, splitKeyValue } from "./text.js";
import { parseGodotVariant, parseQuotedString } from "./variant.js";

/**
 * Deterministic `.tres` (Godot text resource) parser (Stage 3 milestone 8).
 *
 * Parses resource headers (`type`, `load_steps`, `format`, `uid`),
 * `ext_resource`, `sub_resource`, and the `[resource]` property section.
 * Parsing stays generic: common Godot resources are inspected through the
 * same property model without class-specific implementations. Values are
 * parsed conservatively and unknown forms are preserved as bounded raw
 * text. No expressions are evaluated and no project code runs.
 */

export interface ParseResourceOptions {
  /** Exact workspace revision of the parsed source state (host-bound). */
  readonly revision?: string | null;
}

type DiagnosticSink = (
  code: GodotDiagnosticCode,
  severity: GodotTextDiagnostic["severity"],
  message: string,
  line?: number,
) => void;

/** Mutable build-time subresource shape. */
interface MutableSubResource {
  readonly id: string;
  readonly type: string;
  readonly line: number;
  readonly properties: {
    readonly name: string;
    readonly value: import("./models.js").GodotVariantValue;
    readonly rawValue: string;
    readonly line: number;
  }[];
}

export function parseGodotResource(
  content: string,
  path: string,
  options: ParseResourceOptions = {},
): GodotTextDocument<GodotResourceModel> {
  const revision = options.revision ?? null;
  const diagnostics: GodotTextDiagnostic[] = [];
  const addDiagnostic: DiagnosticSink = (code, severity, message, line) => {
    if (diagnostics.length < GODOT_SCENE_LIMITS.maxDiagnostics) {
      diagnostics.push({ code, severity, message, ...(line === undefined ? {} : { line }) });
    }
  };

  const externalResources: ExternalResourceRef[] = [];
  const subResources: MutableSubResource[] = [];
  const properties: { readonly name: string; readonly valueText: string; readonly line: number }[] =
    [];
  const extIds = new Set<string>();
  const subIds = new Set<string>();
  let header: { type: string; format?: number; loadSteps?: number; uid?: string } | null = null;
  let currentSection: "header" | "ext_resource" | "sub_resource" | "resource" | "body" = "header";
  let currentSubResource: MutableSubResource | null = null;
  let seenResourceHeader = false;
  let truncated = false;
  let limitReached = false;
  let sectionCount = 0;
  let resourceCount = 0;
  let propertyCount = 0;

  const lines = content.split(/\r?\n/);
  const lineCount = Math.min(lines.length, GODOT_SCENE_LIMITS.maxLines);
  if (lines.length > GODOT_SCENE_LIMITS.maxLines) {
    truncated = true;
    addDiagnostic(
      "resource.document_truncated",
      "error",
      "The document exceeds the line bound; parsing stopped.",
    );
  }

  for (let index = 0; index < lineCount && !limitReached; index += 1) {
    const rawLine = lines[index] ?? "";
    const trimmed = rawLine.trim();
    if (trimmed.length === 0 || isCommentLine(trimmed)) {
      continue;
    }
    if (trimmed.startsWith("[")) {
      sectionCount += 1;
      if (sectionCount > GODOT_SCENE_LIMITS.maxSections) {
        truncated = true;
        limitReached = true;
        addDiagnostic(
          "resource.document_truncated",
          "error",
          `The section count exceeded the bound (${GODOT_SCENE_LIMITS.maxSections}); parsing stopped.`,
        );
        break;
      }
      // The section's closing bracket is the LAST "]" on the line: header
      // values may themselves contain "]" (e.g. `binds=[1, "x"]` or
      // `groups=["a"]`), so the first "]" is never a safe split point.
      const closeIndex = trimmed.lastIndexOf("]");
      if (closeIndex < 0) {
        addDiagnostic(
          "resource.malformed_section",
          "error",
          `Malformed section header at line ${index + 1}: missing closing bracket.`,
          index + 1,
        );
        currentSection = "body";
        currentSubResource = null;
        continue;
      }
      const inner = trimmed.slice(1, closeIndex);
      const match = /^([A-Za-z_][A-Za-z0-9_]*)(?:\s+(.*))?$/s.exec(inner);
      if (match === null) {
        addDiagnostic(
          "resource.malformed_section",
          "error",
          `Malformed section header at line ${index + 1}.`,
          index + 1,
        );
        currentSection = "body";
        continue;
      }
      const sectionName = match[1] as string;
      const attributes = parseHeaderAttributes(
        match[2] ?? "",
        GODOT_SCENE_LIMITS.maxHeaderAttributes,
      ).attributes;
      switch (sectionName) {
        case "gd_resource": {
          if (seenResourceHeader) {
            addDiagnostic(
              "resource.unexpected_header",
              "error",
              `Unexpected duplicate resource header at line ${index + 1}.`,
              index + 1,
            );
          }
          seenResourceHeader = true;
          header = parseResourceHeader(attributes, addDiagnostic, index + 1);
          currentSection = "header";
          currentSubResource = null;
          break;
        }
        case "gd_scene":
          addDiagnostic(
            "resource.unexpected_header",
            "error",
            `A scene header ([gd_scene]) is not valid inside a .tres document (line ${index + 1}).`,
            index + 1,
          );
          currentSection = "header";
          break;
        case "ext_resource": {
          currentSection = "ext_resource";
          currentSubResource = null;
          if (resourceCount >= GODOT_SCENE_LIMITS.maxResources) {
            truncated = true;
            addDiagnostic(
              "resource.document_truncated",
              "error",
              `The resource count exceeded the bound (${GODOT_SCENE_LIMITS.maxResources}); remaining resources are ignored.`,
              index + 1,
            );
            break;
          }
          const ref = parseExtResource(attributes, index + 1, addDiagnostic, "resource");
          if (ref !== null) {
            resourceCount += 1;
            if (extIds.has(ref.id)) {
              addDiagnostic(
                "resource.duplicate_resource_id",
                "error",
                `Duplicate ext_resource id "${ref.id}" at line ${index + 1}; the later declaration is ignored.`,
                index + 1,
              );
            } else {
              extIds.add(ref.id);
              externalResources.push(ref);
            }
          }
          break;
        }
        case "sub_resource": {
          currentSection = "sub_resource";
          if (resourceCount >= GODOT_SCENE_LIMITS.maxResources) {
            truncated = true;
            addDiagnostic(
              "resource.document_truncated",
              "error",
              `The resource count exceeded the bound (${GODOT_SCENE_LIMITS.maxResources}); remaining resources are ignored.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          resourceCount += 1;
          const type = readStringAttribute(attributes, "type") ?? "";
          const id = readStringAttribute(attributes, "id");
          if (id === null) {
            addDiagnostic(
              "resource.missing_resource_id",
              "error",
              `sub_resource at line ${index + 1} is missing its id attribute.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          if (subIds.has(id)) {
            addDiagnostic(
              "resource.duplicate_resource_id",
              "error",
              `Duplicate sub_resource id "${id}" at line ${index + 1}; the later declaration is ignored.`,
              index + 1,
            );
            currentSubResource = null;
            break;
          }
          subIds.add(id);
          currentSubResource = { id, type, properties: [], line: index + 1 };
          subResources.push(currentSubResource);
          break;
        }
        case "resource": {
          currentSection = "resource";
          currentSubResource = null;
          break;
        }
        default:
          addDiagnostic(
            "resource.malformed_section",
            "error",
            `Unknown section header "[${sectionName}]" at line ${index + 1}.`,
            index + 1,
          );
          currentSection = "body";
          currentSubResource = null;
          break;
      }
      continue;
    }

    // Ordinary `key = value` record (possibly multiline).
    const record = readRecord(lines, index, lineCount, addDiagnostic);
    index = record.endIndex;
    if (record.key === null) {
      continue;
    }
    if (currentSection === "resource") {
      if (propertyCount >= GODOT_SCENE_LIMITS.maxProperties) {
        truncated = true;
        addDiagnostic(
          "resource.document_truncated",
          "error",
          `The property count exceeded the bound (${GODOT_SCENE_LIMITS.maxProperties}); remaining properties are ignored.`,
          record.line,
        );
        continue;
      }
      propertyCount += 1;
      properties.push({
        name: unquoteKey(record.key),
        valueText: record.valueText,
        line: record.line,
      });
    } else if (currentSection === "sub_resource" && currentSubResource !== null) {
      if (propertyCount >= GODOT_SCENE_LIMITS.maxProperties) {
        truncated = true;
        addDiagnostic(
          "resource.document_truncated",
          "error",
          `The property count exceeded the bound (${GODOT_SCENE_LIMITS.maxProperties}); remaining properties are ignored.`,
          record.line,
        );
        continue;
      }
      propertyCount += 1;
      currentSubResource.properties.push(
        makeProperty(record.key, record.valueText, record.line, addDiagnostic),
      );
    } else {
      addDiagnostic(
        "resource.unknown_property",
        "warning",
        `Property "${record.key}" at line ${record.line} is not valid in the current section and was ignored.`,
        record.line,
      );
    }
  }

  const model = buildResourceModel(
    path,
    revision,
    header,
    externalResources,
    subResources,
    properties,
    addDiagnostic,
  );

  const errorCount = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length;
  const status =
    !seenResourceHeader || header === null ? "invalid" : errorCount === 0 ? "complete" : "partial";
  return {
    path,
    revision,
    kind: "resource",
    status,
    document: model,
    diagnostics,
    truncated,
  };
}

function parseResourceHeader(
  attributes: readonly HeaderAttribute[],
  addDiagnostic: DiagnosticSink,
  line: number,
): { type: string; format?: number; loadSteps?: number; uid?: string } {
  const type = readStringAttribute(attributes, "type") ?? "";
  if (type.length === 0) {
    addDiagnostic(
      "resource.malformed_section",
      "error",
      `The resource header at line ${line} is missing its type attribute.`,
      line,
    );
  }
  const header: { type: string; format?: number; loadSteps?: number; uid?: string } = { type };
  for (const attribute of attributes) {
    if (attribute.name === "format") {
      const parsed = Number(attribute.valueText);
      if (Number.isInteger(parsed) && parsed >= 0) {
        header.format = parsed;
      }
    } else if (attribute.name === "load_steps") {
      const parsed = Number(attribute.valueText);
      if (Number.isInteger(parsed) && parsed >= 0) {
        header.loadSteps = parsed;
      }
    } else if (attribute.name === "uid") {
      const uid = unquoteValue(attribute.valueText);
      if (uid !== null && uid.startsWith("uid://")) {
        header.uid = uid;
      }
    }
  }
  return header;
}

function parseExtResource(
  attributes: readonly HeaderAttribute[],
  line: number,
  addDiagnostic: DiagnosticSink,
  kind: "scene" | "resource",
): ExternalResourceRef | null {
  const id = readStringAttribute(attributes, "id");
  if (id === null) {
    addDiagnostic(
      kind === "scene" ? "scene.missing_resource_id" : "resource.missing_resource_id",
      "error",
      `ext_resource at line ${line} is missing its id attribute.`,
      line,
    );
    return null;
  }
  const type = readStringAttribute(attributes, "type");
  const path = readStringAttribute(attributes, "path");
  const uid = readStringAttribute(attributes, "uid");
  return {
    id,
    ...(type === null ? {} : { type }),
    ...(path === null ? {} : { path }),
    ...(uid === null ? {} : { uid }),
    line,
  };
}

function buildResourceModel(
  path: string,
  revision: string | null,
  header: { type: string; format?: number; loadSteps?: number; uid?: string } | null,
  externalResources: readonly ExternalResourceRef[],
  subResources: readonly SubResourceRef[],
  properties: readonly {
    readonly name: string;
    readonly valueText: string;
    readonly line: number;
  }[],
  addDiagnostic: DiagnosticSink,
): GodotResourceModel | null {
  if (header === null) {
    return null;
  }
  const scriptProperty = properties.find((property) => property.name === "script");
  const script = resolveScriptReference(
    scriptProperty,
    externalResources,
    subResources,
    addDiagnostic,
  );
  return {
    path,
    revision,
    type: header.type,
    ...(header.uid === undefined ? {} : { uid: header.uid }),
    ...(header.format === undefined ? {} : { format: header.format }),
    ...(header.loadSteps === undefined ? {} : { loadSteps: header.loadSteps }),
    ...(script === undefined ? {} : { script }),
    externalResources,
    subResources,
    properties: properties.map((property) => ({
      name: property.name,
      value: parseGodotVariant(property.valueText).value,
      rawValue: property.valueText.slice(0, GODOT_SCENE_LIMITS.maxRawValueLength),
      line: property.line,
    })),
  };
}

function resolveScriptReference(
  scriptProperty:
    { readonly name: string; readonly valueText: string; readonly line: number } | undefined,
  externalResources: readonly ExternalResourceRef[],
  subResources: readonly SubResourceRef[],
  addDiagnostic: DiagnosticSink,
): ResourceReference | undefined {
  if (scriptProperty === undefined) {
    return undefined;
  }
  const parsed = parseGodotVariant(scriptProperty.valueText);
  const scriptValue = parsed.value;
  if (scriptValue.kind === "ext_resource") {
    const declared = externalResources.find((resource) => resource.id === scriptValue.id);
    if (declared === undefined) {
      addDiagnostic(
        "resource.unknown_resource_reference",
        "warning",
        `Unknown script reference ExtResource("${scriptValue.id}") — no matching ext_resource declaration.`,
        scriptProperty.line,
      );
      return undefined;
    }
    const resolvedPath = declared.path === undefined ? undefined : resolveResPath(declared.path);
    return {
      resource: declared,
      ...(resolvedPath?.ok === true ? { resolvedPath: resolvedPath.relativePath } : {}),
    };
  }
  if (scriptValue.kind === "sub_resource") {
    const declared = subResources.find((resource) => resource.id === scriptValue.id);
    if (declared === undefined) {
      addDiagnostic(
        "resource.unknown_resource_reference",
        "warning",
        `Unknown script reference SubResource("${scriptValue.id}") — no matching sub_resource declaration.`,
        scriptProperty.line,
      );
      return undefined;
    }
    return {
      resource: {
        id: declared.id,
        type: declared.type,
        ...(declared.line === undefined ? {} : { line: declared.line }),
      },
    };
  }
  return undefined;
}

interface Record {
  readonly key: string | null;
  readonly valueText: string;
  readonly line: number;
  readonly endIndex: number;
}

function readRecord(
  lines: readonly string[],
  startIndex: number,
  lineCount: number,
  addDiagnostic: DiagnosticSink,
): Record {
  const lineNumber = startIndex + 1;
  const firstLine = lines[startIndex] ?? "";
  const split = splitKeyValue(firstLine);
  if (split === null) {
    addDiagnostic(
      "resource.unknown_property",
      "warning",
      `Unrecognized record without a value at line ${lineNumber}.`,
      lineNumber,
    );
    return { key: null, valueText: "", line: lineNumber, endIndex: startIndex };
  }
  let valueText = firstLine.slice(split.valueStart).trim();
  let endIndex = startIndex;
  let continuation = 0;
  while (
    !isBalancedText(valueText) &&
    continuation < GODOT_SCENE_LIMITS.maxValueContinuationLines
  ) {
    endIndex += 1;
    continuation += 1;
    if (endIndex >= lineCount) {
      break;
    }
    const nextLine = lines[endIndex] ?? "";
    const nextTrimmed = nextLine.trim();
    if (nextTrimmed.length === 0 || isCommentLine(nextTrimmed)) {
      continue;
    }
    valueText = `${valueText}\n${nextLine.trim()}`;
  }
  if (!isBalancedText(valueText)) {
    addDiagnostic(
      "resource.unbalanced_value",
      "error",
      `The value of "${split.key}" at line ${lineNumber} is unbalanced; it was truncated at the continuation bound.`,
      lineNumber,
    );
  }
  return { key: split.key, valueText, line: lineNumber, endIndex };
}

function makeProperty(
  key: string,
  valueText: string,
  line: number,
  addDiagnostic: DiagnosticSink,
): {
  readonly name: string;
  readonly value: import("./models.js").GodotVariantValue;
  readonly rawValue: string;
  readonly line: number;
} {
  const parsed = parseGodotVariant(valueText);
  if (parsed.truncated) {
    addDiagnostic(
      "resource.value_truncated",
      "warning",
      `The value of "${key}" at line ${line} exceeds interpretation bounds and was preserved partially.`,
      line,
    );
  }
  return {
    name: unquoteKey(key),
    value: parsed.value,
    rawValue: valueText.slice(0, GODOT_SCENE_LIMITS.maxRawValueLength),
    line,
  };
}

function unquoteKey(key: string): string {
  if (key.length >= 2 && key.startsWith('"') && key.endsWith('"')) {
    return key.slice(1, -1);
  }
  return key;
}

interface HeaderAttribute {
  readonly name: string;
  readonly valueText: string;
  readonly quoted: boolean;
}

function readStringAttribute(attributes: readonly HeaderAttribute[], name: string): string | null {
  const attribute = attributes.find((candidate) => candidate.name === name);
  if (attribute === null || attribute === undefined) {
    return null;
  }
  return unquoteValue(attribute.valueText);
}

function unquoteValue(valueText: string): string | null {
  if (valueText.length >= 2 && valueText.startsWith('"') && valueText.endsWith('"')) {
    const parsed = parseQuotedString(valueText);
    return parsed.ok ? parsed.value : null;
  }
  return valueText.length > 0 ? valueText : null;
}
