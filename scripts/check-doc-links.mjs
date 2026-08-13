/**
 * Deterministic local-document link check.
 *
 * The check validates repository-relative targets in Markdown inline links
 * and reference definitions. External URLs and same-document anchors are
 * intentionally outside its scope. Path spelling is checked component by
 * component so a link that only works through Windows case folding still
 * fails before reaching a case-sensitive checkout.
 */
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const SKIPPED_DIRECTORIES = new Set([
  ".git",
  ".reasonix",
  "coverage",
  "dist",
  "node_modules",
  "target",
]);

const INLINE_LINK =
  /!?\[[^\]\n]*\]\(\s*(<[^>\n]+>|[^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?\s*\)/g;
const REFERENCE_LINK = /^\s{0,3}\[[^\]\n]+\]:\s*(<[^>\n]+>|\S+)/gm;
const EXTERNAL_TARGET = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;

/** Return every project-owned Markdown path relative to `root`. */
export function collectMarkdownFiles(root) {
  const files = [];

  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isSymbolicLink()) {
        continue;
      }

      const fullPath = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED_DIRECTORIES.has(entry.name)) {
          walk(fullPath);
        }
      } else if (entry.isFile() && entry.name.toLowerCase().endsWith(".md")) {
        files.push(relative(root, fullPath).split(sep).join("/"));
      }
    }
  };

  walk(root);
  return files.sort();
}

/** Replace fenced code with newlines so offsets and line numbers stay stable. */
function maskFencedCode(markdown) {
  const lines = markdown.split(/(?<=\n)/);
  let fence = null;

  return lines
    .map((line) => {
      const match = /^\s*(`{3,}|~{3,})/.exec(line);
      if (match) {
        const marker = match[1][0];
        if (fence === null) {
          fence = marker;
        } else if (fence === marker) {
          fence = null;
        }
        return line.replace(/[^\r\n]/g, " ");
      }
      return fence === null ? line : line.replace(/[^\r\n]/g, " ");
    })
    .join("");
}

function lineAt(content, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (content.charCodeAt(index) === 10) {
      line += 1;
    }
  }
  return line;
}

/** Extract local targets with their source line. */
export function extractLocalLinks(markdown) {
  const masked = maskFencedCode(markdown);
  const links = [];

  for (const pattern of [INLINE_LINK, REFERENCE_LINK]) {
    pattern.lastIndex = 0;
    for (const match of masked.matchAll(pattern)) {
      const rawTarget = match[1].startsWith("<") ? match[1].slice(1, -1) : match[1];
      if (rawTarget.length === 0 || rawTarget.startsWith("#") || EXTERNAL_TARGET.test(rawTarget)) {
        continue;
      }
      links.push({ target: rawTarget, line: lineAt(masked, match.index) });
    }
  }

  return links.sort(
    (left, right) => left.line - right.line || left.target.localeCompare(right.target),
  );
}

function exactPathExists(root, target) {
  const targetRelative = relative(root, target);
  if (targetRelative === "") {
    return true;
  }

  let cursor = root;
  for (const component of targetRelative.split(sep)) {
    if (!existsSync(cursor) || !statSync(cursor).isDirectory()) {
      return false;
    }
    const names = readdirSync(cursor);
    if (!names.includes(component)) {
      return false;
    }
    cursor = join(cursor, component);
  }
  return existsSync(cursor);
}

/** Validate all local Markdown targets beneath `root`. */
export function runCheck(root) {
  const violations = [];

  for (const file of collectMarkdownFiles(root)) {
    const sourcePath = join(root, file);
    const markdown = readFileSync(sourcePath, "utf8");
    for (const link of extractLocalLinks(markdown)) {
      let pathname;
      try {
        pathname = decodeURIComponent(link.target.split(/[?#]/, 1)[0]);
      } catch {
        violations.push({
          file,
          line: link.line,
          target: link.target,
          reason: "invalid URI encoding",
        });
        continue;
      }

      if (isAbsolute(pathname)) {
        violations.push({
          file,
          line: link.line,
          target: link.target,
          reason: "absolute local path",
        });
        continue;
      }

      const targetPath = resolve(dirname(sourcePath), pathname);
      const fromRoot = relative(root, targetPath);
      if (fromRoot === ".." || fromRoot.startsWith(`..${sep}`) || isAbsolute(fromRoot)) {
        violations.push({
          file,
          line: link.line,
          target: link.target,
          reason: "outside repository",
        });
      } else if (!exactPathExists(root, targetPath)) {
        violations.push({ file, line: link.line, target: link.target, reason: "target not found" });
      }
    }
  }

  return { ok: violations.length === 0, violations };
}

function main() {
  const root = join(import.meta.dirname, "..");
  const result = runCheck(root);
  if (!result.ok) {
    console.error("Documentation link violations:");
    for (const violation of result.violations) {
      console.error(
        `  - ${violation.file}:${violation.line}: ${violation.target} (${violation.reason})`,
      );
    }
    process.exit(1);
  }
  console.log("Documentation link check passed: all local Markdown targets exist.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
