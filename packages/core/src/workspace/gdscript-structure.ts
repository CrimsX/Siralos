/**
 * Deterministic GDScript structural extraction (Stage 3 milestone 3).
 *
 * A lightweight scanner — not a full compiler — that extracts the
 * high-level declaration structure of a Godot 4.x GDScript file: extends,
 * class_name, file/declaration annotations, signals, enums, constants,
 * typed/untyped properties, and function signatures (name, parameters,
 * return annotation, static, source range). Only syntax-derived facts are
 * reported; no semantic interpretation. Keywords inside comments or string
 * literals never produce declarations. A syntactically invalid file yields
 * a structured `partial` result with parser errors — never fabricated
 * structure and never an infrastructure exception.
 */

export interface GDScriptParameter {
  readonly name: string;
  readonly type: string | null;
}

export interface GDScriptAnnotationInfo {
  readonly name: string;
  readonly arguments: readonly string[];
  readonly line: number;
}

export interface GDScriptFunctionInfo {
  readonly name: string;
  readonly parameters: readonly GDScriptParameter[];
  readonly returnType: string | null;
  readonly isStatic: boolean;
  readonly annotations: readonly string[];
  readonly line: number;
  readonly multilineSignature: boolean;
}

export interface GDScriptPropertyInfo {
  readonly name: string;
  readonly type: string | null;
  readonly annotations: readonly string[];
  readonly line: number;
  readonly multiline: boolean;
}

export interface GDScriptSignalInfo {
  readonly name: string;
  readonly parameters: readonly GDScriptParameter[];
  readonly line: number;
}

export interface GDScriptEnumInfo {
  readonly name: string | null;
  readonly members: readonly string[];
  readonly line: number;
  readonly multiline: boolean;
}

export interface GDScriptConstantInfo {
  readonly name: string;
  readonly type: string | null;
  readonly line: number;
  readonly multiline: boolean;
}

export interface GDScriptParserError {
  readonly line: number;
  readonly message: string;
}

export interface GDScriptStructure {
  readonly path: string;
  readonly extendsType: string | null;
  readonly className: string | null;
  readonly fileAnnotations: readonly GDScriptAnnotationInfo[];
  readonly signals: readonly GDScriptSignalInfo[];
  readonly enums: readonly GDScriptEnumInfo[];
  readonly constants: readonly GDScriptConstantInfo[];
  readonly properties: readonly GDScriptPropertyInfo[];
  readonly functions: readonly GDScriptFunctionInfo[];
  /** preload/load string paths referenced by the file. */
  readonly dependencies: readonly string[];
  readonly status: "complete" | "partial";
  readonly parserErrors: readonly GDScriptParserError[];
  /** True when the declaration cap was reached (output is bounded). */
  readonly truncated: boolean;
}

export const GDSCRIPT_STRUCTURE_LIMITS = {
  /** Declaration cap: never silently unbounded structural output. */
  maxDeclarations: 256,
  maxDependencies: 32,
  maxAnnotationArguments: 16,
  maxParameters: 32,
  maxEnumMembers: 256,
} as const;

interface Token {
  readonly kind: "ident" | "string" | "number" | "punct" | "newline" | "error";
  readonly text: string;
  /** Unquoted string value; null for non-string tokens. */
  readonly value: string | null;
  readonly line: number;
  readonly column: number;
}

/**
 * Tokenize GDScript with string/comment awareness. Comments are dropped;
 * unterminated multiline strings become error tokens.
 */
