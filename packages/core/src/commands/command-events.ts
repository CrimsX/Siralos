/**
 * UI-neutral application events for provider-accessible commands.
 *
 * No ANSI codes are ever embedded; the CLI sanitizes and renders them.
 * Full command output is never persisted by this milestone.
 */
export type CommandApplicationEvent =
  | {
      readonly type: "command_prepared";
      readonly commandId: string;
      readonly runnerId: string;
      readonly summary: string;
    }
  | {
      readonly type: "command_started";
      readonly commandId: string;
      readonly runnerId: string;
      readonly displayName: string;
      readonly digestPrefix: string;
    }
  | {
      readonly type: "command_stdout";
      readonly commandId: string;
      readonly text: string;
    }
  | {
      readonly type: "command_stderr";
      readonly commandId: string;
      readonly text: string;
    }
  | {
      readonly type: "command_completed";
      readonly commandId: string;
      readonly exitCode: number;
      readonly durationMs: number;
    }
  | {
      readonly type:
        | "command_denied"
        | "command_conflict"
        | "command_cancelled"
        | "command_timed_out"
        | "command_failed";
      readonly commandId: string;
      readonly message: string;
    };

/** Bounded in-memory session metadata for completed commands. */
export interface CommandAuditRecord {
  readonly commandId: string;
  readonly runnerId: string;
  readonly summary: string;
  readonly digest: string;
  readonly startedAt: number;
  readonly durationMs: number | null;
  readonly exitCode: number | null;
  readonly outcome: string;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
}

export const MAX_RETAINED_COMMAND_AUDIT_RECORDS = 20;
