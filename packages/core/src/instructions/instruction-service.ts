import type { ProjectInstruction, ResolvedInstructionSet } from "./instruction-model.js";

/**
 * Project-instruction service port (Stage 3 milestone 4).
 *
 * The port is implemented by the adapter layer (bounded filesystem
 * discovery); core owns the types and the resolution semantics. The CLI
 * and the projection wiring consume only this port — provider adapters
 * never discover AGENTS.md files themselves.
 */

export interface InstructionDiscoveryOutcome {
  readonly instructions: readonly ProjectInstruction[];
  /** True when a discovery bound was exhausted; truncation is never silent. */
  readonly truncated: boolean;
  readonly scannedDirectories: number;
  readonly scannedFiles: number;
}

export interface ProjectInstructionService {
  /** Discover the current instruction inventory (idempotent reload). */
  load(): Promise<InstructionDiscoveryOutcome>;
  /** Re-discover and refresh changed file revisions. */
  refresh(): Promise<InstructionDiscoveryOutcome>;
  /** All discovered instructions (most recent load). */
  instructions(): readonly ProjectInstruction[];
  /** Resolve instructions applicable to one workspace-relative path. */
  resolveForPath(workspaceRelativePath: string): Promise<ResolvedInstructionSet>;
  /** Resolve the union of instructions applicable to multiple paths. */
  resolveForPaths(paths: readonly string[]): Promise<ResolvedInstructionSet>;
  /** Digest over the discovered inventory; null before the first load. */
  revision(): string | null;
}
