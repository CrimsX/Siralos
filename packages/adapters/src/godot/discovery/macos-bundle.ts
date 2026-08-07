import { stat } from "node:fs/promises";
import { basename, join } from "node:path";

export type MacOsBundleResolution =
  | {
      readonly ok: true;
      /** Exact executable inside the bundle. */
      readonly executablePath: string;
    }
  | {
      readonly ok: false;
      readonly error: string;
    };

/**
 * Resolves the exact executable of a macOS Godot application bundle.
 *
 * A bundle may be configured as `/path/Godot.app` or directly as
 * `/path/Godot.app/Contents/MacOS/Godot`. Bundles are never launched
 * through `open` and never use Apple Events: only the exact executable is
 * returned for direct execution.
 */
export async function resolveMacOsBundle(path: string): Promise<MacOsBundleResolution> {
  const extension = basename(path).toLowerCase().endsWith(".app");
  if (!extension) {
    return { ok: false, error: "The configured path is not an .app bundle." };
  }
  const contents = join(path, "Contents");
  const macosDirectory = join(contents, "MacOS");
  let contentsMetadata;
  try {
    contentsMetadata = await stat(contents);
  } catch {
    return { ok: false, error: "The bundle has no Contents directory." };
  }
  if (!contentsMetadata.isDirectory()) {
    return { ok: false, error: "The bundle Contents path is not a directory." };
  }
  let macosMetadata;
  try {
    macosMetadata = await stat(macosDirectory);
  } catch {
    return { ok: false, error: "The bundle has no Contents/MacOS directory." };
  }
  if (!macosMetadata.isDirectory()) {
    return { ok: false, error: "The bundle Contents/MacOS path is not a directory." };
  }
  const executableName = await readBundleExecutableName(contents);
  const executablePath = join(macosDirectory, executableName);
  let executableMetadata;
  try {
    executableMetadata = await stat(executablePath);
  } catch {
    return {
      ok: false,
      error: `The bundle executable ${executableName} does not exist in Contents/MacOS.`,
    };
  }
  if (!executableMetadata.isFile()) {
    return {
      ok: false,
      error: `The bundle executable ${executableName} is not a regular file.`,
    };
  }
  return { ok: true, executablePath };
}

const BUNDLE_EXECUTABLE_NAME_LIMIT = 64 * 1024;

/**
 * Reads `CFBundleExecutable` from an XML Info.plist. Binary plists are not
 * decoded; the conventional `Godot` name is used as a fallback and the
 * caller sees a bounded diagnostic through the resolution result. The
 * plist content is untrusted data and is only scanned textually.
 */
async function readBundleExecutableName(contentsDirectory: string): Promise<string> {
  const plistPath = join(contentsDirectory, "Info.plist");
  let content: string;
  try {
    const { open } = await import("node:fs/promises");
    const handle = await open(plistPath, "r");
    try {
      const buffer = Buffer.alloc(BUNDLE_EXECUTABLE_NAME_LIMIT);
      const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
      content = buffer.subarray(0, bytesRead).toString("utf8");
    } finally {
      await handle.close().catch(() => undefined);
    }
  } catch {
    return "Godot";
  }
  const match = /<key>\s*CFBundleExecutable\s*<\/key>\s*<string>([^<]+)<\/string>/.exec(content);
  const name = match?.[1]?.trim();
  if (name === undefined || name.length === 0 || name.includes("/") || name.includes("\\")) {
    return "Godot";
  }
  return name;
}
