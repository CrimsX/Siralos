export function formatTimeoutSeconds(timeoutMs: number): string {
  if (timeoutMs % 1000 === 0) {
    return `${timeoutMs / 1000} seconds`;
  }
  return `${(timeoutMs / 1000).toFixed(1)} seconds`;
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    const value = bytes / (1024 * 1024);
    return `${value} MiB`;
  }
  return `${Math.round(bytes / 1024)} KiB`;
}

export function formatDuration(durationMs: number): string {
  if (durationMs < 1000) {
    return `${durationMs}ms`;
  }
  return `${(durationMs / 1000).toFixed(1)}s`;
}

export function formatFileCount(count: number): string {
  return count.toLocaleString("en-US");
}

export function yesNo(value: boolean): string {
  return value ? "yes" : "no";
}

export function operationMark(operation: "create" | "update" | "delete"): string {
  return operation === "create" ? "A" : operation === "update" ? "M" : "D";
}
