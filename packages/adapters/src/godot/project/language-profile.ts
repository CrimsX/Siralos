import { basename } from "node:path";
import type { GodotLanguageProfile } from "@solaris/core";
import { scanProjectFiles, type BoundedScanResult } from "./bounded-scan.js";

export interface LanguageProfileResult {
  readonly profile: GodotLanguageProfile;
  /** Bounded static evidence descriptions. */
  readonly evidence: readonly string[];
  readonly truncated: boolean;
}

/**
 * Static language-profile detection from bounded evidence. A project is
 * never claimed GDScript-only merely because no `.cs` file was found in a
 * partial traversal: truncation is reported and treated as uncertainty.
 * `dotnet` project settings and declared `C#` features are project.godot
 * evidence; dotnet is never invoked and C# projects are never parsed
 * deeply.
 */
export async function detectLanguageProfile(options: {
  readonly workspaceRoot: string;
  readonly signal?: AbortSignal;
  readonly dotnetAssemblyName: string | null;
  readonly declaredFeatures: readonly string[];
  readonly scan?: Promise<BoundedScanResult>;
}): Promise<LanguageProfileResult> {
  const evidence: string[] = [];
  const scan =
    options.scan ??
    scanProjectFiles({
      workspaceRoot: options.workspaceRoot,
      ...(options.signal === undefined ? {} : { signal: options.signal }),
      includeDotnet: true,
    });
  const result = await scan;
  let gdscript = false;
  let dotnet = false;
  for (const file of result.files) {
    const name = basename(file).toLowerCase();
    if (name.endsWith(".gd")) {
      gdscript = true;
    }
    if (name.endsWith(".cs") || name.endsWith(".csproj") || name.endsWith(".sln")) {
      dotnet = true;
    }
  }
  if (options.dotnetAssemblyName !== null) {
    dotnet = true;
    evidence.push("[dotnet] project/assembly_name is present in project.godot");
  }
  const declaredCSharp = options.declaredFeatures.some((feature) => feature.toLowerCase() === "c#");
  if (declaredCSharp) {
    dotnet = true;
    evidence.push("the project declares the C# feature");
  }
  if (gdscript) {
    evidence.push("bounded traversal found .gd source files");
  }
  if (dotnet) {
    evidence.push("bounded traversal or project settings show .NET evidence");
  }
  let profile: GodotLanguageProfile = "unknown";
  if (gdscript && dotnet) {
    profile = "mixed";
  } else if (gdscript) {
    profile = "gdscript";
  } else if (dotnet) {
    profile = "dotnet";
  }
  return { profile, evidence, truncated: result.truncated };
}
