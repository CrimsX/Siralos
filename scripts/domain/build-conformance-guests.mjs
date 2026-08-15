/**
 * Build the synthetic conformance domain components (Stage 3R R6) and
 * refresh the checked-in fixture bytes.
 *
 * Requires the pinned Rust toolchain with the wasm32-wasip2 target
 * installed. Prints the new fixture SHA-256 digests; update the digest
 * constants in crates/siralos-adapters/tests/domain_conformance.rs in
 * the same change.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const CONFORMANCE = join(ROOT, "tests", "domain-conformance");
const FIXTURES = join(CONFORMANCE, "fixtures");

function run(args, cwd) {
  const result = spawnSync("cargo", args, { cwd, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`cargo ${args.join(" ")} failed with status ${result.status}`);
  }
}

function copyWasm(guest, artifact, fixture) {
  const source = join(CONFORMANCE, guest, "target", "wasm32-wasip2", "release", artifact);
  const target = join(FIXTURES, fixture);
  copyFileSync(source, target);
  const digest = createHash("sha256").update(readFileSync(target)).digest("hex");
  console.log(`${fixture} ${digest}`);
}

run(
  [
    "build",
    "--manifest-path",
    join(CONFORMANCE, "guest", "Cargo.toml"),
    "--release",
    "--target",
    "wasm32-wasip2",
    "--locked",
  ],
  ROOT,
);
copyWasm("guest", "siralos_domain_conformance_guest.wasm", "conformance-domain.component.wasm");
run(
  [
    "build",
    "--manifest-path",
    join(CONFORMANCE, "guest-incompatible", "Cargo.toml"),
    "--release",
    "--target",
    "wasm32-wasip2",
    "--locked",
  ],
  ROOT,
);
copyWasm(
  "guest-incompatible",
  "siralos_domain_conformance_guest_incompatible.wasm",
  "incompatible-domain.component.wasm",
);
console.log("domain conformance fixtures refreshed");
