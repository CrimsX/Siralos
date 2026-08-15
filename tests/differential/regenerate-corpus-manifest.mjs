// Regenerate the R3 corpus manifest digests (canonical form).
// Usage: node tests/differential/regenerate-corpus-manifest.mjs
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const CORPUS = join(HERE, "corpus");

function canonicalizeJson(value) {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return "[" + value.map((entry) => canonicalizeJson(entry)).join(",") + "]";
  }
  const keys = Object.keys(value).sort();
  return (
    "{" +
    keys.map((key) => JSON.stringify(key) + ":" + canonicalizeJson(value[key])).join(",") +
    "}"
  );
}

function sha256Hex(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

const manifest = JSON.parse(readFileSync(join(CORPUS, "manifest.json"), "utf8"));
const entries = [];
for (const name of readdirSync(CORPUS).filter(
  (name) => name.endsWith(".json") && name !== "manifest.json",
)) {
  const text = readFileSync(join(CORPUS, name), "utf8");
  const scenario = JSON.parse(text);
  entries.push({ file: name, sha256: sha256Hex(canonicalizeJson(scenario)) });
}
entries.sort((a, b) => a.file.localeCompare(b.file));
manifest.scenarios = entries;
manifest.corpusSha256 = sha256Hex(
  canonicalizeJson({
    schemaVersion: manifest.schemaVersion,
    corpusVersion: manifest.corpusVersion,
    scenarios: manifest.scenarios,
  }),
);
writeFileSync(join(CORPUS, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n", "utf8");
console.log("manifest regenerated:", manifest.corpusSha256);
