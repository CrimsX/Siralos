/**
 * Reachability catalogue and ratchet (W4.0).
 *
 * WHAT THIS IS: a conservative, textual, module-level ratchet that keeps the list
 * of product modules the product cannot reach honest. It is a catalogue check,
 * NOT a reachability proof.
 *
 * METHOD: a module is "unreferenced" when the identifier of its file (or of its
 * directory, for a `mod.rs») appears nowhere in the product crates' Rust sources
 * except (a) in the module's own file or directory, (b) on a bare `mod x;» /
 * `pub mod x;» declaration line, which declares rather than uses, or (c) in a
 * the shape of an import or a path. A bare mention of the identifier — a field
 * named after it, a comment, a string — is not a reference, because a module the
 * product never imports is unreachable however often its name appears in prose.
 *
 * LIMITS, stated so they cannot become hiding places:
 *   - it is textual and import-shaped: a module reached only through a glob
 *     import or a macro expansion is reported unreachable when it is not, which
 *     over-lists rather than under-lists;
 *   - it is module-level: it says nothing about individual items inside a
 *     reachable module;
 *   - reachability here is DIRECT: a module imported only by another unreachable
 *     module is not listed, so the catalogue is a lower bound on the unreachable
 *     set rather than an exact enumeration;
 *   - it is conservative in the direction that matters — a module nothing
 *     imports must be listed, so unreachable code cannot arrive unlisted. It does
 *     not prove that a listed module is truly unreachable.
 *
 * It also refuses a stale report: a listed module that the product references
 * again, or a stamp from a corpus version other than the current one, must be
 * refreshed before the gate passes.
 *
 * The "corpus subjects" column is derived by matching a module identifier inside
 * the scenario documents, so it is a pointer for a reader, not a boundary claim.
 *
 * Usage:
 *   node scripts/check-reachability.mjs            # check (gate mode)
 *   node scripts/check-reachability.mjs --write    # refresh the table
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CORPUS_VERSION } from "../tests/differential/shared/contract.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..");
const REPORT = join(REPO_ROOT, "docs", "development", "REACHABILITY.md");
const BEGIN = "<!-- reachability:begin -->";
const END = "<!-- reachability:end -->";
const PRODUCT_CRATES = ["crates/siralos-core", "crates/siralos-adapters", "crates/siralos-cli"];

function rustFiles(directory) {
  const found = [];
  const walk = (current) => {
    for (const entry of readdirSync(current)) {
      const full = join(current, entry);
      if (statSync(full).isDirectory()) {
        walk(full);
      } else if (entry.endsWith(".rs")) {
        found.push(full);
      }
    }
  };
  walk(directory);
  return found;
}

/** Every module of a product crate, with the identifier that references it. */
function productModules() {
  const modules = [];
  for (const crate of PRODUCT_CRATES) {
    const sourceRoot = join(REPO_ROOT, ...crate.split("/"), "src");
    for (const file of rustFiles(sourceRoot)) {
      const withinCrate = relative(sourceRoot, file).split("\\").join("/");
      const stem = withinCrate.replace(/\.rs$/u, "");
      const parts = stem.split("/");
      const identifier =
        parts[parts.length - 1] === "mod" ? parts[parts.length - 2] : parts[parts.length - 1];
      if (identifier === undefined || identifier === "lib" || identifier === "main") {
        continue;
      }
      modules.push({
        crate,
        identifier,
        path: crate + "/src/" + withinCrate,
        ownFile: file,
        ownDirectory: dirname(file),
      });
    }
  }
  return modules.sort((left, right) => left.path.localeCompare(right.path));
}

/** Whether any product source references the identifier outside its own module. */
function isReferenced(module) {
  const reaches = new RegExp("\\b" + module.identifier + "\\s*::", "u");
  const imports = new RegExp("\\buse\\b[^;]*\\b" + module.identifier + "\\b", "u");
  for (const crate of PRODUCT_CRATES) {
    const sourceRoot = join(REPO_ROOT, ...crate.split("/"), "src");
    for (const file of rustFiles(sourceRoot)) {
      if (file === module.ownFile || dirname(file) === module.ownDirectory) {
        continue;
      }
      for (const line of readFileSync(file, "utf8").split("\n")) {
        if (line.trim().startsWith("//")) {
          continue;
        }
        if (reaches.test(line) || imports.test(line)) {
          return true;
        }
      }
    }
  }
  return false;
}

