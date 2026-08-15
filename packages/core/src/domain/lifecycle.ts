/**
 * Domain lifecycle state machine (Stage 3R R6).
 *
 * Availability, installation, enablement, and activation are
 * mechanically distinct and never conflated:
 *
 *   absent --install--> installed --enable--> enabled --activate--> active
 *
 * Invalid transitions are typed failures; no implicit transition
 * exists. Activation is run/session scoped and still requires the
 * exact package identity, a compatible protocol, the declared
 * capability requests, the Host policy decision, and the required
 * resource/runtime checks. Workspace contents never install, enable,
 * or activate a domain: R6 has no file heuristic (workspace contents
 * are opaque to the lifecycle).
 */

import {
  decideGrant,
  parseCapabilityRequest,
  type CapabilityGrant,
  type CapabilityRequest,
  type HostAuthority,
} from "./capability.js";
import {
  invalidInput,
  type DomainFailure,
  type ParseResult,
  type ResourceKind,
} from "./failure.js";
import {
  abiIsCompatible,
  parseDomainAbi,
  parseDomainPackageId,
  parsePackageDigest,
  type DomainAbi,
  type DomainPackage,
  type DomainPackageId,
  type PackageDigest,
} from "./package.js";

/** The lifecycle state of one domain slot. */
export type LifecycleState = "absent" | "installed" | "enabled" | "active";

/**
 * The result of the Host resource/runtime check at activation.
 */
export type RuntimeCheckResult =
  | { readonly kind: "ready" }
  | { readonly kind: "resource-exceeded"; readonly resource: ResourceKind }
  | { readonly kind: "unavailable" };

/** Parse a canonical runtime-check result value. */
export function parseRuntimeCheckResult(value: unknown): ParseResult<RuntimeCheckResult> {
  if (value === "ready") {
    return { ok: true, value: { kind: "ready" } };
  }
  if (value === "resource-exceeded") {
    return { ok: true, value: { kind: "resource-exceeded", resource: "FUEL" } };
  }
  if (value === "unavailable") {
    return { ok: true, value: { kind: "unavailable" } };
  }
  return invalidInput("invalid runtime check result");
}

/**
 * The exact identity a caller requests to activate: the package id,
 * the exact digest, the ABI, and the requested capabilities.
 */
export interface ActivationRequest {
  readonly packageId: DomainPackageId;
  readonly digest: PackageDigest;
  readonly abi: DomainAbi;
  readonly capabilities: CapabilityRequest;
}

/** Parse an activation request from untrusted values. */
export function parseActivationRequest(
  packageId: unknown,
  digest: unknown,
  abi: unknown,
  capabilities: unknown,
): ParseResult<ActivationRequest> {
  const parsedId = parseDomainPackageId(packageId);
  if (!parsedId.ok) {
    return parsedId;
  }
  const parsedDigest = parsePackageDigest(digest);
  if (!parsedDigest.ok) {
    return parsedDigest;
  }
  const parsedAbi = parseDomainAbi(abi);
  if (!parsedAbi.ok) {
    return parsedAbi;
  }
  const parsedRequest = parseCapabilityRequest(capabilities);
  if (!parsedRequest.ok) {
    return parsedRequest;
  }
  return {
    ok: true,
    value: {
      packageId: parsedId.value,
      digest: parsedDigest.value,
      abi: parsedAbi.value,
      capabilities: parsedRequest.value,
    },
  };
}

/** The exact package identity bound by one successful activation. */
export interface ActivationBinding {
  readonly packageId: DomainPackageId;
  readonly digest: PackageDigest;
  readonly abi: DomainAbi;
}

/** Build the binding for the exact request identity. */
export function bindingFromRequest(request: ActivationRequest): ActivationBinding {
  return {
    packageId: request.packageId,
    digest: request.digest,
    abi: request.abi,
  };
}

/** Whether a binding matches the installed package exactly. */
export function bindingMatches(binding: ActivationBinding, package_: DomainPackage): boolean {
  return (
    binding.packageId === package_.id &&
    binding.digest === package_.digest &&
    binding.abi === package_.abi
  );
}

/** One run/session-scoped active domain. */
export interface ActiveDomain {
  /** The monotonic session id of this activation. */
  readonly sessionId: number;
  /** The exact identity bound by this activation. */
  readonly binding: ActivationBinding;
  /** The effective capability grant (never wider than Host authority). */
  readonly grant: CapabilityGrant;
}

/** A typed reason why an activation is not eligible. */
export type EligibilityReason =
  | "NOT_INSTALLED"
  | "DISABLED"
  | "IDENTITY_MISMATCH"
  | "UNSUPPORTED_ABI"
  | "CAPABILITY_DENIED"
  | "RESOURCE_EXCEEDED"
  | "UNAVAILABLE";