function tokenize(source: string): {
  readonly tokens: readonly Token[];
  readonly errors: readonly GDScriptParserError[];
} {
  const tokens: Token[] = [];
  const errors: GDScriptParserError[] = [];
  let index = 0;
  let line = 1;
  let column = 0;

  function advance(count: number): void {
    for (let step = 0; step < count; step += 1) {
      if (source[index] === "\n") {
        line += 1;
        column = 0;
      } else {
        column += 1;
      }
      index += 1;
    }
  }

  while (index < source.length) {
    const character = source[index] as string;
    if (character === " " || character === "\t" || character === "\r") {
      advance(1);
      continue;
    }
    if (character === "\n") {
      tokens.push({ kind: "newline", text: "\n", value: null, line, column });
      advance(1);
      continue;
    }
    if (character === "#") {
      while (index < source.length && source[index] !== "\n") {
        advance(1);
      }
      continue;
    }
    if (character === '"' || character === "'") {
      const quote = character;
      const startLine = line;
      const startColumn = column;
      const multiline =
        (quote === '"' && source.startsWith('"""', index)) ||
        (quote === "'" && source.startsWith("'''", index));
      advance(multiline ? 3 : 1);
      let value = "";
      let closed = false;
      while (index < source.length) {
        if (
          multiline &&
          ((quote === '"' && source.startsWith('"""', index)) ||
            (quote === "'" && source.startsWith("'''", index)))
        ) {
          advance(3);
          closed = true;
          break;
        }
        if (!multiline && source[index] === quote) {
          advance(1);
          closed = true;
          break;
        }
        if (!multiline && source[index] === "\\" && index + 1 < source.length) {
          value += source[index + 1];
          advance(2);
          continue;
        }
        value += source[index];
        advance(1);
        if (!multiline && source[index - 1] === "\n") {
          break; // single-line string cannot span lines
        }
      }
      if (!closed) {
        errors.push({
          line: startLine,
          message: multiline ? "Unterminated multiline string." : "Unterminated string literal.",
        });
        tokens.push({ kind: "error", text: "", value: null, line: startLine, column: startColumn });
        continue;
      }
      tokens.push({ kind: "string", text: value, value, line: startLine, column: startColumn });
      continue;
    }
    if (character === "_" || /[A-Za-z]/.test(character)) {
      const startLine = line;
      const startColumn = column;
      let text = "";
      while (index < source.length && /[A-Za-z0-9_]/.test(source[index] as string)) {
        text += source[index];
        advance(1);
      }
      tokens.push({ kind: "ident", text, value: null, line: startLine, column: startColumn });
      continue;
    }
    if (/[0-9]/.test(character)) {
      const startLine = line;
      const startColumn = column;
      let text = "";
      while (
        index < source.length &&
        /[0-9._xXa-fA-F]/.test(source[index] as string) &&
        source[index] !== "."
      ) {
        // numbers: decimals, hex (0x...), underscores; '.' handled below
        if (/[a-fA-F]/.test(source[index] as string) && !text.toLowerCase().includes("x")) {
          break;
        }
        text += source[index];
        advance(1);
      }
      if (source[index] === "." && /[0-9]/.test(source[index + 1] ?? "")) {
        text += ".";
        advance(1);
        while (index < source.length && /[0-9_]/.test(source[index] as string)) {
          text += source[index];
          advance(1);
        }
      }
      tokens.push({ kind: "number", text, value: null, line: startLine, column: startColumn });
      continue;
    }
    // punctuation (including -> handled by consumers as two puncts)
    tokens.push({ kind: "punct", text: character, value: null, line, column });
    advance(1);
  }
  return { tokens, errors };
}

interface Cursor {
  readonly tokens: readonly Token[];
  index: number;
}

function peek(cursor: Cursor, offset = 0): Token | null {
  return cursor.tokens[cursor.index + offset] ?? null;
}

/** Skip newline tokens (blank/comment-only lines). */
function skipNewlines(cursor: Cursor): void {
  while (peek(cursor)?.kind === "newline") {
    cursor.index += 1;
  }
}

function isBracketOpen(text: string): boolean {
  return text === "(" || text === "[" || text === "{";
}

function isBracketClose(text: string): boolean {
  return text === ")" || text === "]" || text === "}";
}

/** Collect an identifier (or null when the token is not an ident). */
function expectIdent(cursor: Cursor): string | null {
  const token = peek(cursor);
  if (token?.kind === "ident") {
    cursor.index += 1;
    return token.text;
  }
  return null;
}

