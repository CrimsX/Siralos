/**
 * Versioned outcome protocol shared by the R2 runners and comparator.
 *
 * Scenario outcomes describe product behavior. Runner outcomes describe the
 * lifecycle of the isolated reference/candidate process. Keeping those layers
 * distinct prevents a product failure from being mistaken for a harness crash.
 *
 * Facade: the implementation lives in the cohesive `protocol/*.mjs`
 * submodules; this module re-exports the unchanged public surface.
 */
export {
  HARNESS_DIAGNOSTIC_PREFIX,
  RUNNER_OUTCOME,
  RUNNER_PROTOCOL_SCHEMA_VERSION,
  SCENARIO_OUTCOME,
} from "./protocol/core.mjs";
export {
  canonicalRecordDocument,
  harnessDiagnostic,
  parseCanonicalRecordDocument,
  validateOutcomeRecord,
} from "./protocol/records.mjs";