/** Corpus subjects whose scenario documents mention a module identifier. */
function corpusSubjects(modules) {
  const corpusDir = join(REPO_ROOT, "tests", "differential", "corpus");
  const manifest = JSON.parse(readFileSync(join(corpusDir, "manifest.json"), "utf8"));
  const found = new Map();
  for (const entry of manifest.scenarios) {
    const scenario = JSON.parse(readFileSync(join(corpusDir, entry.file), "utf8"));
    const text = JSON.stringify(scenario);
    for (const module of modules) {
      if (text.includes(module.identifier)) {
        const subjects = found.get(module.identifier) ?? new Set();
        subjects.add(scenario.subject);
        found.set(module.identifier, subjects);
      }
    }
  }
  const rendered = new Map();
  for (const [identifier, subjects] of found) {
    rendered.set(
      identifier,
      [...subjects]
        .sort()
        .map((subject) => "`" + subject + "`")
        .join(", "),
    );
  }
  return rendered;
}

/** Parse the checked-in allowlist out of the report's marked block. */
function listedModules(text) {
  const start = text.indexOf(BEGIN);
  const end = text.indexOf(END);
  if (start === -1 || end === -1 || end < start) {
    throw new Error("REACHABILITY.md is missing its reachability:begin/end block");
  }
  const block = text.slice(start + BEGIN.length, end);
  const stamp = /stamp: corpus v(\d+)/u.exec(block);
  const listed = [...block.matchAll(/^\| `([^`]+)`/gmu)].map((match) => match[1]);
  return { listed, stamp: stamp === null ? null : Number(stamp[1]) };
}

function renderBlock(entries) {
  const rows = entries
    .map((entry) => "| `" + entry.path + "` | " + entry.subjects + " | " + entry.consumer + " |")
    .join("\n");
  return [
    BEGIN,
    "stamp: corpus v" + CORPUS_VERSION,
    "",
    "| Module | Corpus subjects that mention it | Sole consumer |",
    "| --- | --- | --- |",
    rows === "" ? "| _(none)_ | — | — |" : rows,
    END,
  ].join("\n");
}

function main() {
  const write = process.argv.includes("--write");
  const modules = productModules();
  const unreferenced = modules.filter((module) => !isReferenced(module));
  const subjects = corpusSubjects(unreferenced);
  const entries = unreferenced.map((module) => ({
    path: module.path,
    subjects: subjects.get(module.identifier) ?? "—",
    consumer: "harness (`harness/src/`)",
  }));
  const text = readFileSync(REPORT, "utf8");
  const { listed, stamp } = listedModules(text);
  if (write) {
    const start = text.indexOf(BEGIN);
    const end = text.indexOf(END);
    const updated = text.slice(0, start) + renderBlock(entries) + text.slice(end + END.length);
    writeFileSync(REPORT, updated, "utf8");
    console.log(
      "reachability report refreshed: " +
        entries.length +
        " unreferenced module(s) of " +
        modules.length,
    );
    return;
  }
  const errors = [];
  if (stamp !== CORPUS_VERSION) {
    errors.push(
      "REACHABILITY.md is stamped for corpus v" +
        String(stamp) +
        " but the corpus is v" +
        CORPUS_VERSION +
        "; refresh it with --write",
    );
  }
  const listedSet = new Set(listed);
  for (const entry of entries) {
    if (!listedSet.has(entry.path)) {
      errors.push(
        entry.path + ": unreferenced by every product crate but absent from REACHABILITY.md",
      );
    }
  }
  const unreferencedPaths = new Set(entries.map((entry) => entry.path));
  for (const path of listedSet) {
    if (!unreferencedPaths.has(path)) {
      errors.push(
        path + ": listed as unreachable but the product references it again; refresh the report",
      );
    }
  }
  if (errors.length > 0) {
    console.error("Reachability ratchet violations:");
    for (const error of errors) {
      console.error("  - " + error);
    }
    process.exit(1);
  }
  console.log(
    "Reachability ratchet held: " +
      entries.length +
      " unreferenced module(s) of " +
      modules.length +
      " are catalogued, and no unlisted module is unreferenced.",
  );
}

try {
  main();
} catch (error) {
  console.error(
    "reachability ratchet failed: " + (error instanceof Error ? error.message : String(error)),
  );
  process.exit(1);
}
