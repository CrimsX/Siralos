import { readFileSync } from "node:fs";
import { createDefaultPolicy } from "../../../packages/core/src/security/default-policy.ts";
import { evaluatePermission } from "../../../packages/core/src/security/permission-evaluator.ts";
import {
  getBuiltInProfile,
  SANDBOX_PROFILE_IDS,
} from "../../../packages/core/src/security/profile.ts";
import { isProtectedBehavioralConfigPath } from "../../../packages/core/src/security/behavioral-config.ts";
import { canonicalizeCommandDigest } from "../../../packages/core/src/commands/command-digest.ts";
import { CAPABILITY_IDS } from "../../../packages/core/src/security/capability.ts";
import { sha256Hex } from "../../../packages/core/src/godot/digest.js";

const policy = (id) => createDefaultPolicy(id);

function decisionRecord(evaluation) {
  return evaluation.decision === "allow"
    ? { decision: "allow" }
    : { decision: evaluation.decision, reason: evaluation.reason };
}

function digestParts(overrides) {
  return {
    runnerId: "node-script",
    executableIdentity: "node@24.17.0 pinned",
    executableVersion: null,
    script: "tools/x.mjs",
    fileHash: "a".repeat(64),
    repositoryScript: null,
    arguments: ["--flag"],
    workingDirectory: "src/tools",
    profileId: "validation-offline",
    environmentPolicy: "minimal",
    timeoutMs: 600000,
    stdoutLimitBytes: 1000000,
    stderrLimitBytes: 1000000,
    stdinPolicy: "closed",
    networkPolicy: "deny",
    ...overrides,
  };
}

function runCase(inputCase) {
  const develop = getBuiltInProfile("develop-offline");
  const inspect = getBuiltInProfile("inspect");
  switch (inputCase.name) {
    case "allow-no-constraint":
      return decisionRecord(
        evaluatePermission("workspace.read", policy("develop-offline"), develop),
      );
    case "missing-rule-fails-closed": {
      const rules = { ...policy("inspect").rules };
      delete rules["self.inspect"];
      return decisionRecord(evaluatePermission("self.inspect", { rules }, inspect));
    }
    case "explicit-deny-research":
      return decisionRecord(evaluatePermission("research.fetch", policy("inspect"), inspect));
    case "ask-rule-godot-diagnose":
      return decisionRecord(
        evaluatePermission("godot.diagnose", policy("develop-offline"), develop),
      );
    case "process-profile-constraint":
      // Permitting rule, process-disabled profile: the constraint must deny.
      return decisionRecord(
        evaluatePermission("process.execute", policy("develop-offline"), inspect),
      );
    case "workspace-write-constraint":
      // Permitting rule, read-only profile: the constraint must deny.
      return decisionRecord(
        evaluatePermission("workspace.write", policy("develop-offline"), inspect),
      );
    case "network-universal-deny":
      return decisionRecord(
        evaluatePermission("network.outbound", policy("develop-offline"), develop),
      );
    case "policy-table-snapshot": {
      const profiles = SANDBOX_PROFILE_IDS.map((id) => {
        const rules = policy(id).rules;
        return { id, rules: CAPABILITY_IDS.map((capability) => [capability, rules[capability]]) };
      });
      return { profiles };
    }
    case "approval-digest-binding": {
      const base = canonicalizeCommandDigest(digestParts({}));
      const same = canonicalizeCommandDigest(digestParts({}));
      const changed = canonicalizeCommandDigest(digestParts({ timeoutMs: 601000 }));
      return {
        baseSha256: sha256Hex(base),
        sameBinding: same === base,
        changedBinding: changed === base,
      };
    }
    case "behavioral-config-classification":
      return {
        protected: inputCase.paths.map((path) => isProtectedBehavioralConfigPath(path)),
      };
    default:
      throw new Error(`unknown security-permissions fixture case ${inputCase.name}`);
  }
}

const input = JSON.parse(readFileSync(0, "utf8"));
const results = input.cases.map(runCase);
process.stdout.write(JSON.stringify({ cases: results }));