/** The deterministic activation eligibility report. */
export interface Eligibility {
  /** Whether the request is eligible (no reasons). */
  readonly ready: boolean;
  /** The ordered reasons (fixed check order). */
  readonly reasons: readonly EligibilityReason[];
}

/**
 * The internal authoritative state: one explicit discriminated union,
 * so impossible boolean combinations cannot be constructed.
 */
type DomainState =
  | { readonly kind: "absent" }
  | { readonly kind: "installed"; readonly package: DomainPackage }
  | { readonly kind: "enabled"; readonly package: DomainPackage }
  | {
      readonly kind: "active";
      readonly package: DomainPackage;
      readonly active: ActiveDomain;
    };

/**
 * Host-owned domain lifecycle state (installation/enablement records
 * plus the current run/session-scoped activation).
 */
export interface DomainLifecycle {
  /** The current lifecycle state. */
  state(): LifecycleState;
  /** Whether a package is installed (regardless of enablement). */
  available(): boolean;
  /** Whether the installed package is enabled. */
  enabled(): boolean;
  /** The active domain session, if any. */
  active(): ActiveDomain | null;
  /** The installed package, if any. */
  installedPackage(): DomainPackage | null;
  /** Explicitly install a locally supplied, Host-verified package. */
  install(package_: DomainPackage): DomainFailure | null;
  /** Explicitly remove the installed package. Refused while active. */
  uninstall(): DomainFailure | null;
  /** Explicitly enable the installed package (grants no capability). */
  enable(): DomainFailure | null;
  /** Explicitly disable the installed package. Refused while active. */
  disable(): DomainFailure | null;
  /** Report activation eligibility: ready or ordered typed reasons. */
  eligibility(
    request: ActivationRequest,
    supportedAbi: DomainAbi,
    authority: HostAuthority,
    runtime: RuntimeCheckResult,
  ): Eligibility;
  /**
   * Activate the installed, enabled package for this session. Fails
   * closed (typed) on any of: wrong identity, stale digest,
   * incompatible ABI, capability denial, or a failed resource/runtime
   * check — before any semantic work.
   */
  activate(
    request: ActivationRequest,
    supportedAbi: DomainAbi,
    authority: HostAuthority,
    runtime: RuntimeCheckResult,
  ):
    | { readonly ok: true; readonly active: ActiveDomain }
    | { readonly ok: false; readonly failure: DomainFailure };
  /** End the current run/session-scoped activation. */
  deactivate(): DomainFailure | null;
}

function deepReasons(
  package_: DomainPackage,
  request: ActivationRequest,
  supportedAbi: DomainAbi,
  authority: HostAuthority,
  runtime: RuntimeCheckResult,
): EligibilityReason[] {
  const reasons: EligibilityReason[] = [];
  if (request.packageId !== package_.id || request.digest !== package_.digest) {
    reasons.push("IDENTITY_MISMATCH");
  }
  if (!abiIsCompatible(request.abi, supportedAbi)) {
    reasons.push("UNSUPPORTED_ABI");
  }
  if (!decideGrant(request.capabilities, authority).granted) {
    reasons.push("CAPABILITY_DENIED");
  }
  if (runtime.kind === "resource-exceeded") {
    reasons.push("RESOURCE_EXCEEDED");
  } else if (runtime.kind === "unavailable") {
    reasons.push("UNAVAILABLE");
  }
  return reasons;
}

/** Whether two package descriptors are exactly equal. */
function packagesEqual(a: DomainPackage, b: DomainPackage): boolean {
  if (a.id !== b.id || a.digest !== b.digest || a.abi !== b.abi) {
    return false;
  }
  if (a.requestedCapabilities.ids.length !== b.requestedCapabilities.ids.length) {
    return false;
  }
  return a.requestedCapabilities.ids.every(
    (id, index) => id === b.requestedCapabilities.ids[index],
  );
}

