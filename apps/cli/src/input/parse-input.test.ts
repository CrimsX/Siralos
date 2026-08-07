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
    expect(parseInput("/help")).toEqual({ type: "command", command: "help", args: [] });
    expect(parseInput("/status")).toEqual({ type: "command", command: "status", args: [] });
    expect(parseInput("/clear")).toEqual({ type: "command", command: "clear", args: [] });
    expect(parseInput("/tools")).toEqual({ type: "command", command: "tools", args: [] });
    expect(parseInput("/sandbox")).toEqual({ type: "command", command: "sandbox", args: [] });
    expect(parseInput("/permissions")).toEqual({
      type: "command",
      command: "permissions",
      args: [],
    });
    expect(parseInput("/git-status")).toEqual({ type: "command", command: "git-status", args: [] });
    expect(parseInput("/diff")).toEqual({ type: "command", command: "diff", args: [] });
    expect(parseInput("/checkpoints")).toEqual({
      type: "command",
      command: "checkpoints",
      args: [],
    });
    expect(parseInput("/undo")).toEqual({ type: "command", command: "undo", args: [] });
    expect(parseInput("/exit")).toEqual({ type: "command", command: "exit", args: [] });
  });

  it("parses command arguments", () => {
    expect(parseInput("/diff staged")).toEqual({
      type: "command",
      command: "diff",
      args: ["staged"],
    });
    expect(parseInput("/undo cp_123")).toEqual({
      type: "command",
      command: "undo",
      args: ["cp_123"],
    });
  });

  it("trims whitespace around commands", () => {
    expect(parseInput("  /exit  ")).toEqual({ type: "command", command: "exit", args: [] });
  });

  it("rejects unknown and mis-cased slash commands", () => {
    expect(parseInput("/bogus")).toEqual({ type: "invalid_command", input: "/bogus" });
    expect(parseInput("/HELP")).toEqual({ type: "invalid_command", input: "/HELP" });
  });

  it("treats text without a slash as a prompt", () => {
    expect(parseInput("help")).toEqual({ type: "prompt", text: "help" });
  });
});
