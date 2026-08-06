export type SandboxErrorCode =
  | "sandbox_configuration_error"
  | "sandbox_setup_required"
  | "sandbox_dependency_missing"
  | "sandbox_unsupported"
  | "sandbox_degraded"
  | "sandbox_initialization_failed"
  | "sandbox_policy_denied"
  | "sandbox_execution_denied"
  | "sandbox_violation"
  | "sandbox_timeout"
  | "sandbox_cancelled"
  | "sandbox_output_limit"
  | "sandbox_cleanup_failed"
  | "unknown_sandbox_failure";

export class SandboxError extends Error {
  readonly code: SandboxErrorCode;
  readonly detail: unknown;

  constructor(code: SandboxErrorCode, message: string, detail: unknown = undefined) {
    super(message);
    this.name = "SandboxError";
    this.code = code;
    this.detail = detail;
  }
}

export function isCancellationName(name: string): boolean {
  return name === "AbortError";
}

export function normalizeSandboxError(error: unknown): SandboxError {
  if (error instanceof SandboxError) {
    return error;
  }
  if (error instanceof Error && isCancellationName(error.name)) {
    return new SandboxError("sandbox_cancelled", "The sandbox operation was cancelled.", error);
  }
  if (error instanceof Error && error.message.length > 0) {
    return new SandboxError("unknown_sandbox_failure", error.message, error);
  }
  return new SandboxError("unknown_sandbox_failure", "An unknown sandbox failure occurred.", error);
}