/** A fresh lifecycle with no package installed. */
export function createDomainLifecycle(): DomainLifecycle {
  let state: DomainState = { kind: "absent" };
  let nextSession = 1;

  function stateOf(): LifecycleState {
    return state.kind;
  }

  function packageOf(): DomainPackage | null {
    if (state.kind === "absent") {
      return null;
    }
    return state.package;
  }

  return {
    state: stateOf,
    available: () => state.kind !== "absent",
    enabled: () => state.kind === "enabled" || state.kind === "active",
    active: () => (state.kind === "active" ? state.active : null),
    installedPackage: packageOf,
    install(package_) {
      if (state.kind === "absent") {
        state = { kind: "installed", package: package_ };
        return null;
      }
      if (packagesEqual(state.package, package_)) {
        return { code: "ALREADY_INSTALLED" };
      }
      return {
        code: "IDENTITY_MISMATCH",
        detail: "a different package is installed; uninstall first",
      };
    },
    uninstall() {
      if (state.kind === "absent") {
        return { code: "NOT_INSTALLED" };
      }
      if (state.kind === "active") {
        return { code: "ACTIVE" };
      }
      state = { kind: "absent" };
      return null;
    },
    enable() {
      if (state.kind === "absent") {
        return { code: "NOT_INSTALLED" };
      }
      if (state.kind === "installed") {
        state = { kind: "enabled", package: state.package };
        return null;
      }
      return { code: "ALREADY_ENABLED" };
    },
    disable() {
      if (state.kind === "absent") {
        return { code: "NOT_INSTALLED" };
      }
      if (state.kind === "active") {
        return { code: "ACTIVE" };
      }
      if (state.kind === "installed") {
        return { code: "ALREADY_DISABLED" };
      }
      state = { kind: "installed", package: state.package };
      return null;
    },
    eligibility(request, supportedAbi, authority, runtime) {
      let reasons: EligibilityReason[];
      if (state.kind === "absent") {
        reasons = ["NOT_INSTALLED"];
      } else if (state.kind === "installed") {
        reasons = ["DISABLED"];
      } else {
        reasons = deepReasons(state.package, request, supportedAbi, authority, runtime);
      }
      return { ready: reasons.length === 0, reasons };
    },
    activate(request, supportedAbi, authority, runtime) {
      if (state.kind === "absent") {
        return { ok: false, failure: { code: "NOT_INSTALLED" } };
      }
      if (state.kind === "installed") {
        return { ok: false, failure: { code: "DISABLED" } };
      }
      if (request.packageId !== state.package.id) {
        return {
          ok: false,
          failure: {
            code: "IDENTITY_MISMATCH",
            detail: "requested package id does not match the installed package",
          },
        };
      }
      if (request.digest !== state.package.digest) {
        return {
          ok: false,
          failure: {
            code: "IDENTITY_MISMATCH",
            detail: "requested digest does not match the installed package",
          },
        };
      }
      if (!abiIsCompatible(request.abi, supportedAbi)) {
        return {
          ok: false,
          failure: { code: "UNSUPPORTED_ABI", expected: supportedAbi, found: request.abi },
        };
      }
      const decision = decideGrant(request.capabilities, authority);
      if (!decision.granted) {
        return {
          ok: false,
          failure: { code: "CAPABILITY_DENIED", missing: [...decision.missing] },
        };
      }
      if (runtime.kind === "resource-exceeded") {
        return { ok: false, failure: { code: "RESOURCE_EXCEEDED", kind: runtime.resource } };
      }
      if (runtime.kind === "unavailable") {
        return {
          ok: false,
          failure: { code: "UNAVAILABLE", reason: "domain runtime is unavailable" },
        };
      }
      const active: ActiveDomain = {
        sessionId: nextSession,
        binding: bindingFromRequest(request),
        grant: decision.grant,
      };
      nextSession += 1;
      state = { kind: "active", package: state.package, active };
      return { ok: true, active };
    },
    deactivate() {
      if (state.kind !== "active") {
        return { code: "NOT_ACTIVE" };
      }
      state = { kind: "enabled", package: state.package };
      return null;
    },
  };
}

/**
 * The classification kind of a workspace file with respect to domain
 * acquisition. R6 has exactly one kind: workspace files are opaque.
 */
export const WORKSPACE_FILE_OPAQUE = "opaque";

/**
 * Classify one workspace file name. R6 deliberately defines no domain
 * heuristic: every workspace file is opaque, so workspace contents can
 * never install, enable, activate, download, or recommend a domain.
 * This is the explicit absence of a heuristic, not a magic detection
 * rule.
 */
export function classifyWorkspaceFile(_name: string): string {
  return WORKSPACE_FILE_OPAQUE;
}

/**
 * The deterministic workspace domain scan report. All side-effect
 * counters are zero by construction: the scan is a pure function over
 * file names and has no access to any installation or activation
 * machinery.
 */
export interface WorkspaceDomainScan {
  /** The number of classified file names. */
  readonly classified: number;
  /** Domain candidates found (always zero). */
  readonly candidates: number;
  /** Implicit installations (always zero). */
  readonly installs: number;
  /** Implicit enablements (always zero). */
  readonly enables: number;
  /** Implicit activations (always zero). */
  readonly activations: number;
  /** Downloads (always zero). */
  readonly downloads: number;
  /** Recommendations (always zero). */
  readonly recommendations: number;
}

/**
 * Scan workspace file names for implicit domain acquisition. The
 * report proves that no filesystem heuristic grants Domain authority.
 */
export function workspaceDomainScan(files: readonly string[]): WorkspaceDomainScan {
  return {
    classified: files.length,
    candidates: 0,
    installs: 0,
    enables: 0,
    activations: 0,
    downloads: 0,
    recommendations: 0,
  };
}
