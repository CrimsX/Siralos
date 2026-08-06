import type { ToolExecutionResult } from "../tools/tool.js";

export type ConversationItem =
  | {
      readonly type: "user_message";
      readonly content: string;
    }
  | {
      readonly type: "assistant_message";
      readonly content: string;
    }
  | {
      readonly type: "assistant_tool_call";
      readonly callId: string;
      readonly toolName: string;
      readonly input: unknown;
    }
  | {
      readonly type: "tool_result";
      readonly callId: string;
      readonly toolName: string;
      readonly result: ToolExecutionResult;
    };
