export type SandboxProfileId = "inspect" | "develop-offline" | "validation-offline";

export type WorkspaceAccess = "read-only" | "read-write";

export interface SandboxProfile {
  readonly id: SandboxProfileId;
  readonly filesystem: {
    readonly workspaceAccess: WorkspaceAccess;
    readonly protectGitMetadata: boolean;
    readonly protectSolarisMetadata: boolean;
    readonly denySensitiveProjectFiles: boolean;
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

export function getBuiltInProfile(profileId: SandboxProfileId): SandboxProfile {
  switch (profileId) {
    case "inspect":
      return INSPECT_PROFILE;
    case "develop-offline":
      return DEVELOP_OFFLINE_PROFILE;
    case "validation-offline":
      return VALIDATION_OFFLINE_PROFILE;
  }
}
