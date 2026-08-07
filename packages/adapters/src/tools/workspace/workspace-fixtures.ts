import { mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import type { CheckpointStore, JsonObject, JsonValue, ToolExecutionResult } from "@solaris/core";
import { expect } from "vitest";
import { createFilesystemCheckpointStore } from "../../checkpoints/filesystem/checkpoint-store.js";

const checkpointDirectories: string[] = [];

export async function cleanupTempCheckpointDirs(): Promise<void> {
  for (const directory of checkpointDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
}

export async function createTempCheckpointStore(
  workspaceRoot: string,
  options: { maxCheckpoints?: number; maxStorageBytes?: number } = {},
): Promise<CheckpointStore> {
  const rootDirectory = await mkdtemp(join(tmpdir(), "solaris-cp-store-"));
  checkpointDirectories.push(rootDirectory);
  return createFilesystemCheckpointStore({ workspaceRoot, rootDirectory, ...options });
}

export interface TempWorkspace {
  readonly root: string;
  cleanup(): Promise<void>;
}

export function expectSuccess(result: ToolExecutionResult): JsonObject {
  expect(result.status).toBe("success");
  if (result.status !== "success") {
    throw new Error(`Expected success, got ${result.status}.`);
  }
  return result.output as JsonObject;
}

export function fieldArray(output: JsonObject, key: string): JsonValue[] {
  const value = output[key];
  if (!Array.isArray(value)) {
    throw new Error(`Expected "${key}" to be an array.`);
  }
  return value as JsonValue[];
}

export function fieldBoolean(output: JsonObject, key: string): boolean {
  const value = output[key];
  if (typeof value !== "boolean") {
    throw new Error(`Expected "${key}" to be a boolean.`);
  }
  return value;
}

export function fieldNumber(output: JsonObject, key: string): number {
  const value = output[key];
  if (typeof value !== "number") {
    throw new Error(`Expected "${key}" to be a number.`);
  }
  return value;
}

export function objectOf(value: JsonValue): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Expected a JSON object.");
  }
  return value as JsonObject;
}

export function stringOf(value: JsonValue | undefined): string {
  if (typeof value !== "string") {
    throw new Error("Expected a string.");
  }
  return value;
}

export async function createTempWorkspace(): Promise<TempWorkspace> {
  const root = await mkdtemp(join(tmpdir(), "solaris-workspace-"));
  return {
    root,
    async cleanup() {
      await rm(root, { recursive: true, force: true });
    },
  };
}

export async function writeFixtureFiles(
  root: string,
  files: Readonly<Record<string, string | Buffer>>,
): Promise<void> {
  for (const [relativePath, content] of Object.entries(files)) {
    const fullPath = join(root, relativePath);
    await mkdir(dirname(fullPath), { recursive: true });
    await writeFile(fullPath, content);
  }
}

export async function createFile(
  root: string,
  relativePath: string,
  content: string,
): Promise<string> {
  const fullPath = join(root, relativePath);
  await mkdir(dirname(fullPath), { recursive: true });
  await writeFile(fullPath, content);
  return fullPath;
}

export async function createSymlink(target: string, linkPath: string): Promise<void> {
  await symlink(target, linkPath);
}

export function probeSymlinkSupport(): boolean {
  let supported = false;
  let probeDir: string | undefined;
  try {
    probeDir = mkdtempSync(join(tmpdir(), "solaris-symlink-probe-"));
    const target = join(probeDir, "target.txt");
    writeFileSync(target, "x");
    symlinkSync(target, join(probeDir, "link.txt"));
    supported = true;
  } catch {
    supported = false;
  } finally {
    if (probeDir !== undefined) {
      rmSync(probeDir, { recursive: true, force: true });
    }
  }
  return supported;
}

export const SYMLINKS_SUPPORTED = probeSymlinkSupport();
