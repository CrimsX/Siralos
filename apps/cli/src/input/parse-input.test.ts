import { describe, expect, it } from "vitest";
import { parseInput } from "./parse-input.js";

describe("parseInput", () => {
  it("parses a plain prompt", () => {
    expect(parseInput("hello")).toEqual({ type: "prompt", text: "hello" });
  });

  it("trims surrounding whitespace from prompts", () => {
    expect(parseInput("  hello  ")).toEqual({ type: "prompt", text: "hello" });
  });

  it("treats empty and whitespace-only input as empty", () => {
    expect(parseInput("")).toEqual({ type: "empty" });
    expect(parseInput("   ")).toEqual({ type: "empty" });
  });

  it("recognizes every slash command", () => {
    expect(parseInput("/help")).toEqual({ type: "command", command: "help" });
    expect(parseInput("/status")).toEqual({ type: "command", command: "status" });
    expect(parseInput("/clear")).toEqual({ type: "command", command: "clear" });
    expect(parseInput("/tools")).toEqual({ type: "command", command: "tools" });
    expect(parseInput("/exit")).toEqual({ type: "command", command: "exit" });
  });

  it("trims whitespace around commands", () => {
    expect(parseInput("  /exit  ")).toEqual({ type: "command", command: "exit" });
  });

  it("rejects unknown and mis-cased slash commands", () => {
    expect(parseInput("/bogus")).toEqual({ type: "invalid_command", input: "/bogus" });
    expect(parseInput("/HELP")).toEqual({ type: "invalid_command", input: "/HELP" });
    expect(parseInput("/exit now")).toEqual({
      type: "invalid_command",
      input: "/exit now",
    });
  });

  it("treats text without a slash as a prompt", () => {
    expect(parseInput("help")).toEqual({ type: "prompt", text: "help" });
  });
});
