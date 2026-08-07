/** Explicit selection failures never fall back silently. */
export class GodotSelectionError extends Error {
  readonly code = "godot_selection_failed";
}
