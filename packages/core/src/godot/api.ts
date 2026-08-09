/**
 * Provider-neutral Godot API symbol model.
 *
 * Core owns the symbol model, deterministic symbol identities, the query
 * model, and the search/lookup result models. Parsing the engine-generated
 * dump, building the bounded index, storing it, and querying it are
 * adapter-owned; providers receive only bounded structured results.
 */

export type GodotApiSymbolKind =
  "class" | "method" | "property" | "signal" | "constant" | "enum" | "utility" | "operator";

/** Whether the symbol comes from a native engine class or a built-in class. */
export type GodotApiType = "native" | "builtin";

/** One parameter of a method, signal, or utility function. */
export interface GodotApiParameter {
  readonly name: string;
  readonly type: string;
  /** Default-argument expression text when the dump provides one. */
  readonly defaultValue: string | null;
}

/**
 * Bounded structured details for one symbol, filled conservatively from the
 * engine dump. Unknown dump fields are tolerated and never fail the build.
 */
export interface GodotApiSymbolDetails {
  /** Method/utility return type (`void`, `int`, `Node`, ...). */
  readonly returnType?: string | undefined;
  readonly parameters?: readonly GodotApiParameter[] | undefined;
  /** Qualifiers such as `static`, `vararg`, or `const`. */
  readonly qualifiers?: readonly string[] | undefined;
  /** Engine-provided method hash when available. */
  readonly hash?: string | undefined;
  /** Property type. */
  readonly type?: string | undefined;
  /** Property setter name (unset means not settable). */
  readonly setter?: string | null | undefined;
  /** Property getter name. */
  readonly getter?: string | null | undefined;
  /** Constant/enum value when representable. */
  readonly value?: string | undefined;
  /** Enum member values. */
  readonly values?: readonly { readonly name: string; readonly value: string }[] | undefined;
}

/** One bounded indexed API symbol. Engine-native names are preserved exactly. */
export interface GodotApiSymbol {
  /** Deterministic symbol identity, e.g. `class:Node/method:add_child`. */
  readonly id: string;
  readonly kind: GodotApiSymbolKind;
  /** Engine-native symbol name, never rewritten. */
  readonly name: string;
  /** Owning class for members; null for globals and utility functions. */
  readonly owner: string | null;
  readonly apiType: GodotApiType;
  /** Bounded first-line summary derived from the description. */
  readonly summary: string;
  /** Bounded full description; null when the dump provides none. */
  readonly description: string | null;
  /** Canonical signature text for methods, utilities, and operators. */
  readonly signature: string | null;
  /** Class this member is inherited from, when known. */
  readonly inheritedFrom: string | null;
  /** Overload disambiguation ordinal (1-based) when the id carries one. */
  readonly ordinal: number | null;
  readonly details: GodotApiSymbolDetails;
}

/**
 * Immutable searchable API index. The adapter builds, stores, and queries
 * it; the provider can never request the raw index files or the raw dump.
 */
export interface GodotApiIndex {
  readonly schemaVersion: number;
  /** Exact engine version string from the dump header. */
  readonly engineVersion: string;
  /** Dump SHA-256 the index was built from. */
  readonly dumpSha256: string;
  /** All symbols sorted by id (deterministic). */
  readonly symbols: readonly GodotApiSymbol[];
  /** Total raw dump bytes the index was built from (bounded). */
  readonly dumpBytes: number;
}

/**
 * Deterministic symbol identity. Examples:
 *
 * ```text
 * class:Node
 * class:Node/method:add_child
 * class:Node/property:owner
 * class:Node/signal:ready
 * class:Node/constant:NOTIFICATION_READY
 * class:Node/enum:ProcessMode
 * class:Vector2/method:length
 * class:Vector2/operator:+
 * global:enum:Error
 * global:constant:PI
 * utility:lerp
 * ```
 *
 * Identities are stable for the same API dump, contain no filesystem paths
 * and no provider-specific ids, and never assume method names alone are
 * globally unique. The index builder appends a deterministic `#N` ordinal
 * when the dump's metadata requires overload disambiguation.
 */
export function godotSymbolId(parts: {
  readonly kind: GodotApiSymbolKind;
  readonly name: string;
  readonly owner?: string | null;
  readonly ordinal?: number;
}): string {
  const { kind, name, owner } = parts;
  const ordinal = parts.ordinal !== undefined && parts.ordinal > 1 ? `#${parts.ordinal}` : "";
  if (kind === "utility") {
    return `utility:${name}${ordinal}`;
  }
  if (kind === "constant" || kind === "enum") {
    if (owner === undefined || owner === null || owner === "") {
      return `global:${kind}:${name}${ordinal}`;
    }
    return `class:${owner}/${kind}:${name}${ordinal}`;
  }
  if (kind === "class" || kind === "operator") {
    if (kind === "class") {
      return `class:${name}${ordinal}`;
    }
    if (owner === undefined || owner === null || owner === "") {
      return `operator:${name}${ordinal}`;
    }
    return `class:${owner}/operator:${name}${ordinal}`;
  }
  if (owner === undefined || owner === null || owner === "") {
    return `${kind}:${name}${ordinal}`;
  }
  return `class:${owner}/${kind}:${name}${ordinal}`;
}

/** Search kind filter values accepted by `godot.api_search`. */
export type GodotApiSearchKind = GodotApiSymbolKind;

export interface GodotApiSearchQuery {
  /** Required query text; literal/token search only, no embeddings. */
  readonly query: string;
  /** Optional kind filter; absent means every kind. */
  readonly kinds?: readonly GodotApiSearchKind[];
  /** Optional result bound; capped by the immutable global limit. */
  readonly limit?: number;
}

/** Rank tiers: exact name first, then prefix, then token, then document. */
export type GodotApiSearchRank = "exact" | "prefix" | "token" | "document";

export interface GodotApiSearchResult {
  readonly symbol: string;
  readonly kind: GodotApiSymbolKind;
  readonly name: string;
  readonly owner: string | null;
  readonly summary: string;
  readonly rank: GodotApiSearchRank;
  readonly apiType: GodotApiType;
}

export interface GodotApiSearchOutcome {
  readonly results: readonly GodotApiSearchResult[];
  readonly truncated: boolean;
}

export interface GodotApiLookupResult {
  readonly symbol: string;
  readonly kind: GodotApiSymbolKind;
  readonly name: string;
  readonly owner: string | null;
  readonly inheritedFrom: string | null;
  readonly signature: string | null;
  readonly description: string | null;
  readonly apiType: GodotApiType;
  readonly details: GodotApiSymbolDetails;
}
