import type {
  ApprovalRequest,
  Capability,
  CapabilityPolicy,
  RegisteredToolInfo,
  SandboxBackendStatus,
  SandboxProfile,
  SessionStatus,
  SolarisSecurity,
} from "@solaris/core";
import type { SandboxDoctorReport } from "./bootstrap/sandbox-doctor.js";

const CAPABILITIES: readonly Capability[] = [
  "workspace.read",
  "workspace.write",
  "process.execute",
  "network.outbound",
];

export function formatHeader(providerId: string): string {
  return `Solaris
Interactive Godot development harness
Provider: ${providerId}
`;
}

export function formatHelp(): string {
  return `Available commands:
  /help         Show this help
  /status       Show provider, session, and workspace status
  /clear        Clear the terminal (conversation is kept)
  /tools        List the available tools
  /sandbox      Show the sandbox backend status
  /permissions  Show capability rules
  /exit         Close Solaris
`;
}

export interface StatusView {
  readonly status: SessionStatus;
  readonly workspaceRoot: string;
  readonly toolCount: number;
  readonly providerToolCount: number;
  readonly profileId: string;
}

export function formatStatus(view: StatusView): string {
  const { status } = view;
  const sessionState = status.state === "responding" ? "responding" : "active";
  return `Provider: ${status.providerId}
Session: ${sessionState}
Messages: ${status.messageCount}
Workspace: ${view.workspaceRoot}
Sandbox: ${view.profileId}
Pending approval: ${status.pendingApproval ? "yes" : "no"}
Provider tools: ${view.providerToolCount}
Tools: ${view.toolCount}
`;
}

export function formatPermissions(policy: CapabilityPolicy, profileId: string): string {
  const lines = CAPABILITIES.map((capability) => {
    const rule = policy.rules[capability] ?? "deny";
    return `  ${capability.padEnd(18)} ${rule}`;
  });
  return `Profile: ${profileId}

${lines.join("\n")}

No provider-accessible process or write tool exists yet.
`;
}

export function formatSandbox(status: SandboxBackendStatus, profile: SandboxProfile): string {
  const lines = [
    `Profile: ${profile.id}`,
    `Backend: ${status.backendId}`,
    `Platform: ${status.platform}`,
    `State: ${status.state}`,
    `Version: ${status.version}`,
    `Filesystem read restriction: ${yesNo(status.capabilities.filesystemReadRestriction)}`,
    `Filesystem write restriction: ${yesNo(status.capabilities.filesystemWriteRestriction)}`,
    `Network restriction: ${yesNo(status.capabilities.networkRestriction)}`,
    `Process-tree restriction: ${yesNo(status.capabilities.processTreeRestriction)}`,
    `Violation reporting: ${yesNo(status.capabilities.violationReporting)}`,
    `Network: denied`,
    `Environment: minimal`,
  ];
  if (status.message !== undefined) {
    lines.push(`Setup: ${status.message}`);
  }
  if (status.state === "degraded") {
    lines.push("Warning: the sandbox backend is degraded.");
  }
  if (status.state === "failed") {
    lines.push("Warning: the sandbox backend failed its checks; nothing will run sandboxed.");
  }
  if (status.platform === "windows") {
    lines.push(
      "Warning: the native Windows backend is alpha; do not treat it as secure until Solaris conformance passes.",
    );
  }
  return `${lines.join("\n")}\n`;
}

export function formatSandboxViolation(category: string, summary: string): string {
  return `  \u26A0 sandbox violation (${category}): ${summary}\n`;
}

export function formatDoctor(report: SandboxDoctorReport): string {
  const lines = [
    "Solaris sandbox doctor",
    `Profile: ${report.profileId}`,
    `Backend: ${report.backendId}`,
    `Backend version: ${report.backendVersion}`,
    `Platform: ${report.platform}`,
    `State: ${report.state}`,
    `Filesystem read restriction: ${yesNo(report.capabilities.filesystemReadRestriction)}`,
    `Filesystem write restriction: ${yesNo(report.capabilities.filesystemWriteRestriction)}`,
    `Network restriction: ${yesNo(report.capabilities.networkRestriction)}`,
    `Process-tree restriction: ${yesNo(report.capabilities.processTreeRestriction)}`,
    `Violation reporting: ${yesNo(report.capabilities.violationReporting)}`,
  ];
  if (report.statusMessage !== null) {
    lines.push(`Setup requirements: ${report.statusMessage}`);
  }
  if (!report.probesRun) {
    lines.push("Live conformance: not run (use --sandbox-doctor --run-probes)");
  } else {
    lines.push("Live conformance: ran");
    if (report.conformance !== null) {
      for (const result of report.conformance.results) {
        const mark =
          result.outcome === "passed" ? "PASS" : result.outcome === "skipped" ? "SKIP" : "FAIL";
        lines.push(`  [${mark}] ${result.probeId}: ${result.description}`);
      }
      lines.push(
        `Result: ${report.conformance.passed} passed, ${report.conformance.failed} failed, ${report.conformance.skipped} skipped.`,
      );
    }
  }
  return `${lines.join("\n")}\n`;
}

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

export function formatTools(
  tools: readonly RegisteredToolInfo[],
  security: SolarisSecurity,
): string {
  if (tools.length === 0) {
    return "Available tools:\n  (none)\n";
  }
  const lines = tools.map((info) => {
    const kind = info.capability === "workspace.write" ? "write" : "read-only";
    const decision = security.evaluateCapability(info.capability);
    const status =
      decision.decision === "deny"
        ? "denied"
        : decision.decision === "ask"
          ? "approval required"
          : "allowed";
    return `  ${info.definition.name} - ${info.definition.description} (${kind}, ${status})`;
  });
  return `Available tools:\n${lines.join("\n")}\n`;
}

export function formatApprovalPrompt(request: ApprovalRequest): string {
  const file = request.preview.files[0];
  const lines = [
    "Approval required",
    "",
    `Tool: ${request.toolName}`,
    `Capability: ${request.capability}`,
    `File: ${file?.path ?? "(none)"}`,
    `Change: +${request.preview.totalAddedLines} -${request.preview.totalRemovedLines}`,
    "",
  ];
  if (file !== undefined) {
    lines.push(sanitizeForDisplay(file.unifiedDiff));
  }
  return `${lines.join("\n")}\n`;
}

export function formatInvalidCommand(input: string): string {
  return `Unknown command: ${input}
Type /help for the list of available commands.
`;
}

export function formatProviderFailure(message: string): string {
  return `[error] ${message}
`;
}

export function formatToolStarted(toolName: string, displayInput: string): string {
  return `\u25CF ${toolName} ${displayInput}
`;
}

export function formatToolCompleted(summary: string): string {
  return `  ${summary}
`;
}

export function formatToolFailed(message: string): string {
  return `  \u2715 ${message}
`;
}

export function formatToolCancelled(): string {
  return "  \u2715 cancelled\n";
}

const CONTROL_CHARACTER_PATTERN = new RegExp(`[\u0000-\u001f\u007f]`, "g");

export function sanitizeForDisplay(text: string): string {
  return text.replace(CONTROL_CHARACTER_PATTERN, "\uFFFD");
}

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}
