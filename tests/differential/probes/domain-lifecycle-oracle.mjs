/**
 * domain-lifecycle oracle probe (differential harness, ADR 0033,
 * Stage 3R R6).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes lifecycle scenarios against the REAL TypeScript reference
 * domain lifecycle/capability modules
 * (packages/core/src/domain). This is a thin scenario adapter: it
 * wires production functions and maps results to the canonical record
 * vocabulary; it does not reimplement lifecycle behavior.
 *
 * Deterministic: session ids come from the production lifecycle
 * sequence; no ambient clock, randomness, or environment access
 * enters records.
 */
import { readFileSync } from "node:fs";
import {
  classifyWorkspaceFile,
  createDomainLifecycle,
  parseActivationRequest,
  parseRuntimeCheckResult,
  workspaceDomainScan,
} from "../../../packages/core/src/domain/lifecycle.js";
import {
  parseDomainPackage,
  verifyPackageDigest,
} from "../../../packages/core/src/domain/package.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function failureRecord(failure) {
  const record = { code: failure.code };
  if (failure.code === "CAPABILITY_DENIED") {
    record.missing = [...failure.missing];
  }
  return record;
}

function packageRecord(package_) {
  return {
    id: package_.id,
    digest: package_.digest,
    abi: package_.abi,
    requestedCapabilities: [...package_.requestedCapabilities.ids],
  };
}

function activationRecord(active) {
  return {
    sessionId: active.sessionId,
    binding: {
      packageId: active.binding.packageId,
      digest: active.binding.digest,
      abi: active.binding.abi,
    },
    grant: [...active.grant.ids],
  };
}

function inspectRecord(lifecycle) {
  const installed = lifecycle.installedPackage();
  const active = lifecycle.active();
  return {
    op: "inspect",
    state: lifecycle.state(),
    available: lifecycle.available(),
    enabled: lifecycle.enabled(),
    active: active !== null,
    package: installed === null ? null : packageRecord(installed),
    activation: active === null ? null : activationRecord(active),
  };
}

const input = readStdinBounded();
const supportedAbi = input.supportedAbi;
const authority = input.authority;
const lifecycle = createDomainLifecycle();
const ops = [];
for (const entry of input.ops ?? []) {
  const op = entry.op;
  if (op === "inspect") {
    ops.push(inspectRecord(lifecycle));
    continue;
  }
  if (op === "install") {
    const packageValue = entry.package;
    const parsed = parseDomainPackage(
      packageValue.id,
      packageValue.digest,
      packageValue.abi,
      packageValue.requestedCapabilities,
    );
    if (!parsed.ok) {
      ops.push({ op: "install", ok: false, code: parsed.failure.code });
      continue;
    }
    const digestFailure = verifyPackageDigest(parsed.value.digest, entry.computedDigest);
    if (digestFailure !== null) {
      ops.push({ op: "install", ok: false, code: digestFailure.code });
      continue;
    }
    const failure = lifecycle.install(parsed.value);
    ops.push(
      failure === null
        ? { op: "install", ok: true, state: lifecycle.state() }
        : { op: "install", ok: false, ...failureRecord(failure) },
    );
    continue;
  }
  if (op === "uninstall" || op === "enable" || op === "disable" || op === "deactivate") {
    const failure = lifecycle[op]();
    ops.push(failure === null ? { op, ok: true } : { op, ok: false, ...failureRecord(failure) });
    continue;
  }
  if (op === "eligibility") {
    const activationValue = entry.activation;
    const parsedRequest = parseActivationRequest(
      activationValue.packageId,
      activationValue.digest,
      activationValue.abi,
      activationValue.capabilities,
    );
    if (!parsedRequest.ok) {
      ops.push({ op: "eligibility", ok: false, code: parsedRequest.failure.code });
      continue;
    }
    const parsedRuntime = parseRuntimeCheckResult(entry.runtime);
    if (!parsedRuntime.ok) {
      ops.push({ op: "eligibility", ok: false, code: parsedRuntime.failure.code });
      continue;
    }
    const eligibility = lifecycle.eligibility(
      parsedRequest.value,
      supportedAbi,
      { ids: [...authority].sort() },
      parsedRuntime.value,
    );
    ops.push({ op: "eligibility", ready: eligibility.ready, reasons: [...eligibility.reasons] });
    continue;
  }
  if (op === "activate") {
    const activationValue = entry.activation;
    const parsedRequest = parseActivationRequest(
      activationValue.packageId,
      activationValue.digest,
      activationValue.abi,
      activationValue.capabilities,
    );
    if (!parsedRequest.ok) {
      ops.push({ op: "activate", ok: false, code: parsedRequest.failure.code });
      continue;
    }
    const parsedRuntime = parseRuntimeCheckResult(entry.runtime);
    if (!parsedRuntime.ok) {
      ops.push({ op: "activate", ok: false, code: parsedRuntime.failure.code });
      continue;
    }
    const result = lifecycle.activate(
      parsedRequest.value,
      supportedAbi,
      { ids: [...authority].sort() },
      parsedRuntime.value,
    );
    ops.push(
      result.ok
        ? { op: "activate", ok: true, ...activationRecord(result.active) }
        : { op: "activate", ok: false, ...failureRecord(result.failure) },
    );
    continue;
  }
  if (op === "workspaceScan") {
    const files = entry.files ?? [];
    const classified = files.map((name) => ({ name, kind: classifyWorkspaceFile(name) }));
    const scan = workspaceDomainScan(files);
    ops.push({
      op: "workspaceScan",
      files: classified,
      candidates: scan.candidates,
      installs: scan.installs,
      enables: scan.enables,
      activations: scan.activations,
      downloads: scan.downloads,
      recommendations: scan.recommendations,
    });
    continue;
  }
  ops.push({ op, ok: false, code: "INVALID_INPUT" });
}
process.stdout.write(JSON.stringify({ ops }));
