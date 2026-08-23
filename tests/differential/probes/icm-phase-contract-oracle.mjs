/**
 * icm.phase-contract oracle probe (differential harness, ADR 0033,
 * Stage 3R R10b).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises createPhaseContract / validateAuthorityProfile and the
 * pre-built PHASE_CONTRACTS registry from the REAL TypeScript reference
 * (packages/core/src/context/phase-contract.ts).
 */
import { readFileSync } from "node:fs";
import {
  PHASE_CONTRACTS,
  createPhaseContract,
} from "../../../packages/core/src/context/phase-contract.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op ?? "create";

if (op === "registry") {
  const registry = Object.values(PHASE_CONTRACTS)
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    .map((contract) => ({ id: contract.id, digest: contract.digest.value }));
  process.stdout.write(JSON.stringify({ ok: true, registry }));
} else if (op === "create") {
  const declared = input.contract;
  try {
    const contract = createPhaseContract({
      id: declared.id,
      version: declared.version,
      phase: declared.phase ?? "",
      inputs: (declared.inputs ?? []).map((entry) => ({
        artifactType: entry.artifactType,
        optional: entry.optional === true,
        reason: entry.reason ?? "",
      })),
      authority: {
        readOnly: declared.authority.readOnly === true,
        mutation: declared.authority.mutation,
        approvalGrant: declared.authority.approvalGrant === true,
        acceptanceAuthority: declared.authority.acceptanceAuthority === true,
        capabilityNarrowing: declared.authority.capabilityNarrowing ?? [],
      },
      process: (declared.process ?? []).map((entry) => ({
        id: entry.id,
        description: entry.description ?? "",
      })),
      outputs: (declared.outputs ?? []).map((entry) => ({
        artifactType: entry.artifactType,
        verificationKind: entry.verificationKind ?? "",
      })),
      verification: (declared.verification ?? []).map((entry) => ({
        id: entry.id,
        description: entry.description ?? "",
        evidenceClass: entry.evidenceClass ?? "",
      })),
      contextClasses: declared.contextClasses ?? [],
    });
    process.stdout.write(
      JSON.stringify({
        ok: true,
        id: contract.id,
        version: contract.version,
        digest: contract.digest.value,
      }),
    );
  } catch (error) {
    process.stdout.write(JSON.stringify({ ok: false, error: String(error.message) }));
  }
} else {
  throw new Error(`unknown icm.phase-contract op ${JSON.stringify(op)}`);
}
