export type ParsedValue<T> =
  | {
      readonly ok: true;
      readonly value: T;
    }
  | {
      readonly ok: false;
      readonly message: string;
    };

export function readJsonObject(input: unknown): ParsedValue<Record<string, unknown>> {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return { ok: false, message: "Tool input must be a JSON object." };
  }
  return { ok: true, value: input as Record<string, unknown> };
}

export function readOptionalString(
  record: Record<string, unknown>,
  key: string,
): ParsedValue<string | undefined> {
  const value = record[key];
  if (value === undefined) {
    return { ok: true, value: undefined };
  }
  if (typeof value !== "string") {
    return { ok: false, message: `"${key}" must be a string.` };
  }
  return { ok: true, value };
}

export function readRequiredString(
  record: Record<string, unknown>,
  key: string,
): ParsedValue<string> {
  const parsed = readOptionalString(record, key);
  if (!parsed.ok) {
    return parsed;
  }
  if (parsed.value === undefined || parsed.value.length === 0) {
    return { ok: false, message: `"${key}" is required.` };
  }
  return { ok: true, value: parsed.value };
}

export function readArrayField(
  record: Record<string, unknown>,
  key: string,
): ParsedValue<readonly unknown[]> {
  const value = record[key];
  if (!Array.isArray(value)) {
    return { ok: false, message: `"${key}" must be an array.` };
  }
  return { ok: true, value: value as readonly unknown[] };
}

export function readOptionalPositiveInteger(
  record: Record<string, unknown>,
  key: string,
): ParsedValue<number | undefined> {
  const value = record[key];
  if (value === undefined) {
    return { ok: true, value: undefined };
  }
  if (typeof value !== "number" || !Number.isInteger(value) || value < 1) {
    return { ok: false, message: `"${key}" must be a positive integer.` };
  }
  return { ok: true, value };
}
