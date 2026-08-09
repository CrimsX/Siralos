export type SandboxProfileId =
  | "inspect"
  | "develop-offline"
  | "validation-offline"
  | "godot-probe-offline"
  | "godot-recovery-probe-offline"
  | "godot-diagnostics-offline"
  | "godot-lsp-local";

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
    /**
     * Loopback scope intent. `lsp-only` means loopback is limited to the
     * Solaris-to-Godot LSP channel; enforcement depends on backend
     * capabilities and is reported truthfully (never claimed when the
     * backend cannot enforce a port-specific rule). Absent means loopback
     * is not a permitted channel at all.
     */
    readonly loopback?: "denied" | "lsp-only";
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

/**
 * Internal effective profile for recovery-mode Godot project probes.
 *
 * The disposable project mirror is the only project directory visible to the
 * probed engine: the source workspace is never writable and, where the
 * backend can enforce a host-read allowlist, is excluded from readable roots
 * entirely. The mirror and the sandbox-private home/temp are the only
 * writable roots; network and loopback are denied; stdin is closed; and the
 * process tree is confined. This profile is never user-selectable and must
 * never be broadened by public configuration.
 */
export const GODOT_RECOVERY_PROBE_OFFLINE_PROFILE: SandboxProfile = {
  id: "godot-recovery-probe-offline",
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

/**
 * Internal effective profile for GDScript `--check-only` diagnostics.
 *
 * The disposable project mirror is the only project directory visible to
 * the checked engine: the source workspace is never writable and, where the
 * backend can enforce a host-read allowlist, is excluded from readable
 * roots entirely. The mirror and the sandbox-private home/temp are the only
 * writable roots; network and loopback are denied; stdin is closed; and the
 * process tree is confined. This profile is never user-selectable and must
 * never be broadened by public configuration.
 */
/**
 * Internal effective profile for a Godot GDScript LSP session.
 *
 * The disposable project mirror is the only project directory visible to
 * the recovery-mode editor; the source workspace is never writable and,
 * where the backend can enforce a host-read allowlist, is excluded from
 * readable roots entirely. External outbound network is denied; loopback is
 * intended to be limited to the Solaris-to-Godot LSP channel (`lsp-only`),
 * and when the backend cannot enforce that port-specific scope the session
 * reports the isolation as unverified and fails closed rather than
 * claiming port-specific isolation. This profile is never user-selectable
 * and must never be broadened by public configuration.
 */
export const GODOT_LSP_LOCAL_PROFILE: SandboxProfile = {
  id: "godot-lsp-local",
  filesystem: {
    workspaceAccess: "read-only",
    protectGitMetadata: true,
    protectSolarisMetadata: true,
    denySensitiveProjectFiles: true,
    excludeWorkspaceRead: true,
  },
  process: {
    enabled: true,
    timeoutMs: 30 * 60 * 1000,
    maxOutputBytes: 8 * 1024 * 1024,
  },
  network: {
    outbound: "deny",
    loopback: "lsp-only",
  },
  environment: {
    policy: "minimal",
  },
};

export const GODOT_DIAGNOSTICS_OFFLINE_PROFILE: SandboxProfile = {
  id: "godot-diagnostics-offline",
  filesystem: {
    workspaceAccess: "read-only",
    protectGitMetadata: true,
    protectSolarisMetadata: true,
    denySensitiveProjectFiles: true,
    excludeWorkspaceRead: true,
  },
  process: {
    enabled: true,
    timeoutMs: 30_000,
    maxOutputBytes: 8 * 1024 * 1024,
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
    case "godot-recovery-probe-offline":
      return GODOT_RECOVERY_PROBE_OFFLINE_PROFILE;
    case "godot-diagnostics-offline":
      return GODOT_DIAGNOSTICS_OFFLINE_PROFILE;
    case "godot-lsp-local":
      return GODOT_LSP_LOCAL_PROFILE;
  }
}
