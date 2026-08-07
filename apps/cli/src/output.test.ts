import { describe, expect, it } from "vitest";
import { TerminalSanitizer, sanitizeForDisplay } from "./output.js";

describe("TerminalSanitizer", () => {
  it("preserves ordinary text and readable newlines", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("hello world\nsecond line\twith tab")).toBe(
      "hello world\nsecond line\twith tab",
    );
  });

  it("strips ANSI CSI sequences completely", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("before\u001b[31mred\u001b[0mafter")).toBe("beforeredafter");
  });

  it("strips OSC sequences including links, titles, and clipboard writes", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\u001b]8;;https://evil.example\u0007link\u001b]8;;\u0007b")).toBe(
      "alinkb",
    );
    expect(sanitizer.push("\u001b]0;title\u0007rest")).toBe("rest");
    expect(sanitizer.push("\u001b]52;c;c2VjcmV0\u0007rest2")).toBe("rest2");
  });

  it("renders carriage return, backspace, and DEL visibly", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\rb\u0008c\u007fd")).toBe("a^Mb^Hc^?d");
  });

  it("replaces other C0 and C1 controls", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\u0000b\u0003c\u0085d\u009fe")).toBe("a^@b^Cc\uFFFDd\uFFFDe");
  });

  it("handles sequences fragmented across chunks", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("x\u001b")).toBe("x");
    expect(sanitizer.push("[3")).toBe("");
    expect(sanitizer.push("1mboom\u001b]8;;https://e")).toBe("boom");
    expect(sanitizer.push("\u0007tail")).toBe("tail");
  });

  it("drops a dangling sequence at flush", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("safe\u001b[31m")).toBe("safe");
    expect(sanitizer.flush()).toBe("");
    expect(sanitizer.push("after")).toBe("after");
  });

  it("keeps ordinary unicode intact", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("caf\u00e9 \u4e2d\u6587 \u{1F600}")).toBe(
      "caf\u00e9 \u4e2d\u6587 \u{1F600}",
    );
  });

  it("does not let a dangling escape corrupt the next chunk", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("\u001b")).toBe("");
    expect(sanitizer.push("> prompt text")).toBe(" prompt text");
    expect(sanitizer.push("[99m")).toBe("[99m");
    expect(sanitizer.push("ok")).toBe("ok");
  });
});

describe("sanitizeForDisplay", () => {
  it("sanitizes complete and partial control sequences in one shot", () => {
    expect(sanitizeForDisplay("diff \u001b[1mheader\u001b[0m")).toBe("diff header");
    expect(sanitizeForDisplay("partial \u001b[31")).toBe("partial ");
    expect(sanitizeForDisplay("osc \u001b]52;c;bGFtZXI=\u0007 end")).toBe("osc  end");
    expect(sanitizeForDisplay("cr rewrite\rback")).toBe("cr rewrite^Mback");
  });
});
