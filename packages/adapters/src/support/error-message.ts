/**
 * Returns a useful message for an unknown caught value without leaking object
 * serialization details. Callers retain ownership of the domain-specific
 * fallback so failures stay meaningful at their boundary.
 */
export function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message.length > 0 ? error.message : fallback;
}

/** Creates a boundary-local describer while keeping the normalization policy shared. */
export function createErrorDescriber(fallback: string): (error: unknown) => string {
  return (error) => errorMessage(error, fallback);
}

/** Preserves the conventional `String(value)` fallback used by diagnostic-only paths. */
export function stringifyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
