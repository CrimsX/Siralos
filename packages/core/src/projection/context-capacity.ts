/**
 * Route/model working-context capacity (Stage 3 milestone 2).
 *
 * The context projector works against the configured/verified working
 * budget, never blindly against a model's advertised maximum. Where
 * routing is not mature enough for verified values, they stay null and
 * the working maximum is the authoritative bound.
 */

export interface ContextCapacity {
  /** Model-advertised context window in tokens; null when unknown. */
  readonly advertisedMaximum: number | null;
  /** Host-verified context window in tokens; null when unverified. */
  readonly verifiedMaximum: number | null;
  /** Authoritative working budget the projector may use. */
  readonly workingMaximum: number;
  /** Reserved output budget in tokens; null when unknown. */
  readonly maxOutputTokens: number | null;
}

export const DEFAULT_CONTEXT_WORKING_MAXIMUM = 32_768;
export const DEFAULT_CONTEXT_MAX_OUTPUT_TOKENS = 4_096;

/**
 * Deterministic route capacities. Routes map to the existing offline
 * profiles; a future routing milestone can introduce per-model values and
 * verification. The working maximum intentionally reserves headroom below
 * advertised limits so output and overhead never silently overflow.
 */
export function createRouteContextCapacity(route: string): ContextCapacity {
  switch (route) {
    case "develop-offline":
      return {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: DEFAULT_CONTEXT_WORKING_MAXIMUM,
        maxOutputTokens: DEFAULT_CONTEXT_MAX_OUTPUT_TOKENS,
      };
    default:
      return {
        advertisedMaximum: null,
        verifiedMaximum: null,
        workingMaximum: DEFAULT_CONTEXT_WORKING_MAXIMUM,
        maxOutputTokens: DEFAULT_CONTEXT_MAX_OUTPUT_TOKENS,
      };
  }
}
