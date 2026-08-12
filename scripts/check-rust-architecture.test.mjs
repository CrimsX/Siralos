import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { runChecks } from "./check-rust-architecture.mjs";

const tempDirectories = [];

function writeFixture(files) {
  const root = mkdtempSync(join(tmpdir(), "siralos-rust-architecture-"));
  tempDirectories.push(root);
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return root;
}

const ROOT_CARGO = `[workspace]
resolver = "3"
members = [
    "crates/siralos-core",
    "crates/siralos-adapters",
    "crates/siralos-cli",
]

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.85"
`;

const TOOLCHAIN = 'channel = "1.97.1"\n';

const RUSTFMT = 'max_width = 79\nuse_small_heuristics = "max"\nedition = "2024"\n';

function crateCargo(name, dependencies = "") {
  return `[package]
name = "${name}"
description = "fixture"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish = false
${dependencies}
[lints]
workspace = true
`;
}

function cleanWorkspaceFixture(overrides = {}) {
  return {
    "Cargo.toml": ROOT_CARGO,
    "rust-toolchain.toml": TOOLCHAIN,
    "rustfmt.toml": RUSTFMT,
    "crates/siralos-core/Cargo.toml": crateCargo("siralos-core"),
    "crates/siralos-core/src/lib.rs": "//! Fixture core.\n",
    "crates/siralos-adapters/Cargo.toml": crateCargo(
      "siralos-adapters",
      "\n[dependencies]\nsiralos-core = { workspace = true }\n",
    ),
    "crates/siralos-adapters/src/lib.rs": "//! Fixture adapters.\n",
    "crates/siralos-cli/Cargo.toml": `${crateCargo(
      "siralos-cli",
      "\n[dependencies]\nsiralos-core = { workspace = true }\nsiralos-adapters = { workspace = true }\n",
    )}
[[bin]]
name = "siralos"
path = "src/main.rs"
`,
    "crates/siralos-cli/src/main.rs": "fn main() {}\n",
    ...overrides,
  };
}

afterEach(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe("runChecks", () => {
  it("passes on a clean workspace", () => {
    const root = writeFixture(cleanWorkspaceFixture());
    expect(runChecks(root)).toEqual([]);
  });

  it("rejects a missing workspace manifest", () => {
    const root = writeFixture({ "crates/siralos-core/src/lib.rs": "//! Core.\n" });
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes("missing workspace Cargo.toml"))).toBe(true);
  });

  it("rejects placeholder or hypothetical domain crates", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-godot/Cargo.toml": crateCargo("siralos-godot"),
        "crates/siralos-godot/src/lib.rs": "//! Hypothetical domain.\n",
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("unexpected crate") && error.includes("siralos-godot")),
    ).toBe(true);
  });

  it("rejects a core dependency on adapters", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-core/Cargo.toml": crateCargo(
          "siralos-core",
          "\n[dependencies]\nsiralos-adapters = { workspace = true }\n",
        ),
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("siralos-core") && error.includes("siralos-adapters")),
    ).toBe(true);
  });

  it("rejects an adapters dependency on the cli", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-adapters/Cargo.toml": crateCargo(
          "siralos-adapters",
          "\n[dependencies]\nsiralos-cli = { workspace = true }\n",
        ),
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("siralos-adapters") && error.includes("siralos-cli")),
    ).toBe(true);
  });

  it("rejects table-form dependencies that bypass the inline spelling", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-core/Cargo.toml": `${crateCargo("siralos-core")}

[dependencies.siralos-adapters]
workspace = true
`,
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("siralos-core") && error.includes("siralos-adapters")),
    ).toBe(true);
  });

  it("rejects dev-dependencies that bypass the direction check", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-core/Cargo.toml": `${crateCargo("siralos-core")}

[dev-dependencies]
siralos-adapters = { workspace = true }
`,
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("siralos-core") && error.includes("siralos-adapters")),
    ).toBe(true);
  });

  it("rejects aliased dependencies that point at a workspace crate", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-core/Cargo.toml": `${crateCargo("siralos-core")}

[dependencies]
adapter-alias = { workspace = true, package = "siralos-adapters" }
`,
      }),
    );
    const errors = runChecks(root);
    expect(
      errors.some((error) => error.includes("siralos-core") && error.includes("siralos-adapters")),
    ).toBe(true);
  });

  it("rejects a wrong binary name", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-cli/Cargo.toml": `${crateCargo(
          "siralos-cli",
          "\n[dependencies]\nsiralos-core = { workspace = true }\n",
        )}
[[bin]]
name = "solaris-other"
path = "src/main.rs"
`,
      }),
    );
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes('binary must be named "siralos"'))).toBe(true);
  });

  it("rejects Godot symbols in siralos-core sources", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-core/src/lib.rs":
          "//! Core.\n\n/// Godot scene reference must never reach core.\npub fn scene() {}\n",
      }),
    );
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes("domain-neutral"))).toBe(true);
  });

  it("rejects unsafe Rust anywhere in the foundation", () => {
    const root = writeFixture(
      cleanWorkspaceFixture({
        "crates/siralos-adapters/src/lib.rs": "//! Adapters.\n\npub unsafe fn probe() {}\n",
      }),
    );
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes("unsafe Rust is forbidden"))).toBe(true);
  });

  it("rejects a rustfmt configuration that abandons max_width 79", () => {
    const root = writeFixture(cleanWorkspaceFixture({ "rustfmt.toml": "max_width = 100\n" }));
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes("max_width must be 79"))).toBe(true);
  });

  it("rejects a missing explicit toolchain", () => {
    const files = cleanWorkspaceFixture();
    delete files["rust-toolchain.toml"];
    const root = writeFixture(files);
    const errors = runChecks(root);
    expect(errors.some((error) => error.includes("rust-toolchain.toml"))).toBe(true);
  });
});
