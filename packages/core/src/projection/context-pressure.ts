/**
 * Host-owned context pressure classification (Stage 3 milestone 2).
 *
 * Pressure is computed from the deterministic token estimate against the
 * working maximum, never from a provider rejection. `hard` blocks the
 * provider call entirely; `auto` requires deterministic reduction before
 * proceeding; `warn` permits proceeding while surfacing the condition.
 */

export type ContextPressureState = "normal" | "warn" | "auto" | "hard";

export interface ContextPressureLimits {
  /** Ratio of the working maximum that enters warn. */
  readonly warnRatio: number;
  /** Ratio of the working maximum that enters auto. */
  readonly autoRatio: number;
  /** Ratio of the working maximum that enters hard. */
  readonly hardRatio: number;
}

export const DEFAULT_CONTEXT_PRESSURE_LIMITS: ContextPressureLimits = {
  warnRatio: 0.7,
  autoRatio: 0.85,
  hardRatio: 1.0,
};

export interface ContextPressure {
  readonly state: ContextPressureState;
  readonly estimatedTokens: number;
  readonly workingMaximum: number;
  /** estimated / workingMaximum; 0 when the working maximum is 0. */
  readonly ratio: number;
}

export function classifyPressure(
  estimatedTokens: number,
  workingMaximum: number,
  limits: ContextPressureLimits = DEFAULT_CONTEXT_PRESSURE_LIMITS,
): ContextPressure {
  const ratio = workingMaximum <= 0 ? 1 : estimatedTokens / workingMaximum;
  let state: ContextPressureState;
  if (estimatedTokens >= workingMaximum * limits.hardRatio) {
    state = "hard";
  } else if (estimatedTokens >= workingMaximum * limits.autoRatio) {
    state = "auto";
  } else if (estimatedTokens >= workingMaximum * limits.warnRatio) {
    state = "warn";
  } else {
    state = "normal";
  }
  return { state, estimatedTokens, workingMaximum, ratio };
}
