import { describe, expect, it } from "vitest";
import {
  analyzeConventions,
  extractAddedLines,
  type ConventionChangeInput,
} from "./quality-conventions.js";

/**
 * Deterministic line-diff builder: identical lines become context,
 * differing lines are emitted as removals followed by additions. A single
 * trailing newline is normalized away so content lines align.
 */
function diffFor(before: string, after: string): string {
  const beforeLines = before.length === 0 ? [] : before.replace(/\n$/, "").split("\n");
  const afterLines = after.length === 0 ? [] : after.replace(/\n$/, "").split("\n");
  const lines: string[] = [
    `--- a/fixture.gd`,
    `+++ b/fixture.gd`,
    `@@ -1,${beforeLines.length} +1,${afterLines.length} @@`,
  ];
  const max = Math.max(beforeLines.length, afterLines.length);
  for (let index = 0; index < max; index += 1) {
    const removed = beforeLines[index];
    const added = afterLines[index];
    if (removed === added) {
      lines.push(` ${added ?? ""}`);
    } else {
      if (removed !== undefined) {
        lines.push(`-${removed}`);
      }
      if (added !== undefined) {
        lines.push(`+${added}`);
      }
    }
  }
  return lines.join("\n");
}

function change(path: string, before: string, after: string): ConventionChangeInput {
  return {
    path,
    operation: "update",
    afterContent: after,
    unifiedDiff: diffFor(before, after),
  };
}

describe("extractAddedLines", () => {
  it("parses hunk headers and yields absolute line numbers", () => {
    const added = extractAddedLines(diffFor("a\nb\n", "a\nb\nc\n"));
    expect(added).toHaveLength(1);
    expect(added[0]?.line).toBe(3);
    expect(added[0]?.text).toBe("c");
  });

  it("advances line numbers across context lines", () => {
    const added = extractAddedLines(["@@ -1,3 +1,3 @@", " a", " b", "+c", " d", "+e"].join("\n"));
    expect(added.map((entry) => entry.line)).toEqual([3, 5]);
  });

  it("ignores file headers and no-newline markers", () => {
    const added = extractAddedLines(
      ["--- a/x", "+++ b/x", "@@ -1 +1 @@", "+line", "\\ No newline at end of file"].join("\n"),
    );
    expect(added).toHaveLength(1);
    expect(added[0]?.text).toBe("line");
  });
});

describe("convention analysis", () => {
  it("detects trailing whitespace on a changed line", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", "a \n")]);
    expect(findings.some((finding) => finding.rule === "trailing-whitespace")).toBe(true);
    expect(findings.find((finding) => finding.rule === "trailing-whitespace")?.severity).toBe(
      "advisory",
    );
  });

  it("ignores existing unrelated trailing whitespace outside the change", () => {
    const findings = analyzeConventions([change("fixture.gd", "keep   \n", "keep   \nadd\n")]);
    expect(findings.some((finding) => finding.rule === "trailing-whitespace")).toBe(false);
  });

  it("flags mixed indentation in newly added blocks", () => {
    const findings = analyzeConventions([
      change(
        "fixture.gd",
        "func a():\n\tpass\n",
        "func a():\n\tpass\nfunc b():\n\tpass\nfunc c():\n    pass\n",
      ),
    ]);
    expect(findings.some((finding) => finding.rule === "mixed-indentation")).toBe(true);
  });

  it("flags very long newly introduced lines", () => {
    const longLine = "x".repeat(200);
    const findings = analyzeConventions([change("fixture.gd", "a\n", `a\n${longLine}\n`)]);
    expect(findings.some((finding) => finding.rule === "long-line")).toBe(true);
    expect(findings.find((finding) => finding.rule === "long-line")?.line).toBe(2);
  });

  it("flags multiple statements on one newly introduced line", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", "a\nvar x = 1; var y = 2\n")]);
    expect(findings.some((finding) => finding.rule === "multiple-statements")).toBe(true);
  });

  it("does not flag a semicolon ending a statement", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", 'a\nprint("x");\n')]);
    expect(findings.some((finding) => finding.rule === "multiple-statements")).toBe(false);
  });

  it("does not flag semicolons inside string literals", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", 'a\nprint("a;b")\n')]);
    expect(findings.some((finding) => finding.rule === "multiple-statements")).toBe(false);
  });

  it("treats underscore-prefixed Godot callbacks as snake_case, not camelCase", () => {
    const before = [
      "func _ready():",
      "\tpass",
      "func _physics_process(delta):",
      "\tpass",
      "func _process(delta):",
      "\tpass",
    ].join("\n");
    const after = `${before}\nfunc _on_health_changed():\n\tpass\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "naming-drift")).toBe(false);
  });

  it("detects naming drift against the file's local snake_case convention", () => {
    const before = [
      "func first_thing():",
      "\tpass",
      "func second_thing():",
      "\tpass",
      "var third_thing = 1",
    ].join("\n");
    const after = `${before}\nfunc fourthThing():\n\tpass\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "naming-drift")).toBe(true);
  });

  it("respects the file's local camelCase convention", () => {
    const before = [
      "func firstThing():",
      "\tpass",
      "func secondThing():",
      "\tpass",
      "var thirdThing = 1",
    ].join("\n");
    const after = `${before}\nfunc fourth_thing():\n\tpass\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "naming-drift")).toBe(true);
  });

  it("does not flag names in a file with no dominant style", () => {
    const before = ["func one():", "\tpass", "func two():", "\tpass"].join("\n");
    const after = `${before}\nfunc threeThing():\n\tpass\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "naming-drift")).toBe(false);
  });

  it("flags indentation width mismatch with the file's dominant unit", () => {
    const before = ["func a():", "    pass", "func b():", "    pass"].join("\n");
    const after = `${before}\nfunc c():\n\tpass\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "indentation-width-mismatch")).toBe(true);
  });

  it("advisories never block by default", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", "a \n")]);
    expect(findings.every((finding) => finding.severity === "advisory")).toBe(true);
  });

  it("promotes a deterministically enforced mandatory repository rule to warning", () => {
    const findings = analyzeConventions([change("fixture.gd", "a\n", "a \n")], {
      mandatoryRules: ["trailing-whitespace"],
    });
    const trailing = findings.find((finding) => finding.rule === "trailing-whitespace");
    expect(trailing?.severity).toBe("warning");
    expect(trailing?.basis).toBe("repository-guidance");
  });

  it("keeps the typed-file style advisory for untyped new functions in a typed file", () => {
    const before = [
      "func first() -> int:",
      "\treturn 1",
      "func second() -> int:",
      "\treturn 2",
    ].join("\n");
    const after = `${before}\nfunc third():\n\treturn 3\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "typed-signature-drift")).toBe(true);
  });

  it("does not force typing in a dynamic file", () => {
    const before = ["func first():", "\tpass", "func second():", "\tpass"].join("\n");
    const after = `${before}\nfunc third() -> int:\n\treturn 3\n`;
    const findings = analyzeConventions([change("fixture.gd", before, after)]);
    expect(findings.some((finding) => finding.rule === "typed-signature-drift")).toBe(false);
  });

  it("ignores deleted files and empty changes", () => {
    const findings = analyzeConventions([
      { path: "gone.gd", operation: "delete", afterContent: null, unifiedDiff: "" },
    ]);
    expect(findings).toHaveLength(0);
  });

  it("is deterministic for identical inputs", () => {
    const input = [change("fixture.gd", "a\n", "a \nlong\n")];
    const first = analyzeConventions(input);
    const second = analyzeConventions(input);
    expect(first).toEqual(second);
  });
});
