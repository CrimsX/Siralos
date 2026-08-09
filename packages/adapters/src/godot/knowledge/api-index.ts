import {
  GODOT_LIMITS,
  godotSymbolId,
  type GodotApiIndex,
  type GodotApiLookupResult,
  type GodotApiSearchKind,
  type GodotApiSearchOutcome,
  type GodotApiSearchRank,
  type GodotApiSearchResult,
  type GodotApiSymbol,
  type GodotApiSymbolDetails,
} from "@solaris/core";
import { truncateUtf8Bytes } from "./api-dump-with-docs.js";
import type { GodotApiDumpDocument } from "./api-dump-with-docs.js";

/**
 * Bounded API index builder plus literal/token search and exact lookup.
 *
 * The index is built from the exact engine-generated dump: engine-native
 * names are preserved, symbol identities are deterministic, limits are
 * enforced at build time (excess classes or symbols fail safely), and every
 * description is truncated to the immutable bound. The provider can never
 * request the raw dump or the raw index files; it receives only bounded
 * structured search and lookup results.
 */

export type GodotApiIndexBuildResult =
  | { readonly ok: true; readonly index: GodotApiIndex }
  | { readonly ok: false; readonly message: string };

export function buildGodotApiIndex(document: GodotApiDumpDocument): GodotApiIndexBuildResult {
  if (document.rawBytes > GODOT_LIMITS.maxApiDumpWithDocsBytes) {
    return {
      ok: false,
      message: `The API documentation dump is ${document.rawBytes} bytes, exceeding the ${GODOT_LIMITS.maxApiDumpWithDocsBytes}-byte bound.`,
    };
  }
  const classCount = document.classes.length + document.builtinClasses.length;
  if (classCount > GODOT_LIMITS.maxApiClasses) {
    return {
      ok: false,
      message: `The API dump declares ${classCount} classes, exceeding the ${GODOT_LIMITS.maxApiClasses}-class bound.`,
    };
  }
  const symbols: GodotApiSymbol[] = [];
  const usedIds = new Set<string>();

  function addSymbol(
    kind: GodotApiSymbol["kind"],
    name: string,
    owner: string | null,
    apiType: GodotApiSymbol["apiType"],
    description: string | null,
    inheritedFrom: string | null,
    signature: string | null,
    details: GodotApiSymbolDetails,
  ): boolean {
    let ordinal = 1;
    let id = godotSymbolId({ kind, name, owner });
    while (usedIds.has(id)) {
      ordinal += 1;
      id = godotSymbolId({ kind, name, owner, ordinal });
    }
    if (symbols.length >= GODOT_LIMITS.maxApiSymbols) {
      return false;
    }
    usedIds.add(id);
    symbols.push({
      id,
      kind,
      name,
      owner,
      apiType,
      summary: summarize(description),
      description,
      signature,
      inheritedFrom,
      ordinal: ordinal > 1 ? ordinal : null,
      details,
    });
    return true;
  }

  for (const godotClass of document.classes) {
    if (
      !addSymbol(
        "class",
        godotClass.name,
        null,
        "native",
        firstOf(godotClass.briefDescription, godotClass.description),
        godotClass.baseClass,
        null,
        {},
      )
    ) {
      return symbolLimitFailure();
    }
    for (const method of godotClass.methods) {
      if (
        !addSymbol(
          "method",
          method.name,
          godotClass.name,
          "native",
          method.description,
          null,
          methodSignature(method.name, method),
          {
            returnType: method.returnType ?? undefined,
            parameters: method.parameters,
            qualifiers: method.qualifiers,
            hash: method.hash ?? undefined,
          },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const property of godotClass.properties) {
      if (
        !addSymbol(
          "property",
          property.name,
          godotClass.name,
          "native",
          property.description,
          null,
          property.type === null ? null : `${property.name}: ${property.type}`,
          {
            type: property.type ?? undefined,
            setter: property.setter,
            getter: property.getter,
          },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const signal of godotClass.signals) {
      if (
        !addSymbol(
          "signal",
          signal.name,
          godotClass.name,
          "native",
          signal.description,
          null,
          signalSignature(signal.name, signal),
          { parameters: signal.parameters },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const constant of godotClass.constants) {
      if (
        !addSymbol(
          "constant",
          constant.name,
          godotClass.name,
          "native",
          constant.description,
          null,
          constant.value === null ? null : `${constant.name} = ${constant.value}`,
          { value: constant.value ?? undefined },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const entry of godotClass.enums) {
      if (
        !addSymbol("enum", entry.name, godotClass.name, "native", entry.description, null, null, {
          values: entry.values,
        })
      ) {
        return symbolLimitFailure();
      }
    }
  }

  for (const builtin of document.builtinClasses) {
    if (!addSymbol("class", builtin.name, null, "builtin", builtin.description, null, null, {})) {
      return symbolLimitFailure();
    }
    for (const method of builtin.methods) {
      if (
        !addSymbol(
          "method",
          method.name,
          builtin.name,
          "builtin",
          method.description,
          null,
          methodSignature(method.name, method),
          {
            returnType: method.returnType ?? undefined,
            parameters: method.parameters,
            qualifiers: method.qualifiers,
            hash: method.hash ?? undefined,
          },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const operator of builtin.operators) {
      if (
        !addSymbol(
          "operator",
          operator.name,
          builtin.name,
          "builtin",
          null,
          null,
          operator.name,
          {},
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const constant of builtin.constants) {
      if (
        !addSymbol(
          "constant",
          constant.name,
          builtin.name,
          "builtin",
          constant.description,
          null,
          constant.value === null ? null : `${constant.name} = ${constant.value}`,
          { value: constant.value ?? undefined },
        )
      ) {
        return symbolLimitFailure();
      }
    }
    for (const entry of builtin.enums) {
      if (
        !addSymbol("enum", entry.name, builtin.name, "builtin", entry.description, null, null, {
          values: entry.values,
        })
      ) {
        return symbolLimitFailure();
      }
    }
  }

  for (const constant of document.globalConstants) {
    if (
      !addSymbol(
        "constant",
        constant.name,
        null,
        "native",
        constant.description,
        null,
        constant.value === null ? null : `${constant.name} = ${constant.value}`,
        { value: constant.value ?? undefined },
      )
    ) {
      return symbolLimitFailure();
    }
  }
  for (const entry of document.globalEnums) {
    if (
      !addSymbol("enum", entry.name, null, "native", entry.description, null, null, {
        values: entry.values,
      })
    ) {
      return symbolLimitFailure();
    }
  }
  for (const utility of document.utilityFunctions) {
    if (
      !addSymbol(
        "utility",
        utility.name,
        null,
        "native",
        utility.description,
        null,
        methodSignature(utility.name, utility),
        {
          returnType: utility.returnType ?? undefined,
          parameters: utility.parameters,
          qualifiers: utility.qualifiers,
          hash: utility.hash ?? undefined,
        },
      )
    ) {
      return symbolLimitFailure();
    }
  }

  symbols.sort((left, right) => (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
  const index: GodotApiIndex = {
    schemaVersion: GODOT_LIMITS.knowledgeSchemaVersion,
    engineVersion: document.versionFullName ?? "unknown",
    dumpSha256: document.sha256,
    symbols,
    dumpBytes: document.rawBytes,
  };
  return { ok: true, index };
}

function symbolLimitFailure(): { readonly ok: false; readonly message: string } {
  return {
    ok: false,
    message: `The API dump expands beyond the ${GODOT_LIMITS.maxApiSymbols}-symbol bound; the index build failed safely.`,
  };
}

export interface GodotApiIndexSearchOptions {
  readonly kinds?: readonly GodotApiSearchKind[];
  readonly limit?: number;
}

/**
 * Literal/token search. Ranking is deterministic: exact name matches first,
 * then prefix matches, then token matches, then document matches; ties are
 * broken by name length and then symbol id. Case-insensitive matching is
 * accepted; no embeddings, no internet, no fuzzy dependency.
 */
export function searchGodotApiIndex(
  index: GodotApiIndex,
  query: string,
  options: GodotApiIndexSearchOptions = {},
): GodotApiSearchOutcome {
  const normalizedQuery = query.trim().toLowerCase();
  const tokens = normalizedQuery.split(/[^a-z0-9_]+/).filter((token) => token.length > 0);
  const kindFilter = options.kinds;
  const limit = Math.min(
    options.limit ?? GODOT_LIMITS.maxApiSearchResults,
    GODOT_LIMITS.maxApiSearchResults,
  );
  const ranked: { readonly symbol: GodotApiSymbol; readonly rank: GodotApiSearchRank }[] = [];
  for (const symbol of index.symbols) {
    if (kindFilter !== undefined && kindFilter.length > 0 && !kindFilter.includes(symbol.kind)) {
      continue;
    }
    const name = symbol.name.toLowerCase();
    const rank = rankSymbol(name, tokens, symbol);
    if (rank !== null) {
      ranked.push({ symbol, rank });
    }
  }
  ranked.sort((left, right) => {
    if (left.rank !== right.rank) {
      return rankOrder(left.rank) - rankOrder(right.rank);
    }
    const nameLength = left.symbol.name.length - right.symbol.name.length;
    if (nameLength !== 0) {
      return nameLength;
    }
    return left.symbol.id < right.symbol.id ? -1 : left.symbol.id > right.symbol.id ? 1 : 0;
  });
  const truncated = ranked.length > limit;
  const results: GodotApiSearchResult[] = ranked.slice(0, limit).map((entry) => ({
    symbol: entry.symbol.id,
    kind: entry.symbol.kind,
    name: entry.symbol.name,
    owner: entry.symbol.owner,
    summary: entry.symbol.summary,
    rank: entry.rank,
    apiType: entry.symbol.apiType,
  }));
  return { results, truncated };
}

/** Exact-symbol lookup; unknown symbols return null (structured not-found). */
export function lookupGodotApiSymbol(
  index: GodotApiIndex,
  symbolId: string,
): GodotApiLookupResult | null {
  for (const symbol of index.symbols) {
    if (symbol.id === symbolId) {
      return {
        symbol: symbol.id,
        kind: symbol.kind,
        name: symbol.name,
        owner: symbol.owner,
        inheritedFrom: symbol.inheritedFrom,
        signature: symbol.signature,
        description: symbol.description,
        apiType: symbol.apiType,
        details: symbol.details,
      };
    }
  }
  return null;
}

function rankSymbol(
  name: string,
  tokens: readonly string[],
  symbol: GodotApiSymbol,
): GodotApiSearchRank | null {
  const query = tokens.join(" ");
  if (query.length === 0) {
    return null;
  }
  if (name === query) {
    return "exact";
  }
  if (name.startsWith(query)) {
    return "prefix";
  }
  if (tokens.some((token) => name.includes(token))) {
    return "token";
  }
  const document = `${symbol.summary} ${symbol.description ?? ""}`.toLowerCase();
  if (tokens.every((token) => document.includes(token))) {
    return "document";
  }
  return null;
}

function rankOrder(rank: GodotApiSearchRank): number {
  switch (rank) {
    case "exact":
      return 0;
    case "prefix":
      return 1;
    case "token":
      return 2;
    case "document":
      return 3;
  }
}

function methodSignature(
  name: string,
  method: {
    readonly returnType: string | null;
    readonly parameters: readonly { name: string; type: string; defaultValue: string | null }[];
    readonly qualifiers: readonly string[];
  },
): string {
  const qualifiers = method.qualifiers.filter(
    (qualifier) => qualifier === "static" || qualifier === "vararg",
  );
  const argumentsText = method.parameters
    .map((parameter) =>
      parameter.defaultValue === null
        ? `${parameter.name}: ${parameter.type}`
        : `${parameter.name}: ${parameter.type} := ${parameter.defaultValue}`,
    )
    .join(", ");
  const prefix = qualifiers.length > 0 ? `${qualifiers.join(" ")} ` : "";
  const returnType = method.returnType ?? "void";
  return `${prefix}${name}(${argumentsText}) -> ${returnType}`;
}

function signalSignature(
  name: string,
  signal: { readonly parameters: readonly { name: string; type: string }[] },
): string {
  const argumentsText = signal.parameters
    .map((parameter) => `${parameter.name}: ${parameter.type}`)
    .join(", ");
  return `${name}(${argumentsText})`;
}

function firstOf(...values: readonly (string | null)[]): string | null {
  for (const value of values) {
    if (value !== null && value.length > 0) {
      return value;
    }
  }
  return null;
}

function summarize(description: string | null): string {
  if (description === null) {
    return "";
  }
  const firstLine = description.split(/\r?\n/)[0] ?? "";
  return truncateUtf8Bytes(firstLine.trim(), GODOT_LIMITS.maxApiSummaryBytes);
}
