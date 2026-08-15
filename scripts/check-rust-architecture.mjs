/**
 * Siralos Rust architecture ratchet.
 *
 * Machine-enforced boundaries for the Rust candidate implementation
 * (Stage 3R, ADR 0032). This is a developer guardrail, not a security
 * boundary; the authoritative enforcement for unsafe code is the
 * `unsafe_code = "forbid"` workspace lint.
 *
 * Protects:
 * - canonical identity: the CLI binary is named `siralos`;
 * - workspace shape: exactly crates/siralos-core, crates/siralos-adapters,
 *   crates/siralos-cli — no placeholder or hypothetical domain crates;
 * - dependency direction: core depends on nothing; adapters may depend
 *   only on core; cli may depend only on core and adapters;
 * - domain neutrality: `siralos-core` sources contain no Godot-domain
 *   symbols;
 * - unsafe Rust: no `unsafe fn`/`unsafe impl`/`unsafe trait`/`unsafe {`
 *   in any crate source;
 * - edition and formatting policy: edition 2024, rustfmt configuration,
 *   and an explicit rust-toolchain.toml.
 */
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** The only crates allowed in the workspace. */
const EXPECTED_CRATES = ["crates/siralos-core", "crates/siralos-adapters", "crates/siralos-cli"];

/** Godot-domain symbols forbidden in `siralos-core` sources. */
const FORBIDDEN_CORE_SYMBOL_PATTERN =
  /godot|gdscript|\.tscn|\.tres|project\.godot|nodepath|autoload/i;

/**
 * Language-intelligence neutrality (Stage 3R R5): `siralos-core` must
 * stay free of LSP/JSON-RPC transport, process execution, socket
 * infrastructure, async runtimes, and domain-registry concepts. The
 * literal word "LSP" is allowed (position-conversion semantics are
 * documented against the protocol); transport machinery is not.
 */
const FORBIDDEN_CORE_LANGUAGE_PATTERN =
  /json-rpc|content-length|std::process|std::net|tokio|domain registry/i;

/** Dangerous unsafe forms (defense in depth behind the forbid lint). */
const UNSAFE_PATTERN = /\bunsafe\s+(fn|impl|trait|extern|\{)/;

/** Allowed workspace dependencies per crate. */
const ALLOWED_DEPENDENCIES = new Map([
  ["siralos-core", new Set()],
  ["siralos-adapters", new Set(["siralos-core"])],
  ["siralos-cli", new Set(["siralos-core", "siralos-adapters"])],
]);

/** List Rust source files beneath `directory` (absolute paths). */
export function listRustSources(directory) {
  const files = [];
  const walk = (current) => {
    for (const entry of readdirSync(current)) {
      const full = join(current, entry);
      if (statSync(full).isDirectory()) {
        walk(full);
      } else if (entry.endsWith(".rs")) {
        files.push(full);
      }
    }
  };
  walk(directory);
  return files;
}

/** Extract `name = ...` entries from a TOML `[section]` block. */
function tomlSectionKeys(content, section) {
  const match = content.match(new RegExp(`^\\s*\\[${section}\\]\\s*$`, "m"));
  if (match === null) {
    return [];
  }
  // The header regex may consume leading whitespace (a blank line), so
  // the line ending is searched from the END of the match, never from
  // its start.
  const newline = content.indexOf("\n", match.index + match[0].length);
  if (newline === -1) {
    return [];
  }
  const rest = content.slice(newline + 1);
  const end = rest.search(/^\s*\[/m);
  const block = end === -1 ? rest : rest.slice(0, end);
  return [...block.matchAll(/^\s*([A-Za-z0-9_-]+)\s*=/gm)].map((match) => match[1]);
}

/**
 * Workspace crate names referenced by a crate manifest.
 *
 * Covers `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`,
 * and target-scoped tables such as `[target.'cfg(windows)'.dependencies]`
 * in inline, sub-table, and aliased (`package = "siralos-*"`) forms.
 * Sub-table and target-scoped headers never appear beneath a plain
 * `[dependencies]` header, so the whole file is scanned; none of these
 * spellings may evade the dependency-direction check.
 */
function collectWorkspaceDependencies(content) {
  const names = new Set();
  // Sections whose name ends in "dependencies", including target-scoped
  // tables; collects `name = ...` keys and aliased package pointers.
  for (const match of content.matchAll(/^\s*\[([^\]]*dependencies)\]\s*$/gm)) {
    // The section regex may consume leading whitespace (a blank line),
    // so the line ending is searched from the END of the match, never
    // from its start.
    const newline = content.indexOf("\n", match.index + match[0].length);
    if (newline === -1) {
      continue;
    }
    const rest = content.slice(newline + 1);
    const end = rest.search(/^\s*\[/m);
    const block = end === -1 ? rest : rest.slice(0, end);
    for (const key of block.matchAll(/^\s*([A-Za-z0-9_-]+)\s*=\s*/gm)) {
      names.add(key[1]);
    }
    for (const pkg of block.matchAll(/package\s*=\s*"([^"]+)"/g)) {
      names.add(pkg[1]);
    }
  }
  // Sub-tables such as `[dependencies.siralos-core]`,
  // `[dev-dependencies.x]`, and `[target.'cfg(windows)'.dependencies.x]`,
  // including quoted keys, which are valid TOML.
  for (const match of content.matchAll(
    /^\s*\[[^\]]*dependencies\.(?:"([A-Za-z0-9_-]+)"|([A-Za-z0-9_-]+))\]\s*$/gm,
  )) {
    names.add(match[1] ?? match[2]);
  }
  return [...names];
}