/** Collect a type name: identifier with optional dots (e.g. Array[String]). */
function collectTypeName(cursor: Cursor): string | null {
  const token = peek(cursor);
  if (token?.kind !== "ident") {
    return null;
  }
  let typeName = "";
  let depth = 0;
  while (true) {
    const current = peek(cursor);
    if (current === null) {
      break;
    }
    if (current.kind === "ident" || current.kind === "number") {
      typeName += current.text;
      cursor.index += 1;
      continue;
    }
    if (
      current.kind === "punct" &&
      (current.text === "." || current.text === "[" || current.text === "]")
    ) {
      typeName += current.text;
      if (current.text === "[") {
        depth += 1;
      } else if (current.text === "]") {
        depth -= 1;
        if (depth < 0) {
          break;
        }
      }
      cursor.index += 1;
      continue;
    }
    if (current.kind === "punct" && current.text === "," && depth > 0) {
      typeName += current.text;
      cursor.index += 1;
      continue;
    }
    break;
  }
  return typeName.length === 0 ? null : typeName;
}

/** Collect a parenthesized parameter list; may span lines. */
function collectParameters(
  cursor: Cursor,
  parensRequired: boolean,
): {
  readonly parameters: readonly GDScriptParameter[];
  readonly multiline: boolean;
  readonly error: string | null;
} {
  const open = peek(cursor);
  if (open?.text !== "(") {
    // A parameterless signal needs no parentheses; a function does.
    return {
      parameters: [],
      multiline: false,
      error: parensRequired ? "Expected '(' after declaration name." : null,
    };
  }
  cursor.index += 1;
  const parameters: GDScriptParameter[] = [];
  let multiline = false;
  let depth = 1;
  let pendingName: string | null = null;
  while (true) {
    const token = peek(cursor);
    if (token === null) {
      return { parameters, multiline, error: "Unterminated parameter list." };
    }
    if (token.kind === "newline") {
      multiline = true;
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === "(") {
      depth += 1;
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === ")") {
      depth -= 1;
      cursor.index += 1;
      if (depth === 0) {
        if (pendingName !== null) {
          parameters.push({ name: pendingName, type: null });
        }
        break;
      }
      continue;
    }
    if (token.kind === "punct" && token.text === ",") {
      if (pendingName !== null) {
        parameters.push({ name: pendingName, type: null });
        pendingName = null;
      }
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === ":") {
      cursor.index += 1;
      const type = collectTypeName(cursor);
      if (pendingName !== null && type !== null) {
        const last = parameters[parameters.length - 1];
        if (last !== undefined && last.name === pendingName) {
          parameters[parameters.length - 1] = { name: last.name, type };
        } else {
          parameters.push({ name: pendingName, type });
        }
      }
      pendingName = null;
      continue;
    }
    if (token.kind === "ident" && pendingName === null) {
      pendingName = token.text;
      cursor.index += 1;
      continue;
    }
    cursor.index += 1;
    if (parameters.length >= GDSCRIPT_STRUCTURE_LIMITS.maxParameters) {
      break;
    }
  }
  return { parameters, multiline, error: null };
}

/**
 * Consume a statement body (a value/expression after `=`, an enum block,
 * or an ordinary statement) until a newline at bracket balance zero.
 * Returns the number of lines consumed beyond the first.
 */
function consumeStatement(cursor: Cursor, baseline = 0): { readonly multiline: boolean } {
  let depth = baseline;
  let multiline = false;
  while (true) {
    const token = peek(cursor);
    if (token === null) {
      return { multiline };
    }
    if (token.kind === "newline") {
      if (depth === baseline) {
        cursor.index += 1;
        return { multiline };
      }
      multiline = true;
      cursor.index += 1;
      continue;
    }
    if (token.kind === "error") {
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct") {
      if (isBracketOpen(token.text)) {
        depth += 1;
      } else if (isBracketClose(token.text)) {
        depth -= 1;
        if (depth < baseline) {
          cursor.index += 1;
          return { multiline };
        }
      }
    }
    cursor.index += 1;
  }
}

