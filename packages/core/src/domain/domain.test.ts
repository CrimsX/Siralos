/**
 * Reference-semantics tests for the generic domain lifecycle/capability
 * modules (Stage 3R R6). These prove the production TypeScript
 * reference behavior that the differential oracle probes exercise; the
 * Rust candidate mirrors the same semantics via siralos-core::domain.
 */

import { describe, expect, it } from "vitest";
import { decideGrant, parseCapabilityRequest, parseHostAuthority } from "./capability.js";
import { parseDomainAbi, parseDomainPackage, verifyPackageDigest } from "./package.js";
import {
  classifyWorkspaceFile,
  createDomainLifecycle,
  parseActivationRequest,
  parseRuntimeCheckResult,
  workspaceDomainScan,
  WORKSPACE_FILE_OPAQUE,
} from "./lifecycle.js";

const ABI = "siralos:domain-abi@1.0.0";

function digest(byte: number) {
  return String(byte).padStart(2, "0").repeat(32);
}

function packageDescriptor(id: string, digestValue: string) {
  const parsed = parseDomainPackage(id, digestValue, ABI, ["workspace-read"]);
  if (!parsed.ok) {
    throw new Error("test fixture package parse failure");
  }
  return parsed.value;
}

function request(id: string, digestValue: string) {
  const parsed = parseActivationRequest(id, digestValue, ABI, ["workspace-read"]);
  if (!parsed.ok) {
    throw new Error("test fixture request parse failure");
  }
  return parsed.value;
}

function authority() {
  const parsed = parseHostAuthority(["workspace-read"]);
  if (!parsed.ok) {
    throw new Error("test fixture authority parse failure");
  }
  return parsed.value;
}

describe("domain package identity", () => {
  it("validates identifiers, digests, and ABIs", () => {
    expect(parseDomainAbi(ABI).ok).toBe(true);
    expect(parseDomainAbi("siralos:domain-abi@1.1.0").ok).toBe(true);
    expect(parseDomainAbi("name@1.0.0").ok).toBe(false);
    expect(parseDomainAbi("siralos:domain-abi@1").ok).toBe(false);
    expect(parseDomainAbi("siralos:domain-abi").ok).toBe(false);
  });

  it("verifies the exact digest", () => {
    const a = digest(1);
    const b = digest(2);
    expect(verifyPackageDigest(a, a)).toBeNull();
    expect(verifyPackageDigest(a, b)?.code).toBe("IDENTITY_MISMATCH");
  });

  it("parses malformed descriptors as typed failures", () => {
    expect(parseDomainPackage("", digest(1), ABI, []).ok).toBe(false);
    expect(parseDomainPackage("conformance-domain", "nope", ABI, []).ok).toBe(false);
    expect(parseDomainPackage("conformance-domain", digest(1), ABI, ["bad id"]).ok).toBe(false);
  });
});

describe("domain capability decisions", () => {
  it("grants exactly the request within authority", () => {
    const authorityValue = parseHostAuthority(["workspace-read", "process-exec"]);
    const requestValue = parseCapabilityRequest(["workspace-read"]);
    expect(authorityValue.ok && requestValue.ok).toBe(true);
    if (!authorityValue.ok || !requestValue.ok) {
      return;
    }
    const decision = decideGrant(requestValue.value, authorityValue.value);
    expect(decision.granted).toBe(true);
    if (decision.granted) {
      expect(decision.grant.ids).toEqual(["workspace-read"]);
    }
  });

  it("denies with the ordered missing capabilities", () => {
    const authorityValue = parseHostAuthority(["workspace-read"]);
    const requestValue = parseCapabilityRequest(["workspace-read", "process-exec"]);
    expect(authorityValue.ok && requestValue.ok).toBe(true);
    if (!authorityValue.ok || !requestValue.ok) {
      return;
    }
    const decision = decideGrant(requestValue.value, authorityValue.value);
    expect(decision.granted).toBe(false);
    if (!decision.granted) {
      expect(decision.missing).toEqual(["process-exec"]);
    }
  });

  it("requests are deduplicated and sorted", () => {
    const requestValue = parseCapabilityRequest(["process-exec", "workspace-read", "process-exec"]);
    expect(requestValue.ok).toBe(true);
    if (requestValue.ok) {
      expect(requestValue.value.ids).toEqual(["process-exec", "workspace-read"]);
    }
  });
});

