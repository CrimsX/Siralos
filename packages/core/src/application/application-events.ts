import type { CommandApplicationEvent } from "../commands/command-events.js";

/** UI-neutral events emitted while Solaris handles one prompt. */
export type ApplicationEvent =
  | {
      readonly type: "response_started";
    }
  | {
      readonly type: "text_delta";
      readonly text: string;
    }
  | {
      readonly type: "response_completed";
    }
  | {
      readonly type: "response_cancelled";
    }
  | {
      readonly type: "response_failed";
      readonly message: string;
    }
  | {
      readonly type: "tool_started";
      readonly callId: string;
      readonly toolName: string;
      readonly displayInput: string;
    }
  | {
      readonly type: "tool_awaiting_approval";
      readonly callId: string;
      readonly toolName: string;
      readonly requestId: string;
    }
  | {
      readonly type: "tool_completed";
      readonly callId: string;
      readonly toolName: string;
      readonly summary: string;
    }
  | {
      readonly type: "tool_failed";
      readonly callId: string;
      readonly toolName: string;
      readonly message: string;
    }
  | {
      readonly type: "tool_cancelled";
      readonly callId: string;
      readonly toolName: string;
    }
  | {
      readonly type: "approval_requested";
      readonly requestId: string;
      readonly toolName: string;
      readonly capability:
        | "workspace.write"
        | "process.execute"
        | "godot.probe_project"
        | "godot.diagnose"
        | "godot.lsp";
      readonly summary: string;
    }
  | {
      readonly type: "approval_resolved";
      readonly requestId: string;
      readonly decision: "approved" | "denied" | "cancelled";
    }
  | {
      readonly type: "checkpoint_applied";
      readonly checkpointId: string;
      readonly path: string;
    }
  | {
      readonly type: "context_pressure";
      readonly state: "normal" | "warn" | "auto" | "hard";
      readonly estimatedTokens: number;
      readonly workingMaximum: number;
    }
  | CommandApplicationEvent;
