import {
  canonicalizeJson,
  computeResearchDocumentContentDigest,
  computeResearchDocumentId,
  sha256Hex,
  type ResearchBounds,
  type ResearchContentType,
  type ResearchDocument,
  type ResearchLink,
  type ResearchOutcome,
  type ResearchProvenance,
  type ResearchSection,
  type ResearchSourceRef,
} from "@solaris/core";
import type { TransportOutcome } from "@solaris/core";

/**
 * Research normalization (Stage 3 milestone 5).
 *
 * Pure, bounded conversion of raw fetched bytes (as text) into
 * `ResearchSection`s and full `ResearchDocument`s, shared by every research
 * source adapter. Provider output is untrusted: every normalizer operates on
 * already-bounded input (the transport caps downloads), never allocates
 * unboundedly, caps sections/text/headings, and DISCLOSES every truncation
 * (`truncated` + `reason`, with an explicit "… [truncated]" marker on the
 * truncated text). The final document is additionally capped on its
 * serialized size (`maxDocumentBytes`): trailing sections are dropped (then
 * the first section's text trimmed) until the serialized document fits.
 *
 * HTML extraction is deliberately hand-rolled (no parser dependency): strip
 * script/style blocks, strip tags, decode a minimal entity set, collapse
 * whitespace, and split on `<h1>`-`<h4>` headings. Link extraction is out of
 * scope this milestone (documents carry `links: []`).
 */

export const TRUNCATION_MARKER = "… [truncated]";

export interface NormalizationResult {
  readonly sections: readonly ResearchSection[];
  readonly truncated: boolean;
  readonly reason: string | null;
}

export interface HtmlNormalizationResult extends NormalizationResult {
  /** True when the page yielded no extractable text and no headings. */
  readonly isEmpty: boolean;
}

export interface BuildResearchDocumentOptions {
  readonly source: ResearchSourceRef;
  readonly title: string | null;
  readonly contentType: ResearchContentType;
  readonly rawText: string;
  /** Raw downloaded byte count (recorded by the caller; not part of the document byteLength). */
  readonly rawByteLength: number;
  readonly provenance: ResearchProvenance;
  readonly bounds: ResearchBounds;
  /** Clock value used as the document's `fetchedAtMs` (callers pass the same value into provenance). */
  readonly now: number;
}

/** Fixed document overhead counted in `byteLength`. */
const DOCUMENT_FIXED_OVERHEAD_BYTES = 128;
const PER_SECTION_OVERHEAD_BYTES = 64;
const PER_LINK_OVERHEAD_BYTES = 32;

function byteLengthOf(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * UTF-8-safe byte truncation: never splits a multi-byte character. Returns
 * the whole text when it already fits within `maxBytes`.
 */
function byteSlice(text: string, maxBytes: number): string {
  if (maxBytes <= 0) {
    return "";
  }
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= maxBytes) {
    return text;
  }
  let end = maxBytes;
  // Back off over continuation bytes to the lead byte of the cut character.
  while (end > 0 && (bytes[end]! & 0xc0) === 0x80) {
    end -= 1;
  }
  // If the cut lands exactly on a lead byte, the character is incomplete.
  if (end > 0 && end < bytes.length && (bytes[end]! & 0xc0) === 0xc0) {
    end -= 1;
  }
  return new TextDecoder().decode(bytes.subarray(0, end));
}

/**
 * Cap `text` to `maxBytes`, appending the explicit truncation marker when
 * text was actually cut. The result never exceeds `maxBytes` when the marker
 * fits within the budget (the marker is dropped entirely when it does not).
 */
function truncateWithMarker(text: string, maxBytes: number): string {
  const markerBytes = byteLengthOf(TRUNCATION_MARKER);
  const budget = Math.max(0, maxBytes - markerBytes);
  const cut = byteSlice(text, budget);
  if (cut === text) {
    return text;
  }
  if (budget <= 0) {
    return "";
  }
  return `${cut}${TRUNCATION_MARKER}`;
}

/** Mutable accumulation state shared by the markdown and HTML normalizers. */
interface SectionBuilderState {
  readonly sections: ResearchSection[];
  readonly bounds: ResearchBounds;
  truncated: boolean;
  reason: string | null;
  /** Set once the section cap is hit: further content is dropped. */
  stopped: boolean;
}

/**
 * Push one section under the bounds. Text and heading are capped; when the
 * section cap is hit, the LAST section receives the explicit truncation
 * marker and all further content is dropped (truncated + reason set).
 */