/** Consume annotation arguments up to a balanced closing paren. */
function collectAnnotation(cursor: Cursor, nameToken: Token): GDScriptAnnotationInfo {
  const arguments_: string[] = [];
  if (peek(cursor)?.text !== "(") {
    return { name: nameToken.text, arguments: arguments_, line: nameToken.line };
  }
  let depth = 0;
  while (true) {
    const token = peek(cursor);
    if (token === null || token.kind === "newline") {
      if (depth === 0) {
        if (token !== null) {
          cursor.index += 1;
        }
        break;
      }
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === "(") {
      depth += 1;
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === ")") {
      depth -= 1;
      cursor.index += 1;
      if (depth === 0) {
        break;
      }
      continue;
    }
    if (token.kind === "punct" && token.text === "," && depth === 1) {
      cursor.index += 1;
      continue;
    }
    if (
      token.kind === "string" &&
      depth === 1 &&
      arguments_.length < GDSCRIPT_STRUCTURE_LIMITS.maxAnnotationArguments
    ) {
      arguments_.push(token.value ?? "");
      cursor.index += 1;
      continue;
    }
    if (
      token.kind === "ident" &&
      depth === 1 &&
      arguments_.length < GDSCRIPT_STRUCTURE_LIMITS.maxAnnotationArguments
    ) {
      arguments_.push(token.text);
      cursor.index += 1;
      continue;
    }
    cursor.index += 1;
  }
  return { name: nameToken.text, arguments: arguments_, line: nameToken.line };
}

/** Consume the body of a function until the next line indented at or above
 * the function's own indentation (standard GDScript block rule). */
function consumeFunctionBody(cursor: Cursor, funcColumn: number): void {
  while (true) {
    const token = peek(cursor);
    if (token === null) {
      return;
    }
    if (token.kind === "newline") {
      cursor.index += 1;
      // Look at the first non-newline token: does it start a line at or
      // above the function's indentation?
      let lookahead = cursor.index;
      while (cursor.tokens[lookahead]?.kind === "newline") {
        lookahead += 1;
      }
      const nextToken = cursor.tokens[lookahead];
      if (nextToken === undefined) {
        return;
      }
      if (nextToken.column <= funcColumn) {
        return; // body ends before this line
      }
      continue;
    }
    cursor.index += 1;
  }
}

