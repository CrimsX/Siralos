/**
 * Reachability catalogue and ratchet (W4.0).
 *
 * WHAT THIS IS: a conservative, textual, module-level ratchet that keeps the list
 * of product modules the product cannot reach honest. It is a catalogue check,
 * NOT a reachability proof.
 *
 * METHOD: a module is "unreferenced" when no line outside its own file or
 * directory has the shape of an import or a path into it — `commands::`,
 * `crate::commands`, `use … commands;`. A bare mention of the identifier (a
 * field named after it, a comment) is not a reference, because a module the
 * product never imports is unreachable however often its name appears in prose.
 * Line-comment tails are stripped before the test, so `…; // commands::` does not
 * count either.
 *
 * MODULE SET: the module set holds product modules, and a file declared under a
 * `#[cfg(test)]` attribute is not one — `#[cfg(test)] mod tests;` compiles only
 * under `cargo test`, so the file it names is the suite. The attribute is read
 * from the file's declaration site (the sibling `mod.rs`, the parent file, or the
 * crate root), never from the file name, so a genuinely unreferenced non-test
 * module called `tests` cannot hide here. A declaration that cannot be located
 * leaves the module in the set: failing to recognise a test-only module
 * over-lists, which is the direction this check tolerates.
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
 *   - string literals are not parsed: a literal containing `identifier::` still
 *     counts as a reference, which is the one residual way a genuinely unimported
 *     module could stay unlisted, so a listed module is spot-checked by a human
 *     before it is deleted;
 *   - it is conservative in the direction that matters — a product module nothing
 *     imports must be listed, so unreachable product code cannot arrive unlisted.
 *     It does not prove that a listed module is truly unreachable;
 *   - the module set excludes test-only modules but the reference scan does not: a
 *     reference from test-only code still counts as a reference, so a module the
 *     product reaches only through its tests is not catalogued. That under-lists,
 *     and the scan does not strip `#[cfg(test)]` regions to compensate.
 *
 * It also refuses a stale report: a listed module that the product references
 * again, or a stamp that no longer matches the corpus manifest digest, must be
 * refreshed before the gate passes. The stamp is the manifest's SHA-256 rather
 * than a version number because milestone status — versions included — belongs to
 * the canonical status file, and the documentation-truth gate enforces that.
 *
 * The "corpus subjects" column is derived by matching a module identifier inside
 * the scenario documents, so it is a pointer for a reader, not a boundary claim.
 *
 * Usage:
 *   node scripts/check-reachability.mjs            # check (gate mode)
 *   node scripts/check-reachability.mjs --write    # refresh the table
 */
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { sha256Hex } from "../tests/differential/shared/canonical.mjs";

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

const ATTRIBUTE_LINE = /^\s*#\[.*\]\s*$/u;
const DOC_COMMENT_LINE = /^\s*(\/\/\/|\/\/!)/u;
const TEST_ATTRIBUTE = /^\s*#\[cfg\(\s*test\s*\)\]\s*$/u;

/** The `mod <identifier>;` declaration item, wherever it is written. */
function declaration(identifier) {
  return new RegExp("(^|[\\s\\]])(?:pub(?:\\([^)]*\\))?\\s+)?mod\\s+" + identifier + "\\s*;", "u");
}

/** The parent module file that declares `file`, or null when none exists. */
function parentModuleFile(file) {
  const name = basename(file);
  const parentDirectory = name === "mod.rs" ? dirname(dirname(file)) : dirname(file);
  const candidates = [
    join(parentDirectory, "mod.rs"),
    parentDirectory + ".rs",
    join(parentDirectory, "lib.rs"),
    join(parentDirectory, "main.rs"),
  ];
  return candidates.find((candidate) => existsSync(candidate)) ?? null;
}

/**
 * Whether `file` is declared as `mod <identifier>;` under `#[cfg(test)]`.
 *
 * The declaration site is the evidence, never the file name. A declaration that
 * cannot be located leaves the module in the product set: failing to recognise a
 * test-only module over-lists, which is the direction this check tolerates.
 */
function isTestOnlyModule(file, identifier) {
  const parent = parentModuleFile(file);
  if (parent === null) {
    return false;
  }
  const lines = readFileSync(parent, "utf8").split("\n");
  const declared = declaration(identifier);
  for (let index = 0; index < lines.length; index += 1) {
    // Comment tails are stripped so documented prose cannot look like a
    // declaration; see the limits in the header.
    const code = lines[index].split("//")[0];
    const match = declared.exec(code);
    if (match === null) {
      continue;
    }
    // Attributes written on the declaration's own line, before `mod`.
    for (const attribute of code
      .slice(0, match.index + match[1].length)
      .split("#")
      .slice(1)) {
      if (TEST_ATTRIBUTE.test("#" + attribute)) {
        return true;
      }
    }
    // Attributes (and doc comments, which are attributes) on the lines above.
    for (let above = index - 1; above >= 0; above -= 1) {
      if (!ATTRIBUTE_LINE.test(lines[above]) && !DOC_COMMENT_LINE.test(lines[above])) {
        break;
      }
      if (TEST_ATTRIBUTE.test(lines[above])) {
        return true;
      }
    }
    return false;
  }
  return false;
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
      if (isTestOnlyModule(file, identifier)) {
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
        // Comment tails are stripped so a documented name cannot look like an
        // import. String literals are not parsed; see the limits in the header.
        const code = line.split("//")[0];
        if (reaches.test(code) || imports.test(code)) {
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
  const stamp = /stamp: corpus manifest (sha256:[0-9a-f]{64})/u.exec(block);
  const listed = [...block.matchAll(/^\| `([^`]+)`/gmu)].map((match) => match[1]);
  return { listed, stamp: stamp === null ? null : stamp[1] };
}

/** The corpus manifest digest the catalogue was generated against. */
function corpusManifestDigest() {
  const manifest = join(REPO_ROOT, "tests", "differential", "corpus", "manifest.json");
  return "sha256:" + sha256Hex(readFileSync(manifest, "utf8"));
}

function renderBlock(entries) {
  const rows = entries
    .map((entry) => "| `" + entry.path + "` | " + entry.subjects + " | " + entry.consumer + " |")
    .join("\n");
  return [
    BEGIN,
    "stamp: corpus manifest " + corpusManifestDigest(),
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
  const manifestDigest = corpusManifestDigest();
  if (stamp !== manifestDigest) {
    errors.push(
      "REACHABILITY.md is stamped " +
        String(stamp) +
        " but the corpus manifest digests to " +
        manifestDigest +
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