function appendSection(state: SectionBuilderState, heading: string | null, text: string): void {
  if (state.stopped) {
    return;
  }
  if (state.sections.length >= state.bounds.maxSections) {
    state.truncated = true;
    state.reason = "document exceeds the section limit; the last section is truncated";
    const last = state.sections[state.sections.length - 1];
    if (last !== undefined && last.text.length > 0) {
      const markerBytes = byteLengthOf(TRUNCATION_MARKER);
      const room = state.bounds.maxSectionTextBytes - byteLengthOf(last.text);
      if (room >= markerBytes) {
        const textWithMarker = `${last.text}${TRUNCATION_MARKER}`;
        state.sections[state.sections.length - 1] = {
          ...last,
          text: textWithMarker,
          byteLength: byteLengthOf(textWithMarker),
        };
      }
    }
    state.stopped = true;
    return;
  }
  let sectionText = text;
  if (byteLengthOf(sectionText) > state.bounds.maxSectionTextBytes) {
    sectionText = truncateWithMarker(sectionText, state.bounds.maxSectionTextBytes);
    state.truncated = true;
    state.reason = "section text exceeds the byte limit; the section is truncated";
  }
  let sectionHeading = heading;
  if (sectionHeading !== null && byteLengthOf(sectionHeading) > state.bounds.maxHeadingBytes) {
    sectionHeading = byteSlice(sectionHeading, state.bounds.maxHeadingBytes);
    state.truncated = true;
    state.reason = "heading exceeds the byte limit; the heading is truncated";
  }
  state.sections.push({
    heading: sectionHeading,
    text: sectionText,
    byteLength: byteLengthOf(sectionText),
  });
}

/**
 * Markdown → sections. Splits on ATX headings (`^#{1,6} `) and setext
 * underline headings (`^---+` / `^===+`; the previous non-empty line becomes
 * the heading). Sections without headings collapse into one leading section.
 * All bounds are enforced; overflow truncates the last section with the
 * explicit marker.
 */
