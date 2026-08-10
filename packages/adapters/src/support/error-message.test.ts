import { describe, expect, it } from "vitest";
import { createErrorDescriber, errorMessage, stringifyError } from "./error-message.js";

describe("adapter error messages", () => {
  it("preserves non-empty Error messages", () => {
    expect(errorMessage(new Error("specific failure"), "fallback")).toBe("specific failure");
  });

  it("uses the boundary fallback for empty or non-Error values", () => {
    const describeFailure = createErrorDescriber("boundary failure");
    expect(describeFailure(new Error(""))).toBe("boundary failure");
    expect(describeFailure({ reason: "untrusted object" })).toBe("boundary failure");
  });

  it("stringifies diagnostic-only non-Error values explicitly", () => {
    expect(stringifyError("plain failure")).toBe("plain failure");
  });
});
