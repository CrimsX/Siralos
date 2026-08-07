import { lstat, stat } from "node:fs/promises";
import path from "node:path";
import { COMMAND_LIMITS } from "@solaris/core";
import type { ParsedValue } from "../tools/workspace/validation.js";
import { readJsonObject } from "../tools/workspace/validation.js";
import { resolveWorkspacePath } from "../tools/workspace/workspace-path.js";

export interface CommonCommandFields {
  readonly runner: string;
  readonly arguments: readonly string[];
  readonly workingDirectory: string;
  readonly timeoutMs: number;
}

export interface CommandWorkingDirectory {
  readonly workspaceRelativePath: string;
  readonly absolutePath: string;
}

function containsControlCharacter(text: string): boolean {
  for (const character of text) {
    const code = character.charCodeAt(0);
    if (
      code <= 0x08 ||
      code === 0x0b ||
      code === 0x0c ||
      (code >= 0x0e && code <= 0x1f) ||
      code === 0x7f
    ) {
      return true;
    }
  }
  return false;
}

export function parseCommonCommandFields(
  input: unknown,
  runnerId: string,
  allowedKeys: readonly string[],
): ParsedValue<CommonCommandFields> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const unknownKey = Object.keys(object.value).find((key) => !allowedKeys.includes(key));
  if (unknownKey !== undefined) {
    return {
      ok: false,
      message: `"${unknownKey}" is not supported by the ${runnerId} runner.`,
    };
  }
  if (object.value["runner"] !== runnerId) {
    return { ok: false, message: `"runner" must be "${runnerId}".` };
  }
  const arguments_ = parseCommandArguments(object.value);
  if (!arguments_.ok) {
    return arguments_;
  }
  const workingDirectory = readOptionalWorkingDirectory(object.value);
  if (!workingDirectory.ok) {
    return workingDirectory;
  }
  const timeoutMs = parseCommandTimeoutMs(object.value);
  if (!timeoutMs.ok) {
    return timeoutMs;
  }
  return {
    ok: true,
    value: {
      runner: runnerId,
      arguments: arguments_.value,
      workingDirectory: workingDirectory.value,
      timeoutMs: timeoutMs.value,
    },
  };
}

export function parseCommandArguments(
  record: Record<string, unknown>,
): ParsedValue<readonly string[]> {
  const value = record["arguments"];
  if (value === undefined) {
    return { ok: true, value: [] };
  }
  if (!Array.isArray(value)) {
    return { ok: false, message: '"arguments" must be an array of strings.' };
  }
  if (value.length > COMMAND_LIMITS.maxArguments) {
    return {
      ok: false,
      message: `Arguments must not exceed ${COMMAND_LIMITS.maxArguments} entries.`,
    };
  }
  let totalBytes = 0;
  for (const entry of value) {
    if (typeof entry !== "string") {
      return { ok: false, message: "Every argument must be a string." };
    }
    if (entry.includes("\0")) {
      return { ok: false, message: "Arguments must not contain NUL bytes." };
    }
    if (containsControlCharacter(entry)) {
      return { ok: false, message: "Arguments must not contain terminal control characters." };
    }
    const bytes = Buffer.byteLength(entry, "utf8");
    if (bytes > COMMAND_LIMITS.maxArgumentBytes) {
      return {
        ok: false,
        message: `An argument exceeds the ${COMMAND_LIMITS.maxArgumentBytes}-byte limit.`,
      };
    }
    totalBytes += bytes;
  }
  if (totalBytes > COMMAND_LIMITS.maxTotalArgumentBytes) {
    return {
      ok: false,
      message: `Arguments exceed the ${COMMAND_LIMITS.maxTotalArgumentBytes}-byte total limit.`,
    };
  }
  return { ok: true, value: value as readonly string[] };
}

function readOptionalWorkingDirectory(record: Record<string, unknown>): ParsedValue<string> {
  const value = record["workingDirectory"];
  if (value === undefined) {
    return { ok: true, value: "." };
  }
  if (typeof value !== "string") {
    return { ok: false, message: '"workingDirectory" must be a string.' };
  }
  if (value.length === 0) {
    return { ok: false, message: '"workingDirectory" must not be empty.' };
  }
  if (value.includes("\0")) {
    return { ok: false, message: "The working directory contains a null byte." };
  }
  return { ok: true, value };
}

export function parseCommandTimeoutMs(record: Record<string, unknown>): ParsedValue<number> {
  const value = record["timeoutMs"];
  if (value === undefined) {
    return { ok: true, value: COMMAND_LIMITS.defaultTimeoutMs };
  }
  if (typeof value !== "number" || !Number.isInteger(value)) {
    return { ok: false, message: '"timeoutMs" must be an integer number of milliseconds.' };
  }
  if (value < COMMAND_LIMITS.minTimeoutMs || value > COMMAND_LIMITS.maxTimeoutMs) {
    return {
      ok: false,
      message: `"timeoutMs" must be between ${COMMAND_LIMITS.minTimeoutMs} and ${COMMAND_LIMITS.maxTimeoutMs} milliseconds.`,
    };
  }
  return { ok: true, value };
}

/**
 * Resolve and validate a workspace-relative working directory. Symbolic
 * links and unsupported reparse points are rejected; the canonical host
 * path is returned privately for the sandbox backend.
 */
export async function resolveCommandWorkingDirectory(
  workspaceRoot: string,
  requested: string,
): Promise<ParsedValue<CommandWorkingDirectory>> {
  if (requested.length === 0) {
    return { ok: false, message: "The working directory must not be empty." };
  }
  const resolved = await resolveWorkspacePath(workspaceRoot, requested);
  if (resolved.status !== "resolved") {
    return { ok: false, message: resolved.message };
  }
  const rawPath = path.resolve(workspaceRoot, requested);
  let rawStats;
  try {
    rawStats = await lstat(rawPath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The working directory is not accessible: ${describeFsError(error)}`,
    };
  }
  if (rawStats.isSymbolicLink()) {
    return { ok: false, message: "The working directory must not be a symbolic link." };
  }
  let stats;
  try {
    stats = await stat(resolved.absolutePath);
  } catch (error: unknown) {
    return {
      ok: false,
      message: `The working directory is not accessible: ${describeFsError(error)}`,
    };
  }
  if (!stats.isDirectory()) {
    return { ok: false, message: "The working directory is not a directory." };
  }
  return {
    ok: true,
    value: {
      workspaceRelativePath: resolved.workspaceRelativePath,
      absolutePath: resolved.absolutePath,
    },
  };
}

function describeFsError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message.replace(/,\s*'[^']*'$/, "");
  }
  return "a filesystem error occurred";
}