export function normalizeMarkdownToSections(
  text: string,
  bounds: ResearchBounds,
): NormalizationResult {
  const state: SectionBuilderState = {
    sections: [],
    bounds,
    truncated: false,
    reason: null,
    stopped: false,
  };
  let currentHeading: string | null = null;
  let currentLines: string[] = [];
  const flush = (): void => {
    const body = currentLines.join("\n").trim();
    currentLines = [];
    if (body.length === 0 && currentHeading === null) {
      return;
    }
    appendSection(state, currentHeading, body);
    currentHeading = null;
  };
  for (const rawLine of text.split("\n")) {
    if (state.stopped) {
      break;
    }
    const line = rawLine.trimEnd();
    const headingMatch = /^(#{1,6})\s+(.*)$/.exec(line);
    if (headingMatch !== null) {
      flush();
      const heading = (headingMatch[2] ?? "").trim();
      currentHeading = heading.length === 0 ? null : heading;
      continue;
    }
    if (/^(?:-{3,}|={3,})\s*$/.test(line)) {
      // Setext underline: the previous non-empty line becomes the heading.
      // When the current section already has a heading, the previous line
      // closes that section and starts a new one.
      let index = currentLines.length - 1;
      while (index >= 0 && (currentLines[index] ?? "").trim() === "") {
        index -= 1;
      }
      if (index >= 0) {
        const previous = (currentLines[index] ?? "").trim();
        if (previous.length > 0) {
          currentLines.splice(index, 1);
          if (currentHeading !== null) {
            flush();
          }
          currentHeading = previous;
          continue;
        }
      }
    }
    currentLines.push(line);
  }
  flush();
  return { sections: state.sections, truncated: state.truncated, reason: state.reason };
}

const ENTITY_DECODES: ReadonlyArray<readonly [string, string]> = [
  ["&lt;", "<"],
  ["&gt;", ">"],
  ["&quot;", '"'],
  ["&#39;", "'"],
  ["&apos;", "'"],
  ["&nbsp;", " "],
  // &amp; last so decoded entities are never double-decoded.
  ["&amp;", "&"],
];

function decodeEntities(text: string): string {
  let out = text;
  for (const [entity, replacement] of ENTITY_DECODES) {
    out = out.split(entity).join(replacement);
  }
  return out;
}

/** Strip comments and tags, decode entities, collapse whitespace, trim. */
function extractHtmlText(raw: string): string {
  const withoutTags = raw.replace(/<!--[\s\S]*?-->/g, " ").replace(/<[^>]*>/g, " ");
  return decodeEntities(withoutTags)
    .replace(/\s+/g, " ")
    .replace(/\s+([,.;:!?])/g, "$1")
    .trim();
}

/**
 * HTML → sections. Bounded hand-rolled extraction: script/style blocks are
 * stripped first, then tags, then a minimal entity set is decoded and
 * whitespace collapsed. Sections split on `<h1>`-`<h4>` headings. A page
 * with no extractable text and no headings reports `isEmpty: true` so the
 * source can fail closed with `unsupported-content`.
 */
export function normalizeHtmlToSections(
  html: string,
  bounds: ResearchBounds,
): HtmlNormalizationResult {
  const work = html
    .replace(/<script\b[^>]*>[\s\S]*?<\/script\s*>/gi, " ")
    .replace(/<style\b[^>]*>[\s\S]*?<\/style\s*>/gi, " ");
  const state: SectionBuilderState = {
    sections: [],
    bounds,
    truncated: false,
    reason: null,
    stopped: false,
  };
  let currentHeading: string | null = null;
  let pendingBody: string[] = [];
  const appendBody = (fragment: string): void => {
    if (fragment.length > 0) {
      pendingBody.push(fragment);
    }
  };
  const flushSection = (): void => {
    const body = pendingBody.join(" ").trim();
    pendingBody = [];
    if (body.length === 0 && currentHeading === null) {
      return;
    }
    appendSection(state, currentHeading, body);
    currentHeading = null;
  };
  const headingPattern = /<h([1-4])\b[^>]*>([\s\S]*?)<\/h\1\s*>/gi;
  let cursor = 0;
  for (let match = headingPattern.exec(work); match !== null; match = headingPattern.exec(work)) {
    if (state.stopped) {
      break;
    }
    appendBody(extractHtmlText(work.slice(cursor, match.index)));
    const headingText = extractHtmlText(match[2] ?? "");
    flushSection();
    if (headingText.length > 0) {
      currentHeading = headingText;
    }
    cursor = match.index + match[0].length;
  }
  if (!state.stopped) {
    appendBody(extractHtmlText(work.slice(cursor)));
    flushSection();
  }
  return {
    sections: state.sections,
    truncated: state.truncated,
    reason: state.reason,
    isEmpty: state.sections.length === 0,
  };
}

/**
 * JSON → one section (no title). Narrow, documented field extraction: when
 * the top-level value is an object with a non-empty string `body` (or
 * `description` when `body` is absent), that field's text is used; otherwise
 * the value is rendered as a bounded pretty-printed excerpt. Invalid JSON
 * falls back to the raw (bounded) text. Capped by `maxSectionTextBytes`.
 */
export function normalizeJsonToSections(json: string, bounds: ResearchBounds): NormalizationResult {
  let parsed: unknown = null;
  try {
    parsed = JSON.parse(json);
  } catch {
    parsed = null;
  }
  let text: string;
  if (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)) {
    const record = parsed as Record<string, unknown>;
    const body = record["body"];
    const description = record["description"];
    if (typeof body === "string" && body.trim().length > 0) {
      text = body;
    } else if (typeof description === "string" && description.trim().length > 0) {
      text = description;
    } else {
      text = JSON.stringify(parsed, null, 2);
    }
  } else {
    text = parsed === null ? json : JSON.stringify(parsed, null, 2);
  }
  if (byteLengthOf(text) > bounds.maxSectionTextBytes) {
    const capped = truncateWithMarker(text, bounds.maxSectionTextBytes);
    return {
      sections: [{ heading: null, text: capped, byteLength: byteLengthOf(capped) }],
      truncated: true,
      reason: "the JSON excerpt exceeds the byte limit; it is truncated",
    };
  }
  return {
    sections: [{ heading: null, text, byteLength: byteLengthOf(text) }],
    truncated: false,
    reason: null,
  };
}

/** text/plain → one untitled section, capped by `maxSectionTextBytes`. */
function normalizePlainToSections(text: string, bounds: ResearchBounds): NormalizationResult {
  if (byteLengthOf(text) > bounds.maxSectionTextBytes) {
    const capped = truncateWithMarker(text, bounds.maxSectionTextBytes);
    return {
      sections: [{ heading: null, text: capped, byteLength: byteLengthOf(capped) }],
      truncated: true,
      reason: "the text exceeds the byte limit; it is truncated",
    };
  }
  return {
    sections: [{ heading: null, text, byteLength: byteLengthOf(text) }],
    truncated: false,
    reason: null,
  };
}

