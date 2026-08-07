export type { ConversationItem } from "./domain/conversation.js";
export { isCancellationError } from "./domain/cancellation.js";
export type { JsonObject, JsonPrimitive, JsonValue } from "./domain/json.js";
export type { ModelEvent, ModelProvider, ModelRequest } from "./ports/provider.js";
export {
  createToolRegistry,
  type RegisteredToolInfo,
  type ToolRegistry,
} from "./tools/tool-registry.js";
export type {
  Tool,
  ToolDefinition,
  ToolExecutionContext,
  ToolExecutionResult,
} from "./tools/tool.js";
export {
  createPreparedMutation,
  isPreparedMutationTool,
  toolCapability,
  type PreparedMutation,
  type PreparedMutationTool,
  type RegisteredTool,
  type ToolPreparationResult,
} from "./tools/prepared-mutation-tool.js";
export {
  createSolarisApplication,
  DEFAULT_MAX_TOOL_ROUNDS,
  type ApplicationEvent,
  type SessionStatus,
  type SolarisApplication,
  type SolarisApplicationDependencies,
} from "./application/application.js";
export type { Capability, CapabilityPolicy, PermissionRule } from "./security/capability.js";
export { createDefaultPolicy } from "./security/default-policy.js";
export type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "./security/approval.js";
export { DEFAULT_MAX_PENDING_APPROVAL_MS } from "./security/approval.js";
export type {
  ChangePreview,
  FileChangeOperation,
  FileChangePreview,
} from "./security/change-preview.js";
export {
  DEVELOP_OFFLINE_PROFILE,
  INSPECT_PROFILE,
  getBuiltInProfile,
  type SandboxProfile,
  type SandboxProfileId,
  type WorkspaceAccess,
} from "./security/profile.js";
export type { PermissionEvaluation } from "./security/permission-evaluator.js";
export { evaluatePermission } from "./security/permission-evaluator.js";
export type {
  SandboxBackend,
  SandboxBackendStatus,
  SandboxedProcessRequest,
  SandboxedProcessResult,
  SandboxedProcessStatus,
  SandboxViolation,
} from "./security/sandbox-backend.js";
export {
  normalizeSandboxError,
  SandboxError,
  type SandboxErrorCode,
} from "./security/sandbox-error.js";
export type { SandboxEvent } from "./security/sandbox-events.js";
export {
  createSolarisSecurity,
  type SolarisSecurity,
  type SolarisSecurityDependencies,
} from "./security/sandbox-service.js";