/** Accept either the inline or the table form of the lints opt-in. */
function hasWorkspaceLints(content) {
  if (/^\s*lints\s*=\s*\{\s*workspace\s*=\s*true\s*\}\s*$/m.test(content)) {
    return true;
  }
  return /^\[lints\]\s*$/m.test(content) && /^\s*workspace\s*=\s*true\s*$/m.test(content);
}

/** Read a crate's Cargo.toml as text. */
function readCargoToml(root, crate) {
  return readFileSync(join(root, crate, "Cargo.toml"), "utf8");
}

/** Run every Rust architecture check; returns a list of error strings. */
export function runChecks(root) {
  const errors = [];

  const rootCargo = join(root, "Cargo.toml");
  if (!existsSync(rootCargo)) {
    return [`${rootCargo}: missing workspace Cargo.toml`];
  }
  const rootCargoText = readFileSync(rootCargo, "utf8");

  const members = tomlSectionKeys(rootCargoText, "workspace").filter(
    (key) => key !== "resolver" && key !== "members" && key !== "exclude",
  );
  const memberLines = [...rootCargoText.matchAll(/members\s*=\s*\[([^\]]*)\]/g)].flatMap((match) =>
    [...match[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]),
  );
  // The nightly-only fuzz crate must stay outside the stable workspace
  // (docs/development/SUPPLY_CHAIN.md toolchain policy).
  const excludeLines = [...rootCargoText.matchAll(/exclude\s*=\s*\[([^\]]*)\]/g)].flatMap((match) =>
    [...match[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]),
  );
  if (!excludeLines.includes("fuzz")) {
    errors.push("Cargo.toml: the nightly-only fuzz crate must be excluded from the workspace");
  }
  if (memberLines.length === 0) {
    errors.push("Cargo.toml: workspace members must be declared explicitly");
  }
  for (const member of EXPECTED_CRATES) {
    if (!memberLines.includes(member)) {
      errors.push(`Cargo.toml: workspace member ${member} is missing`);
    }
  }
  for (const member of memberLines) {
    if (!EXPECTED_CRATES.includes(member)) {
      errors.push(`Cargo.toml: unexpected workspace member ${member} (no placeholder domains)`);
    }
  }
  if (members.length > 0) {
    errors.push(`Cargo.toml: unexpected keys in [workspace] table: ${members.join(", ")}`);
  }
  if (!/^\s*resolver\s*=\s*"3"\s*$/m.test(rootCargoText)) {
    errors.push('Cargo.toml: workspace resolver must be "3"');
  }
  if (!/^\s*edition\s*=\s*"2024"\s*$/m.test(rootCargoText)) {
    errors.push('Cargo.toml: workspace edition must be "2024"');
  }

  // Exactly the three crates exist, with no placeholder domains.
  const cratesDirectory = join(root, "crates");
  if (!existsSync(cratesDirectory)) {
    return [`${cratesDirectory}: missing crates directory`];
  }
  const present = readdirSync(cratesDirectory).filter((entry) =>
    existsSync(join(cratesDirectory, entry, "Cargo.toml")),
  );
  for (const crate of present) {
    if (!EXPECTED_CRATES.includes(`crates/${crate}`)) {
      errors.push(`crates/${crate}: unexpected crate (no placeholder or hypothetical domains)`);
    }
  }
  for (const crate of EXPECTED_CRATES) {
    if (!present.includes(crate.slice("crates/".length))) {
      errors.push(`${crate}: missing crate`);
    }
  }

  for (const crate of EXPECTED_CRATES) {
    const cargoText = readCargoToml(root, crate);
    if (!/^name\s*=\s*"([^"]+)"\s*$/m.test(cargoText)) {
      errors.push(`${crate}: Cargo.toml must declare a name`);
    }
    if (!/^\s*edition\.workspace\s*=\s*true\s*$/m.test(cargoText)) {
      errors.push(`${crate}: edition must come from the workspace (edition 2024)`);
    }
    if (!hasWorkspaceLints(cargoText)) {
      errors.push(`${crate}: workspace lints must be enabled`);
    }
    if (!/^\s*publish\s*=\s*false\s*$/m.test(cargoText)) {
      errors.push(`${crate}: private crates must set publish = false`);
    }
    const dependencies = collectWorkspaceDependencies(cargoText).filter((key) =>
      key.startsWith("siralos-"),
    );
    const crateName = crate.slice("crates/".length);
    const allowed = ALLOWED_DEPENDENCIES.get(crateName) ?? new Set();
    for (const dependency of dependencies) {
      if (!allowed.has(dependency)) {
        errors.push(
          `${crate}: forbidden workspace dependency on ${dependency} (direction: ${[...allowed].join(", ") || "none"})`,
        );
      }
    }
    if (crate === "siralos-core" && dependencies.length > 0) {
      errors.push("siralos-core: the domain-neutral core must have no workspace dependencies");
    }
  }

  // The CLI binary identity.
  const cliCargo = readCargoToml(root, "crates/siralos-cli");
  const binNames = [...cliCargo.matchAll(/\[\[bin\]\][\s\S]*?name\s*=\s*"([^"]+)"/g)].map(
    (match) => match[1],
  );
  if (!binNames.includes("siralos")) {
    errors.push('crates/siralos-cli: the binary must be named "siralos"');
  }

  // Toolchain and formatting policy are explicit and deterministic.
  if (!existsSync(join(root, "rust-toolchain.toml"))) {
    errors.push("rust-toolchain.toml: an explicit toolchain is required");
  } else {
    const toolchain = readFileSync(join(root, "rust-toolchain.toml"), "utf8");
    if (!/^\s*channel\s*=\s*"[^"]+"\s*$/m.test(toolchain)) {
      errors.push("rust-toolchain.toml: a channel must be pinned");
    }
  }
  if (!existsSync(join(root, "rustfmt.toml"))) {
    errors.push("rustfmt.toml: deterministic formatting configuration is required");
  } else {
    const rustfmt = readFileSync(join(root, "rustfmt.toml"), "utf8");
    if (!/^\s*max_width\s*=\s*79\s*$/m.test(rustfmt)) {
      errors.push("rustfmt.toml: max_width must be 79");
    }
  }

  // Domain neutrality of core, plus the unsafe backstop, over all sources.
  for (const crate of EXPECTED_CRATES) {
    for (const source of listRustSources(join(root, crate))) {
      const content = readFileSync(source, "utf8");
      if (crate === "crates/siralos-core" && FORBIDDEN_CORE_SYMBOL_PATTERN.test(content)) {
        errors.push(`${source}: siralos-core must stay domain-neutral (Godot symbol present)`);
      }
      if (crate === "crates/siralos-core" && FORBIDDEN_CORE_LANGUAGE_PATTERN.test(content)) {
        errors.push(
          `${source}: siralos-core must stay language-intelligence-neutral (LSP transport, process, socket, async runtime, or domain-registry symbol present)`,
        );
      }
      if (UNSAFE_PATTERN.test(content)) {
        errors.push(`${source}: unsafe Rust is forbidden in the Siralos foundation`);
      }
    }
  }

  return errors;
}

function main() {
  const root = join(import.meta.dirname, "..");
  const errors = runChecks(root);
  if (errors.length > 0) {
    console.error("Rust architecture violations:");
    for (const error of errors) {
      console.error(`  - ${error}`);
    }
    process.exit(1);
  }
  console.log("Rust architecture check passed.");
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