/**
 * Content-type allowlist. Accepts prefix matches with parameters
 * (`text/html; charset=utf-8` → `text/html`). Anything else — archives,
 * executables, video, PDF, `application/octet-stream` — returns null so the
 * caller fails closed with `unsupported-content`.
 */
export function classifyContentType(raw: string | null): ResearchContentType | null {
  if (raw === null) {
    return null;
  }
  const base = (raw.split(";", 1)[0] ?? "").trim().toLowerCase();
  switch (base) {
    case "text/markdown":
      return "text/markdown";
    case "text/plain":
      return "text/plain";
    case "application/json":
      return "application/json";
    case "text/html":
      return "text/html";
    default:
      return null;
  }
}

function computeByteLength(sections: readonly ResearchSection[], linkCount: number): number {
  let total = DOCUMENT_FIXED_OVERHEAD_BYTES;
  for (const section of sections) {
    total +=
      section.byteLength +
      (section.heading === null ? 0 : byteLengthOf(section.heading)) +
      PER_SECTION_OVERHEAD_BYTES;
  }
  total += linkCount * PER_LINK_OVERHEAD_BYTES;
  return total;
}

/**
 * Build a full `ResearchDocument` from raw fetched text: applies the
 * normalizer for the content type, computes the document id from
 * `sha256(source.id, resource + requestedRef)`, and enforces the final
 * `maxDocumentBytes` cap on the serialized document (drop trailing sections,
 * then trim the first section's text, with an explicit truncation reason).
 */
export function buildResearchDocument(options: BuildResearchDocumentOptions): ResearchDocument {
  const { source, title, contentType, rawText, provenance, bounds, now } = options;
  let normalized: NormalizationResult;
  switch (contentType) {
    case "text/markdown":
      normalized = normalizeMarkdownToSections(rawText, bounds);
      break;
    case "text/html":
      normalized = normalizeHtmlToSections(rawText, bounds);
      break;
    case "application/json":
      normalized = normalizeJsonToSections(rawText, bounds);
      break;
    case "text/plain":
      normalized = normalizePlainToSections(rawText, bounds);
      break;
  }
  const sections = [...normalized.sections];
  const truncated = normalized.truncated;
  const truncationReason = normalized.reason;
  // Links are not extracted this milestone.
  const links: readonly ResearchLink[] = [];

  const requestDigest = sha256Hex(
    canonicalizeJson({ resource: provenance.resource, requestedRef: provenance.requestedRef }),
  );
  const id = computeResearchDocumentId(source.id, requestDigest);

  let document: ResearchDocument = {
    id,
    source,
    title,
    fetchedAtMs: now,
    contentType,
    sections,
    links,
    provenance,
    truncated,
    truncationReason,
    byteLength: computeByteLength(sections, links.length),
    // Content identity (ADR 0028), recomputed over the final content at
    // the end; the derived fields are inside the measured document cap.
    contentDigest: computeResearchDocumentContentDigest({
      title,
      contentType,
      sections,
    }),
    rawArtifactDigest: sha256Hex(rawText),
  };

  // Final `maxDocumentBytes` cap on the SERIALIZED document: drop trailing
  // sections until it fits, then trim the first section's text (with the
  // explicit marker) until it fits. Bounded: the drop loop runs at most
  // maxSections times; the trim loop shrinks 64 bytes at a time (guarded).
  // The byte cap governs CONTENT bytes (matching `byteLength` semantics);
  // the derived identity fields are bounded metadata and are excluded from
  // the measurement, consistent with how the evidence store accounts
  // content bytes.
  if (measuredBytes(document) > bounds.maxDocumentBytes) {
    const withSections = (next: readonly ResearchSection[]): ResearchDocument => ({
      ...document,
      sections: next,
      byteLength: computeByteLength(next, document.links.length),
    });
    let dropped = 0;
    while (document.sections.length > 1 && measuredBytes(document) > bounds.maxDocumentBytes) {
      document = withSections(document.sections.slice(0, -1));
      dropped += 1;
    }
    if (dropped > 0) {
      document = {
        ...document,
        truncated: true,
        truncationReason: "the document exceeds the byte limit; trailing sections were dropped",
      };
    }
    if (measuredBytes(document) > bounds.maxDocumentBytes) {
      const first = document.sections[0];
      if (first !== undefined && first.text.length > 0) {
        const reason = "the document exceeds the byte limit; the section text was truncated";
        // The candidate must carry the final truncation flags: the reason
        // string itself occupies bytes, so measuring without it would
        // over-commit the cap.
        const candidateFinal = (text: string): ResearchDocument => ({
          ...withSections([{ heading: first.heading, text, byteLength: byteLengthOf(text) }]),
          truncated: true,
          truncationReason: reason,
        });
        let text = first.text;
        let guard = 0;
        while (
          measuredBytes(candidateFinal(text)) > bounds.maxDocumentBytes &&
          text.length > 0 &&
          guard < 4096
        ) {
          text = byteSlice(text, Math.max(0, byteLengthOf(text) - 64));
          guard += 1;
        }
        const marked = `${text}${TRUNCATION_MARKER}`;
        document = candidateFinal(
          measuredBytes(candidateFinal(marked)) <= bounds.maxDocumentBytes ? marked : text,
        );
      }
    }
  }
  // Content identity (ADR 0028): recompute the digest over the exact
  // FINAL normalized content (raw digest is content-independent).
  return {
    ...document,
    contentDigest: computeResearchDocumentContentDigest(document),
  };
}

