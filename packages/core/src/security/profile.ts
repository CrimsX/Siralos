export type SandboxProfileId =
  "inspect" | "develop-offline" | "validation-offline" | "godot-probe-offline";

export type WorkspaceAccess = "read-only" | "read-write";

export interface SandboxProfile {
  readonly id: SandboxProfileId;
  readonly filesystem: {
    readonly workspaceAccess: WorkspaceAccess;
    readonly protectGitMetadata: boolean;
    readonly protectSolarisMetadata: boolean;
    readonly denySensitiveProjectFiles: boolean;
    /**
     * When true, the source workspace is excluded from the sandboxed
     * host-read allowlist where the backend can enforce an allowlist, so the
     * child cannot read the real workspace at all. Never writable is always
     * enforced; this extends the boundary to unreadable where supported.
     */
    readonly excludeWorkspaceRead: boolean;
  };
  readonly process: {
    readonly enabled: boolean;
    readonly timeoutMs: number;
    readonly maxOutputBytes: number;
  };
  readonly network: {
    readonly outbound: "deny";
  };
  readonly environment: {
    readonly policy: "minimal";
  };
}

export const INSPECT_PROFILE: SandboxProfile = {
  id: "inspect",
  filesystem: {
    workspaceAccess: "read-only",
    protectGitMetadata: false,
    protectSolarisMetadata: false,
    denySensitiveProjectFiles: false,
    excludeWorkspaceRead: false,
  },
  process: {
    enabled: false,
    timeoutMs: 30_000,
    maxOutputBytes: 1_000_000,
  },
  network: {
    outbound: "deny",
  },
  environment: {
    policy: "minimal",
  },
};

export const DEVELOP_OFFLINE_PROFILE: SandboxProfile = {
  id: "develop-offline",
  filesystem: {
    workspaceAccess: "read-write",
    protectGitMetadata: true,
    protectSolarisMetadata: true,
    denySensitiveProjectFiles: true,
    excludeWorkspaceRead: false,
  },
  process: {
    enabled: true,
    timeoutMs: 30_000,
    maxOutputBytes: 1_000_000,
  },
  network: {
    outbound: "deny",
  },
  environment: {
    policy: "minimal",
  },
};

/**
 * Internal execution profile used only for provider-accessible commands.
 *
 * Commands always run with a narrower effective profile than approved file
 * edits: the project workspace is readable but never writable, regardless of
 * the active user profile. This profile is never user-selectable and must
 * never be broadened by public configuration.
 */
export const VALIDATION_OFFLINE_PROFILE: SandboxProfile = {
  id: "validation-offline",
  filesystem: {
    workspaceAccess: "read-only",
    protectGitMetadata: true,
    protectSolarisMetadata: true,
    denySensitiveProjectFiles: true,
    excludeWorkspaceRead: false,
  },
  process: {
    enabled: true,
    timeoutMs: 600_000,
    maxOutputBytes: 1_000_000,
  },
  network: {
    outbound: "deny",
  },
  environment: {
    policy: "minimal",
  },
};

/**
 * Internal effective profile for Godot engine probes.
 *
 * Godot probes are project-independent, read-only, offline, and fixed by
 * Solaris: they run in a Solaris-private probe directory with a sandbox
 * private home and temp, the project workspace is never writable and, where
 * the backend can enforce a host-read allowlist, is excluded from readable
 * roots entirely (the probe executes a verified private copy inside its run
 * directory and needs no workspace or installation-parent access), network
 * and loopback are denied, stdin is closed, and the process tree is
 * confined. This profile is never user-selectable and must never be
 * broadened by public configuration.
 */
export const GODOT_PROBE_OFFLINE_PROFILE: SandboxProfile = {
  id: "godot-probe-offline",
  filesystem: {
    workspaceAccess: "read-only",
    protectGitMetadata: true,
    protectSolarisMetadata: true,
    denySensitiveProjectFiles: true,
    excludeWorkspaceRead: true,
  },
  process: {
    enabled: true,
    timeoutMs: 120_000,
    maxOutputBytes: 1_000_000,
  },
  network: {
    outbound: "deny",
  },
  environment: {
    policy: "minimal",
  },
};

export function getBuiltInProfile(profileId: SandboxProfileId): SandboxProfile {
  switch (profileId) {
    case "inspect":
      return INSPECT_PROFILE;
    case "develop-offline":
      return DEVELOP_OFFLINE_PROFILE;
    case "validation-offline":
      return VALIDATION_OFFLINE_PROFILE;
    case "godot-probe-offline":
      return GODOT_PROBE_OFFLINE_PROFILE;
  }
}
