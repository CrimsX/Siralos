/** UI-neutral Godot application events. Emitted by the Godot inspector adapter. */
export type GodotApplicationEvent =
  | {
      readonly type: "godot_discovery_started";
    }
  | {
      readonly type: "godot_candidate_found";
      readonly installationId: string;
      readonly source: string;
    }
  | {
      readonly type: "godot_probe_started";
      readonly installationId: string;
      readonly probe: "version" | "help" | "api" | "recovery" | "knowledge" | "diagnostic";
    }
  | {
      readonly type: "godot_probe_completed";
      readonly installationId: string;
      readonly probe: "version" | "help" | "api" | "recovery" | "knowledge" | "diagnostic";
      readonly status: "success" | "degraded" | "failed";
    }
  | {
      readonly type: "godot_project_inspected";
      readonly detected: boolean;
      readonly warnings: number;
    };
