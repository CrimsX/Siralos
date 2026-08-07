/** A bounded, sanitized diagnostic that is safe to surface to users and providers. */
export interface SafeDiagnostic {
  readonly severity: "info" | "warning" | "error";
  readonly message: string;
}
