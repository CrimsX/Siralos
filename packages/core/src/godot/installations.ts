/** User-supplied edition hint. A hint only, never an authoritative result. */
export type GodotEditionHint = "standard" | "dotnet" | "unknown";

export type GodotInstallationSource =
  | "user-config"
  | "path"
  | "cli-path"
  | "cli-installation"
  | "environment-path"
  | "environment-installation"
  | "active-config";

/**
 * A validated Godot executable candidate.
 *
 * The canonical path is private to Siralos: it must never enter
 * provider-visible results. Provider results use installation ids and
 * executable fingerprints instead.
 */
export interface GodotInstallation {
  readonly id: string;
  /** Human-readable discovery source label. */
  readonly sourceLabel: string;
  /** Machine source for provenance tracking. */
  readonly source: GodotInstallationSource;
  /** Canonical absolute path of the executable (private). */
  readonly canonicalPath: string;
  readonly sizeBytes: number;
  /** Modification time in epoch milliseconds. */
  readonly modifiedAtMs: number;
  /** SHA-256 of the executable bytes. */
  readonly sha256: string;
  readonly editionHint: GodotEditionHint;
  readonly status: "valid" | "invalid";
  /** Present only for invalid candidates; bounded. */
  readonly error?: string;
}
