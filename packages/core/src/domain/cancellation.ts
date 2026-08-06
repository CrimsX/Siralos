export function isCancellationError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}
