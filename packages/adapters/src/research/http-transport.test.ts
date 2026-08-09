import { describe, expect, it } from "vitest";
import { createFakeTransport, createNodeHttpsTransport } from "./http-transport.js";

const DEFAULT_OPTIONS = {
  maxBytes: 1024,
  maxRedirects: 4,
  timeoutMs: 1000,
  signal: new AbortController().signal,
};

function aborted(): AbortSignal {
  const controller = new AbortController();
  controller.abort();
  return controller.signal;
}

describe("createFakeTransport", () => {
  it("serves a route with classified content type", async () => {
    const transport = createFakeTransport({
      "https://example.com/doc.md": { body: "# Hi", contentType: "text/markdown; charset=utf-8" },
    });
    const outcome = await transport.get("https://example.com/doc.md", DEFAULT_OPTIONS);
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.statusCode).toBe(200);
      expect(outcome.contentType).toBe("text/markdown");
      expect(new TextDecoder().decode(outcome.bytes)).toBe("# Hi");
    }
  });

  it("defaults the status code to 200 and content type to text/markdown", async () => {
    const transport = createFakeTransport({ "https://example.com/a": { body: "x" } });
    const outcome = await transport.get("https://example.com/a", DEFAULT_OPTIONS);
    expect(outcome.status).toBe("ok");
    if (outcome.status === "ok") {
      expect(outcome.statusCode).toBe(200);
      expect(outcome.contentType).toBe("text/markdown");
    }
  });

  it("fails with no route for unknown URLs", async () => {
    const transport = createFakeTransport({});
    const outcome = await transport.get("https://example.com/unknown", DEFAULT_OPTIONS);
    expect(outcome).toEqual({ status: "failed", reason: "no route" });
  });

  it("refuses non-https URLs before any route lookup", async () => {
    const transport = createFakeTransport({ "http://example.com/a": { body: "x" } });
    expect(await transport.get("http://example.com/a", DEFAULT_OPTIONS)).toEqual({
      status: "refused",
      reason: "https only",
    });
    expect(await transport.get("ftp://example.com/a", DEFAULT_OPTIONS)).toEqual({
      status: "refused",
      reason: "only https URLs are supported",
    });
  });

  it("returns oversized when the body exceeds maxBytes", async () => {
    const transport = createFakeTransport({ "https://example.com/big": { body: "x".repeat(100) } });
    const outcome = await transport.get("https://example.com/big", {
      ...DEFAULT_OPTIONS,
      maxBytes: 50,
    });
    expect(outcome.status).toBe("oversized");
    if (outcome.status === "oversized") {
      expect(outcome.reason).toContain("download limit");
    }
  });

  it("follows redirects up to maxRedirects and fails beyond", async () => {
    const transport = createFakeTransport({
      "https://example.com/a": { redirectsTo: "/b" },
      "https://example.com/b": { redirectsTo: "https://example.com/c" },
      "https://example.com/c": { body: "done" },
    });
    const chain = await transport.get("https://example.com/a", {
      ...DEFAULT_OPTIONS,
      maxRedirects: 2,
    });
    expect(chain.status).toBe("ok");
    if (chain.status === "ok") {
      expect(new TextDecoder().decode(chain.bytes)).toBe("done");
    }
    const capped = await transport.get("https://example.com/a", {
      ...DEFAULT_OPTIONS,
      maxRedirects: 1,
    });
    expect(capped).toEqual({ status: "failed", reason: "too many redirects" });
  });

  it("returns timeout when the route latency exceeds timeoutMs", async () => {
    const transport = createFakeTransport({
      "https://example.com/slow": { body: "x", delayMs: 30 },
    });
    const outcome = await transport.get("https://example.com/slow", {
      ...DEFAULT_OPTIONS,
      timeoutMs: 10,
    });
    expect(outcome.status).toBe("timeout");
  });

  it("honors an already-aborted signal", async () => {
    const transport = createFakeTransport({ "https://example.com/a": { body: "x" } });
    expect(
      await transport.get("https://example.com/a", { ...DEFAULT_OPTIONS, signal: aborted() }),
    ).toEqual({
      status: "cancelled",
    });
  });

  it("checks the signal after the route delay", async () => {
    const transport = createFakeTransport({
      "https://example.com/slow": { body: "x", delayMs: 20 },
    });
    const controller = new AbortController();
    const pending = transport.get("https://example.com/slow", {
      ...DEFAULT_OPTIONS,
      timeoutMs: 1000,
      signal: controller.signal,
    });
    controller.abort();
    expect(await pending).toEqual({ status: "cancelled" });
  });

  it("returns unsupported-content for unclassifiable content types", async () => {
    const transport = createFakeTransport({
      "https://example.com/bin": { body: "x", contentType: "application/octet-stream" },
    });
    const outcome = await transport.get("https://example.com/bin", DEFAULT_OPTIONS);
    expect(outcome.status).toBe("unsupported-content");
    if (outcome.status === "unsupported-content") {
      expect(outcome.contentType).toBe("application/octet-stream");
      expect(outcome.reason).toContain("unsupported content type");
    }
  });
});

describe("createNodeHttpsTransport (no network)", () => {
  it("constructs a transport port", () => {
    const transport = createNodeHttpsTransport();
    expect(typeof transport.get).toBe("function");
  });

  it("refuses http:// before any socket opens", async () => {
    const transport = createNodeHttpsTransport();
    const outcome = await transport.get("http://example.com/doc.md", DEFAULT_OPTIONS);
    expect(outcome).toEqual({ status: "refused", reason: "https only" });
  });

  it("refuses non-http(s) schemes", async () => {
    const transport = createNodeHttpsTransport();
    expect(await transport.get("ftp://example.com/doc.md", DEFAULT_OPTIONS)).toEqual({
      status: "refused",
      reason: "only https URLs are supported",
    });
  });

  it("refuses malformed URLs", async () => {
    const transport = createNodeHttpsTransport();
    const outcome = await transport.get("not a url", DEFAULT_OPTIONS);
    expect(outcome.status).toBe("refused");
    if (outcome.status === "refused") {
      expect(outcome.reason).toContain("malformed");
    }
  });

  it("returns cancelled for an already-aborted signal without network", async () => {
    const transport = createNodeHttpsTransport();
    expect(
      await transport.get("https://example.com/doc.md", { ...DEFAULT_OPTIONS, signal: aborted() }),
    ).toEqual({
      status: "cancelled",
    });
  });
});