export function extractGDScriptStructure(
  source: string,
  path: string,
  options: { readonly maxDeclarations?: number } = {},
): GDScriptStructure {
  const maxDeclarations = options.maxDeclarations ?? GDSCRIPT_STRUCTURE_LIMITS.maxDeclarations;
  const { tokens, errors: tokenizerErrors } = tokenize(source);
  const cursor: Cursor = { tokens, index: 0 };
  const parserErrors: GDScriptParserError[] = [...tokenizerErrors];
  const signals: GDScriptSignalInfo[] = [];
  const enums: GDScriptEnumInfo[] = [];
  const constants: GDScriptConstantInfo[] = [];
  const properties: GDScriptPropertyInfo[] = [];
  const functions: GDScriptFunctionInfo[] = [];
  const dependencies: string[] = [];
  let extendsType: string | null = null;
  let className: string | null = null;
  const fileAnnotations: GDScriptAnnotationInfo[] = [];
  let pendingAnnotations: GDScriptAnnotationInfo[] = [];
  let truncated = false;
  let declarationCount = 0;

  function recordDeclaration(): boolean {
    if (truncated) {
      return false;
    }
    declarationCount += 1;
    if (declarationCount > maxDeclarations) {
      truncated = true;
      return false;
    }
    return true;
  }

  function attachAnnotations(): readonly string[] {
    const names = pendingAnnotations.map((annotation) => annotation.name);
    pendingAnnotations = [];
    return names;
  }

  function attachPendingToFile(): void {
    for (const annotation of pendingAnnotations) {
      fileAnnotations.push(annotation);
    }
    pendingAnnotations = [];
  }

  skipNewlines(cursor);
  while (peek(cursor) !== null) {
    const token = peek(cursor) as Token;
    if (token.kind === "newline") {
      cursor.index += 1;
      continue;
    }
    if (token.kind === "error") {
      cursor.index += 1;
      continue;
    }
    if (token.kind === "punct" && token.text === "@") {
      cursor.index += 1;
      const nameToken = peek(cursor);
      if (nameToken?.kind === "ident") {
        cursor.index += 1;
        const annotation = collectAnnotation(cursor, nameToken);
        pendingAnnotations.push(annotation);
      } else {
        parserErrors.push({
          line: token.line,
          message: "Annotation without a name.",
        });
        consumeStatement(cursor);
      }
      continue;
    }
    if (token.kind !== "ident") {
      // Ordinary statement; consume to the end of the line (balance-aware).
      if (token.kind === "string") {
        const value = token.value ?? "";
        if (
          value.startsWith("res://") &&
          dependencies.length < GDSCRIPT_STRUCTURE_LIMITS.maxDependencies
        ) {
          dependencies.push(value);
        }
      }
      cursor.index += 1;
      consumeStatement(cursor);
      continue;
    }
    const keyword = token.text;
    const keywordColumn = token.column;
    if (
      keyword === "static" &&
      peek(cursor, 1)?.kind === "ident" &&
      peek(cursor, 1)?.text === "func"
    ) {
      cursor.index += 1; // consume "static"; the func branch records isStatic
      const funcToken = peek(cursor) as Token;
      cursor.index += 1;
      const name = expectIdent(cursor);
      if (name === null) {
        parserErrors.push({
          line: funcToken.line,
          message: "Malformed function declaration (no name).",
        });
        consumeStatement(cursor);
        continue;
      }
      const params = collectParameters(cursor, true);
      if (params.error !== null) {
        parserErrors.push({ line: funcToken.line, message: params.error });
        consumeFunctionBody(cursor, keywordColumn);
        continue;
      }
      let returnType: string | null = null;
      if (peek(cursor)?.text === "-" && peek(cursor, 1)?.text === ">") {
        cursor.index += 2;
        returnType = collectTypeName(cursor);
      }
      const annotations = attachAnnotations();
      if (recordDeclaration()) {
        functions.push({
          name,
          parameters: params.parameters,
          returnType,
          isStatic: true,
          annotations,
          line: funcToken.line,
          multilineSignature: params.multiline,
        });
      }
      consumeFunctionBody(cursor, keywordColumn);
      continue;
    }
    if (keyword === "extends") {
      cursor.index += 1;
      const target = peek(cursor);
      if (target?.kind === "ident" || target?.kind === "string") {
        extendsType = target.kind === "string" ? (target.value ?? "") : target.text;
        cursor.index += 1;
      }
      attachPendingToFile();
      consumeStatement(cursor);
      continue;
    }
    if (keyword === "class_name") {
      cursor.index += 1;
      const name = expectIdent(cursor);
      if (name !== null) {
        className = name;
      }
      attachPendingToFile();
      consumeStatement(cursor);
      continue;
    }
    if (keyword === "signal") {
      cursor.index += 1;
      const name = expectIdent(cursor);
      const params = collectParameters(cursor, false);
      if (params.error !== null) {
        parserErrors.push({ line: token.line, message: params.error });
      } else if (name !== null && recordDeclaration()) {
        signals.push({ name, parameters: params.parameters, line: token.line });
      }
      consumeStatement(cursor);
      continue;
    }
    if (keyword === "enum") {
      cursor.index += 1;
      const name = expectIdent(cursor);
      const members: string[] = [];
      let multiline = false;
      const open = peek(cursor);
      if (open?.text === "{") {
        cursor.index += 1;
        let depth = 1;
        while (true) {
          const member = peek(cursor);
          if (member === null) {
            parserErrors.push({ line: token.line, message: "Unterminated enum block." });
            break;
          }
          if (member.kind === "newline") {
            multiline = true;
            cursor.index += 1;
            continue;
          }
          if (member.kind === "punct" && member.text === "{") {
            depth += 1;
            cursor.index += 1;
            continue;
          }
          if (member.kind === "punct" && member.text === "}") {
            depth -= 1;
            cursor.index += 1;
            if (depth === 0) {
              break;
            }
            continue;
          }
          if (
            member.kind === "ident" &&
            members.length < GDSCRIPT_STRUCTURE_LIMITS.maxEnumMembers
          ) {
            members.push(member.text);
          }
          cursor.index += 1;
        }
      }
      if (recordDeclaration()) {
        enums.push({ name, members, line: token.line, multiline });
      }
      continue;
    }
    if (keyword === "const") {
      cursor.index += 1;
      const name = expectIdent(cursor);
      let type: string | null = null;
      if (peek(cursor)?.text === ":") {
        cursor.index += 1;
        type = collectTypeName(cursor);
      }
      const rest = consumeStatement(cursor);
      if (name !== null && recordDeclaration()) {
        constants.push({ name, type, line: token.line, multiline: rest.multiline });
      }
      continue;
    }
    if (keyword === "var") {
      cursor.index += 1;
      const name = expectIdent(cursor);
      let type: string | null = null;
      if (peek(cursor)?.text === ":") {
        cursor.index += 1;
        type = collectTypeName(cursor);
      }
      const rest = consumeStatement(cursor);
      if (name === null) {
        parserErrors.push({ line: token.line, message: "Malformed variable declaration." });
        pendingAnnotations = [];
        continue;
      }
      const annotations = attachAnnotations();
      if (recordDeclaration()) {
        properties.push({
          name,
          type,
          annotations,
          line: token.line,
          multiline: rest.multiline,
        });
      }
      continue;
    }
    if (keyword === "func") {
      cursor.index += 1;
      let isStatic = false;
      if (peek(cursor)?.text === "static") {
        isStatic = true;
        cursor.index += 1;
      }
      const name = expectIdent(cursor);
      if (name === null) {
        parserErrors.push({
          line: token.line,
          message: "Malformed function declaration (no name).",
        });
        consumeStatement(cursor);
        continue;
      }
      const params = collectParameters(cursor, true);
      if (params.error !== null) {
        parserErrors.push({ line: token.line, message: params.error });
        consumeFunctionBody(cursor, keywordColumn);
        continue;
      }
      let returnType: string | null = null;
      // optional "-> Type" (tokens "-" then ">")
      if (peek(cursor)?.text === "-" && peek(cursor, 1)?.text === ">") {
        cursor.index += 2;
        returnType = collectTypeName(cursor);
      }
      const annotations = attachAnnotations();
      if (recordDeclaration()) {
        functions.push({
          name,
          parameters: params.parameters,
          returnType,
          isStatic,
          annotations,
          line: token.line,
          multilineSignature: params.multiline,
        });
      }
      consumeFunctionBody(cursor, keywordColumn);
      continue;
    }
    if (keyword === "preload" || keyword === "load") {
      cursor.index += 1;
      const open = peek(cursor);
      if (open?.text === "(") {
        cursor.index += 1;
        const argument = peek(cursor);
        const value = argument?.kind === "string" ? (argument.value ?? "") : "";
        if (
          value.startsWith("res://") &&
          !dependencies.includes(value) &&
          dependencies.length < GDSCRIPT_STRUCTURE_LIMITS.maxDependencies
        ) {
          dependencies.push(value);
        }
      }
      consumeStatement(cursor);
      continue;
    }
    // Ordinary statement.
    cursor.index += 1;
    consumeStatement(cursor);
  }

  // Resource references: any res:// string literal is a structural
  // dependency of the file (preload/load calls and direct references).
  // Seed the set with entries already pushed inline so nothing duplicates.
  const dependencySet = new Set<string>(dependencies);
  for (const token of tokens) {
    if (token.kind === "string" && (token.value ?? "").startsWith("res://")) {
      dependencySet.add(token.value as string);
      if (dependencySet.size >= GDSCRIPT_STRUCTURE_LIMITS.maxDependencies) {
        break;
      }
    }
  }
  dependencies.length = 0;
  for (const dependency of dependencySet) {
    dependencies.push(dependency);
  }

  const status: GDScriptStructure["status"] = parserErrors.length === 0 ? "complete" : "partial";
  return {
    path,
    extendsType,
    className,
    fileAnnotations,
    signals,
    enums,
    constants,
    properties,
    functions,
    dependencies,
    status,
    parserErrors,
    truncated,
  };
}
