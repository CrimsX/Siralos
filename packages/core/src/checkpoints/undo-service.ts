export type UndoOutcome =
  | {
      readonly type: "undone";
      readonly checkpointId: string;
      readonly path: string;
      /** Optional advisory detail (for example a leftover recoverable copy). */
      readonly message?: string;
    }
  | {
      readonly type: "denied";
      readonly checkpointId: string;
      readonly path: string;
    }
  | {
      readonly type: "cancelled";
      readonly checkpointId: string;
      readonly path: string;
    }
  | {
      readonly type: "conflict";
      readonly checkpointId: string;
      readonly path: string;
      readonly message: string;
    }
  | {
      readonly type: "failed";
      readonly checkpointId: string | null;
      readonly path: string | null;
      readonly message: string;
    };

export interface UndoService {
  undo(checkpointId?: string, signal?: AbortSignal): Promise<UndoOutcome>;
}
