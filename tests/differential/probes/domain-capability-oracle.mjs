/**
 * domain-capability oracle probe (differential harness, ADR 0033,
 * Stage 3R R6).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes capability scenarios against the REAL TypeScript reference
 * domain capability module (packages/core/src/domain/capability).
 * Thin scenario adapter: it wires the production policy function and
 * maps results to the canonical record vocabulary; it does not
 * reimplement grant semantics.
 *
 * Deterministic: decisions derive from declared authority and request
 * sets only.
 */
import { readFileSync } from "node:fs";
import {
  decideGrant,
  parseCapabilityRequest,
  parseHostAuthority,
} from "../../../packages/core/src/domain/capability.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();
const parsedAuthority = parseHostAuthority(input.authority);
if (!parsedAuthority.ok) {
  process.stdout.write(
    JSON.stringify({
      authority: [],
      ops: [{ op: "invalid", ok: false, code: parsedAuthority.failure.code }],
    }),
  );
} else {
  const ops = [];
  for (const entry of input.ops ?? []) {
    if (entry.op === "decide") {
      const parsedRequest = parseCapabilityRequest(entry.request);
      if (!parsedRequest.ok) {
        ops.push({ op: "decide", ok: false, code: parsedRequest.failure.code });
        continue;
      }
      const decision = decideGrant(parsedRequest.value, parsedAuthority.value);
      ops.push(
        decision.granted
          ? { op: "decide", granted: true, grant: [...decision.grant.ids] }
          : { op: "decide", granted: false, missing: [...decision.missing] },
      );
      continue;
    }
    if (entry.op === "inspectAuthority") {
      ops.push({ op: "inspectAuthority", authority: [...parsedAuthority.value.ids] });
      continue;
    }
    ops.push({ op: entry.op, ok: false, code: "INVALID_INPUT" });
  }
  process.stdout.write(JSON.stringify({ ops }));
}
