import { describe, expect, it } from "vitest";
import { resolveReviewProviderId } from "./review-provider.js";

describe("independent-review provider resolution (ADR 0013 §26, §61)", () => {
  const registered = new Set(["deterministic-fake", "reviewer"]);

  it("uses the active development provider profile by default", () => {
    const resolved = resolveReviewProviderId({
      configured: null,
      registered,
      defaultId: "deterministic-fake",
    });
    expect(resolved).toEqual({ ok: true, providerId: "deterministic-fake" });
  });

  it("resolves an explicitly configured review provider when it is registered", () => {
    const resolved = resolveReviewProviderId({
      configured: "reviewer",
      registered,
      defaultId: "deterministic-fake",
    });
    expect(resolved).toEqual({ ok: true, providerId: "reviewer" });
  });

  it("fails clearly for a missing profile and never silently falls back", () => {
    const resolved = resolveReviewProviderId({
      configured: "does-not-exist",
      registered,
      defaultId: "deterministic-fake",
    });
    expect(resolved.ok).toBe(false);
    if (!resolved.ok) {
      expect(resolved.message).toContain("does-not-exist");
      expect(resolved.message).toContain("registered provider profile");
    }
  });

  it("is deterministic", () => {
    const options = { configured: null, registered, defaultId: "deterministic-fake" };
    expect(resolveReviewProviderId(options)).toEqual(resolveReviewProviderId(options));
  });
});