describe("domain lifecycle", () => {
  it("absent cannot enable or activate", () => {
    const lifecycle = createDomainLifecycle();
    expect(lifecycle.state()).toBe("absent");
    expect(lifecycle.available()).toBe(false);
    expect(lifecycle.enable()?.code).toBe("NOT_INSTALLED");
    const activation = lifecycle.activate(
      request("conformance-domain", digest(1)),
      ABI,
      authority(),
      { kind: "ready" },
    );
    expect(activation.ok).toBe(false);
    if (!activation.ok) {
      expect(activation.failure.code).toBe("NOT_INSTALLED");
    }
    expect(lifecycle.uninstall()?.code).toBe("NOT_INSTALLED");
    expect(lifecycle.deactivate()?.code).toBe("NOT_ACTIVE");
    const eligibility = lifecycle.eligibility(
      request("conformance-domain", digest(1)),
      ABI,
      authority(),
      { kind: "ready" },
    );
    expect(eligibility).toEqual({ ready: false, reasons: ["NOT_INSTALLED"] });
  });

  it("install is explicit and distinct from enable", () => {
    const lifecycle = createDomainLifecycle();
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest(1)))).toBeNull();
    expect(lifecycle.state()).toBe("installed");
    expect(lifecycle.available()).toBe(true);
    expect(lifecycle.enabled()).toBe(false);
    expect(lifecycle.active()).toBeNull();
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest(1)))?.code).toBe(
      "ALREADY_INSTALLED",
    );
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest(2)))?.code).toBe(
      "IDENTITY_MISMATCH",
    );
    const activation = lifecycle.activate(
      request("conformance-domain", digest(1)),
      ABI,
      authority(),
      { kind: "ready" },
    );
    expect(activation.ok).toBe(false);
    if (!activation.ok) {
      expect(activation.failure.code).toBe("DISABLED");
    }
  });

  it("enablement never implies authority", () => {
    const lifecycle = createDomainLifecycle();
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest(1)))).toBeNull();
    expect(lifecycle.enable()).toBeNull();
    expect(lifecycle.state()).toBe("enabled");
    expect(lifecycle.enable()?.code).toBe("ALREADY_ENABLED");
    const narrow = parseHostAuthority([]);
    expect(narrow.ok).toBe(true);
    if (!narrow.ok) {
      return;
    }
    const activation = lifecycle.activate(
      request("conformance-domain", digest(1)),
      ABI,
      narrow.value,
      { kind: "ready" },
    );
    expect(activation.ok).toBe(false);
    if (!activation.ok) {
      expect(activation.failure.code).toBe("CAPABILITY_DENIED");
    }
  });

  it("activation binds the exact identity and grant", () => {
    const lifecycle = createDomainLifecycle();
    const digest1 = digest(1);
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest1))).toBeNull();
    expect(lifecycle.enable()).toBeNull();
    const activation = lifecycle.activate(
      request("conformance-domain", digest1),
      ABI,
      authority(),
      { kind: "ready" },
    );
    expect(activation.ok).toBe(true);
    if (!activation.ok) {
      return;
    }
    expect(activation.active.sessionId).toBe(1);
    expect(activation.active.binding).toEqual({
      packageId: "conformance-domain",
      digest: digest1,
      abi: ABI,
    });
    expect(activation.active.grant.ids).toEqual(["workspace-read"]);
    expect(lifecycle.state()).toBe("active");
    // Active -> Active is rejected and the session is preserved.
    const again = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(again.ok).toBe(false);
    if (!again.ok) {
      expect(again.failure.code).toBe("ACTIVE");
    }
    expect(lifecycle.active()?.sessionId).toBe(1);
    expect(lifecycle.deactivate()).toBeNull();
    // Wrong digest fails before any semantic work.
    const wrong = lifecycle.activate(request("conformance-domain", digest(2)), ABI, authority(), {
      kind: "ready",
    });
    expect(wrong.ok).toBe(false);
    if (!wrong.ok) {
      expect(wrong.failure.code).toBe("IDENTITY_MISMATCH");
    }
    // An ABI that identifies neither the installed package nor the
    // Host fails closed as a package identity mismatch (the
    // package-ABI gate precedes the Host-compatibility gate).
    const incompatibleRequest = parseActivationRequest(
      "conformance-domain",
      digest1,
      "siralos:domain-abi@9.9.9",
      ["workspace-read"],
    );
    expect(incompatibleRequest.ok).toBe(true);
    if (!incompatibleRequest.ok) {
      return;
    }
    const incompatible = lifecycle.activate(incompatibleRequest.value, ABI, authority(), {
      kind: "ready",
    });
    expect(incompatible.ok).toBe(false);
    if (!incompatible.ok) {
      expect(incompatible.failure.code).toBe("IDENTITY_MISMATCH");
    }
    // Runtime checks gate activation.
    const exhausted = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "resource-exceeded",
      resource: "FUEL",
    });
    expect(exhausted.ok).toBe(false);
    if (!exhausted.ok) {
      expect(exhausted.failure.code).toBe("RESOURCE_EXCEEDED");
    }
  });

  it("activation is session scoped", () => {
    const lifecycle = createDomainLifecycle();
    const digest1 = digest(1);
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest1))).toBeNull();
    expect(lifecycle.enable()).toBeNull();
    const first = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(first.ok && first.active.sessionId === 1).toBe(true);
    expect(lifecycle.deactivate()).toBeNull();
    expect(lifecycle.state()).toBe("enabled");
    const second = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(second.ok && second.active.sessionId === 2).toBe(true);
    expect(lifecycle.uninstall()?.code).toBe("ACTIVE");
    expect(lifecycle.disable()?.code).toBe("ACTIVE");
    expect(lifecycle.deactivate()).toBeNull();
    expect(lifecycle.disable()).toBeNull();
    expect(lifecycle.uninstall()).toBeNull();
    expect(lifecycle.state()).toBe("absent");
  });

  it("eligibility accumulates deeper reasons in fixed order", () => {
    const lifecycle = createDomainLifecycle();
    const digest1 = digest(1);
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest1))).toBeNull();
    expect(
      lifecycle.eligibility(request("conformance-domain", digest1), ABI, authority(), {
        kind: "ready",
      }),
    ).toEqual({ ready: false, reasons: ["DISABLED"] });
    expect(lifecycle.enable()).toBeNull();
    const narrow = parseHostAuthority(["workspace-read"]);
    const badRequest = parseActivationRequest("conformance-domain", digest(2), ABI, [
      "workspace-read",
      "process-exec",
    ]);
    expect(narrow.ok && badRequest.ok).toBe(true);
    if (!narrow.ok || !badRequest.ok) {
      return;
    }
    const eligibility = lifecycle.eligibility(badRequest.value, ABI, narrow.value, {
      kind: "resource-exceeded",
      resource: "MEMORY",
    });
    expect(eligibility).toEqual({
      ready: false,
      reasons: [
        "IDENTITY_MISMATCH",
        "UNDECLARED_CAPABILITY",
        "CAPABILITY_DENIED",
        "RESOURCE_EXCEEDED",
      ],
    });
  });

  it("rejects active-to-active and reports active eligibility", () => {
    const lifecycle = createDomainLifecycle();
    const digest1 = digest(1);
    expect(lifecycle.install(packageDescriptor("conformance-domain", digest1))).toBeNull();
    expect(lifecycle.enable()).toBeNull();
    const first = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(first.ok && first.active.sessionId === 1).toBe(true);
    const second = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(second.ok).toBe(false);
    if (!second.ok) {
      expect(second.failure.code).toBe("ACTIVE");
    }
    // The original session is preserved exactly.
    const preserved = lifecycle.active();
    expect(preserved?.sessionId).toBe(1);
    expect(preserved?.binding.digest).toBe(digest1);
    // Active eligibility is explicitly not ready.
    const eligibility = lifecycle.eligibility(
      request("conformance-domain", digest1),
      ABI,
      authority(),
      { kind: "ready" },
    );
    expect(eligibility).toEqual({ ready: false, reasons: ["ACTIVE"] });
    // Reactivation works only after explicit deactivate.
    expect(lifecycle.deactivate()).toBeNull();
    const third = lifecycle.activate(request("conformance-domain", digest1), ABI, authority(), {
      kind: "ready",
    });
    expect(third.ok && third.active.sessionId === 2).toBe(true);
  });

  it("bounds activation requests by the package declaration", () => {
    const lifecycle = createDomainLifecycle();
    const digest1 = digest(1);
    const declared = parseDomainPackage("conformance-domain", digest1, ABI, [
      "workspace-read",
      "process-exec",
    ]);
    expect(declared.ok).toBe(true);
    if (!declared.ok) {
      return;
    }
    expect(lifecycle.install(declared.value)).toBeNull();
    expect(lifecycle.enable()).toBeNull();
    const bothAuthority = parseHostAuthority(["workspace-read", "process-exec"]);
    expect(bothAuthority.ok).toBe(true);
    if (!bothAuthority.ok) {
      return;
    }
    // Equal set succeeds.
    const equal = parseActivationRequest("conformance-domain", digest1, ABI, [
      "workspace-read",
      "process-exec",
    ]);
    expect(equal.ok).toBe(true);
    if (!equal.ok) {
      return;
    }
    const active = lifecycle.activate(equal.value, ABI, bothAuthority.value, { kind: "ready" });
    expect(active.ok).toBe(true);
    if (active.ok) {
      expect(active.active.grant.ids).toEqual(["process-exec", "workspace-read"]);
    }
    expect(lifecycle.deactivate()).toBeNull();
    // Strict subset succeeds.
    const subset = parseActivationRequest("conformance-domain", digest1, ABI, ["workspace-read"]);
    expect(subset.ok).toBe(true);
    if (!subset.ok) {
      return;
    }
    const narrowed = lifecycle.activate(subset.value, ABI, bothAuthority.value, { kind: "ready" });
    expect(narrowed.ok).toBe(true);
    if (narrowed.ok) {
      expect(narrowed.active.grant.ids).toEqual(["workspace-read"]);
    }
    expect(lifecycle.deactivate()).toBeNull();
    // Exceeding the declaration fails typed even when authority allows it.
    const exceeding = parseActivationRequest("conformance-domain", digest1, ABI, [
      "workspace-read",
      "process-exec",
      "network-access",
    ]);
    expect(exceeding.ok).toBe(true);
    if (!exceeding.ok) {
      return;
    }
    const denied = lifecycle.activate(exceeding.value, ABI, bothAuthority.value, { kind: "ready" });
    expect(denied.ok).toBe(false);
    if (!denied.ok && denied.failure.code === "UNDECLARED_CAPABILITY") {
      expect(denied.failure.missing).toEqual(["network-access"]);
    }
    expect(lifecycle.state()).toBe("enabled");
    // Host authority narrows independently.
    const narrowAuthority = parseHostAuthority(["workspace-read"]);
    expect(narrowAuthority.ok).toBe(true);
    if (!narrowAuthority.ok) {
      return;
    }
    const both = parseActivationRequest("conformance-domain", digest1, ABI, [
      "workspace-read",
      "process-exec",
    ]);
    expect(both.ok).toBe(true);
    if (!both.ok) {
      return;
    }
    const policyDenied = lifecycle.activate(both.value, ABI, narrowAuthority.value, {
      kind: "ready",
    });
    expect(policyDenied.ok).toBe(false);
    if (!policyDenied.ok && policyDenied.failure.code === "CAPABILITY_DENIED") {
      expect(policyDenied.failure.missing).toEqual(["process-exec"]);
    }
    // Undeclared capabilities are reported in canonical order.
    const unordered = parseActivationRequest("conformance-domain", digest1, ABI, [
      "network-access",
      "workspace-read",
      "process-exec",
      "telemetry",
    ]);
    expect(unordered.ok).toBe(true);
    if (!unordered.ok) {
      return;
    }
    const ordered = lifecycle.activate(unordered.value, ABI, bothAuthority.value, {
      kind: "ready",
    });
    expect(ordered.ok).toBe(false);
    if (!ordered.ok && ordered.failure.code === "UNDECLARED_CAPABILITY") {
      expect(ordered.failure.missing).toEqual(["network-access", "telemetry"]);
    }
  });
  it("workspace files are opaque and never acquire", () => {
    const files = ["scene.project", "main.ts", "README.md"];
    for (const file of files) {
      expect(classifyWorkspaceFile(file)).toBe(WORKSPACE_FILE_OPAQUE);
    }
    expect(workspaceDomainScan(files)).toEqual({
      classified: 3,
      candidates: 0,
      installs: 0,
      enables: 0,
      activations: 0,
      downloads: 0,
      recommendations: 0,
    });
  });

  it("parses runtime check results", () => {
    expect(parseRuntimeCheckResult("ready")).toEqual({ ok: true, value: { kind: "ready" } });
    expect(parseRuntimeCheckResult("resource-exceeded").ok).toBe(true);
    expect(parseRuntimeCheckResult("unavailable").ok).toBe(true);
    expect(parseRuntimeCheckResult("bogus").ok).toBe(false);
  });
});
