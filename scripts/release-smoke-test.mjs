/**
 * Release smoke test (W3.3b).
 *
 * What this proves, in the order the release path needs it:
 *
 *   1. the product builds from a CLEAN CLONE with no Godot sibling present —
 *      the release artifact may not depend on a neighbouring checkout;
 *   2. the built binary reports the workspace manifest version, and that version
 *      matches the tag being released when one is given;
 *   3. the binary runs with no configuration at all, answering one headless turn
 *      through the deterministic fake provider;
 *   4. a session opens and exits cleanly (the stdio frontend, `/exit`).
 *
 * It is runnable locally — `node scripts/release-smoke-test.mjs` — and is called
 * by the tag-triggered release workflow before anything is published, so a tag
 * that cannot pass its own smoke test never reaches a release.
 *
 * Usage:
 *   node scripts/release-smoke-test.mjs [--source <repo>] [--expected-version <v>] [--log <file>]
 */
import { spawnSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..");
const BUILD_TIMEOUT_MS = 30 * 60 * 1000;

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

/** Run a command, capturing output, and fail loudly with it when it fails. */
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout: options.timeoutMs ?? BUILD_TIMEOUT_MS,
    cwd: options.cwd,
    input: options.input,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`.trim();
  if (result.error !== undefined || result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status ?? result.error?.message})\n${output}`,
    );
  }
  return output;
}

/** The workspace version declared by the product manifest. */
function workspaceVersion(root) {
  const text = readFileSync(join(root, "Cargo.toml"), "utf8");
  const workspace = /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/u.exec(text);
  const version = workspace === null ? null : /version\s*=\s*"([^"]+)"/u.exec(workspace[1]);
  if (version === null) {
    throw new Error("Cargo.toml declares no [workspace.package] version");
  }
  return version[1];
}

function main() {
  const source = resolve(optionValue(process.argv, "--source") ?? REPO_ROOT);
  const expectedVersion = optionValue(process.argv, "--expected-version");
  const logPath = optionValue(process.argv, "--log");
  const transcript = [];
  const say = (line) => {
    console.log(line);
    transcript.push(line);
  };
  const scratch = mkdtempSync(join(tmpdir(), "siralos-release-smoke-"));
  try {
    const clone = join(scratch, "siralos");
    run("git", ["clone", "--quiet", source, clone]);
    say(`clone: ${source} -> ${clone}`);
    const sibling = join(scratch, "siralos-godot");
    if (existsSync(sibling)) {
      throw new Error("the smoke test requires a checkout with no Godot sibling beside it");
    }
    say("no Godot sibling beside the clone: confirmed");

    say("build: cargo build --release --locked --bin siralos");
    run("cargo", ["build", "--release", "--locked", "--bin", "siralos"], { cwd: clone });
    const binary = join(
      clone,
      "target",
      "release",
      process.platform === "win32" ? "siralos.exe" : "siralos",
    );
    if (!existsSync(binary)) {
      throw new Error("the release build produced no siralos binary");
    }

    const declared = workspaceVersion(clone);
    const reported = run(binary, ["--version"], { cwd: clone });
    if (reported !== `siralos ${declared}`) {
      throw new Error(
        `the binary reports ${JSON.stringify(reported)} but the manifest declares ${declared}`,
      );
    }
    say(`version identity: manifest ${declared}, binary reports ${JSON.stringify(reported)}`);
    if (expectedVersion !== undefined && expectedVersion !== declared) {
      throw new Error(
        `the workspace version is ${declared} but this run expects ${expectedVersion}`,
      );
    }
    if (expectedVersion !== undefined) {
      say(`tag agreement: ${expectedVersion} matches the workspace version`);
    }

    const empty = join(scratch, "empty-workspace");
    mkdirSync(empty);
    const answer = run(binary, ["--cwd", empty, "--print", "hello", "--json"], { cwd: clone });
    const record = JSON.parse(answer);
    if (record.answer === null || record.failure !== null) {
      throw new Error(`the no-config turn did not complete: ${answer}`);
    }
    say(
      `no-config headless turn: provider=${record.provider} answer=${JSON.stringify(record.answer)}`,
    );

    const session = run(binary, ["--stdio"], { cwd: empty, input: "/exit\n" });
    say(`scripted session opened and exited cleanly (output ${JSON.stringify(session)})`);

    say("release smoke test: PASS");
  } finally {
    if (logPath !== undefined) {
      writeFileSync(logPath, `${transcript.join("\n")}\n`, "utf8");
    }
    rmSync(scratch, { recursive: true, force: true });
  }
}

try {
  main();
} catch (error) {
  const message = `release smoke test: FAIL — ${error instanceof Error ? error.message : error}`;
  console.error(message);
  // main() writes the transcript up to the point of failure; append the reason so
  // a failed run leaves a complete local record instead of a log that stops.
  const logPath = optionValue(process.argv, "--log");
  if (logPath !== undefined) {
    appendFileSync(logPath, `${message}\n`, "utf8");
  }
  process.exit(1);
}
