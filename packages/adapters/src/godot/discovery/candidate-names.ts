/** Fixed Godot executable names searched on PATH. Never broadened by projects. */
export function godotCandidateNames(platform: NodeJS.Platform): readonly string[] {
  if (platform === "win32") {
    return ["godot.exe", "godot4.exe", "godot-mono.exe", "godot4-mono.exe"];
  }
  return ["godot", "godot4", "godot-mono", "godot4-mono"];
}

/** PATH entry separator for the platform. */
export function pathListSeparator(platform: NodeJS.Platform): string {
  return platform === "win32" ? ";" : ":";
}
