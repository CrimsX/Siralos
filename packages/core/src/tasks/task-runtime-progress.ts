import type { ProgressState } from "./task-model.js";
import type { HostObservation } from "./task-runtime-model.js";
import type { InternalTaskProgress } from "./task-runtime-record.js";

export const PROGRESS_WINDOW_SIZE = 8;
export const PROGRESS_DEGRADED_REPETITIONS = 3;
export const PROGRESS_STALLED_REPETITIONS = 5;

export function createInternalTaskProgress(): InternalTaskProgress {
  return {
    usefulObservations: 0,
    repeatedActions: 0,
    state: "healthy",
    lastProgressAtMs: null,
    stalledAtMs: null,
    window: [],
  };
}

export function taskProgressSnapshot(progress: InternalTaskProgress): ProgressState {
  return {
    state: progress.state,
    usefulObservations: progress.usefulObservations,
    repeatedActions: progress.repeatedActions,
    lastProgressAtMs: progress.lastProgressAtMs,
    stalledAtMs: progress.stalledAtMs,
  };
}

export function observeTaskProgress(
  progress: InternalTaskProgress,
  observation: HostObservation,
  now: () => number,
): ProgressState {
  const key = `${observation.action}:${observation.fingerprint}`;
  const alreadyInWindow = progress.window.some((entry) => entry.key === key);
  const fresh = observation.progress === true || !alreadyInWindow;
  if (fresh) {
    progress.usefulObservations += 1;
    progress.lastProgressAtMs = now();
  }
  progress.window.push({ key, useful: fresh });
  if (progress.window.length > PROGRESS_WINDOW_SIZE) {
    progress.window.shift();
  }
  const occurrences = progress.window.filter((entry) => entry.key === key).length;
  const usefulInWindow = progress.window.filter((entry) => entry.useful).length;
  progress.repeatedActions = occurrences;
  if (occurrences >= PROGRESS_STALLED_REPETITIONS || usefulInWindow === 0) {
    progress.state = "stalled";
    if (progress.stalledAtMs === null) {
      progress.stalledAtMs = now();
    }
  } else if (occurrences >= PROGRESS_DEGRADED_REPETITIONS) {
    progress.state = "degraded";
    progress.stalledAtMs = null;
  } else {
    progress.state = "healthy";
    progress.stalledAtMs = null;
  }
  return taskProgressSnapshot(progress);
}