/** Serialized content bytes of a document, excluding derived identity fields. */
function measuredBytes(document: ResearchDocument): number {
  const {
    contentDigest: _contentDigest,
    rawArtifactDigest: _rawArtifactDigest,
    ...content
  } = document;
  return JSON.stringify(content).length;
}

/**
 * Bound an unexpected error message (never a raw stack trace) for a typed
 * `failed` outcome.
 */
export function boundedErrorMessage(error: unknown, maxBytes = 200): string {
  const message = error instanceof Error ? error.message : String(error);
  const collapsed = message.replace(/\s+/g, " ").trim();
  if (byteLengthOf(collapsed) <= maxBytes) {
    return collapsed;
  }
  return `${byteSlice(collapsed, Math.max(0, maxBytes - 3))}...`;
}

/** Map every non-ok transport outcome to the corresponding research outcome. */
export function transportErrorToResearchOutcome(
  outcome: Exclude<TransportOutcome, { readonly status: "ok" }>,
): ResearchOutcome {
  switch (outcome.status) {
    case "refused":
      return { status: "refused", reason: outcome.reason };
    case "timeout":
      return { status: "timeout" };
    case "cancelled":
      return { status: "cancelled" };
    case "oversized":
      return { status: "oversized", reason: outcome.reason };
    case "unsupported-content":
      return { status: "unsupported-content", reason: outcome.reason };
    case "failed":
      return { status: "failed", reason: outcome.reason };
  }
}

export interface ResearchDocumentOutcomeOptions {
  readonly source: ResearchSourceRef;
  readonly title: string | null;
  /** Provenance fields except `source`/`fetchedAtMs` (both are filled here). */
  readonly provenance: Omit<ResearchProvenance, "source" | "fetchedAtMs">;
  readonly bounds: ResearchBounds;
  readonly now: number;
}

/**
 * Build a `document` outcome from a successful transport exchange. HTML
 * pages with no extractable text fail closed as `unsupported-content`.
 */
export function researchDocumentOutcome(
  outcome: Extract<TransportOutcome, { readonly status: "ok" }>,
  options: ResearchDocumentOutcomeOptions,
): ResearchOutcome {
  const provenance: ResearchProvenance = {
    ...options.provenance,
    source: options.source,
    fetchedAtMs: options.now,
  };
  const rawText = new TextDecoder().decode(outcome.bytes);
  const classified = classifyContentType(outcome.contentType);
  if (classified === null) {
    return {
      status: "unsupported-content",
      reason: `unsupported content type: ${outcome.contentType ?? "unknown"}`,
    };
  }
  const document = buildResearchDocument({
    source: options.source,
    title: options.title,
    contentType: classified,
    rawText,
    rawByteLength: outcome.bytes.length,
    provenance,
    bounds: options.bounds,
    now: options.now,
  });
  if (document.contentType === "text/html" && document.sections.length === 0) {
    return {
      status: "unsupported-content",
      reason: "the fetched page contains no extractable text",
    };
  }
  return { status: "document", document };
}
