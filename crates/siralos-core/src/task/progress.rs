//! Host-observed progress tracker (R3).
//!
//! Distinguishes useful progress from repeated identical action/result
//! observations. A model turn by itself is not progress; progress is a
//! Host-observed state derived from typed observations within a bounded
//! window.

use std::collections::VecDeque;

use crate::task::model::{ProgressState, ProgressStateValue, limits};

/// One host-observed execution fact fed to the progress tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObservation {
    /// Canonical action identity.
    /// Action.
    pub action: String,
    /// Canonical result fingerprint: equal results produce equal values.
    /// Fingerprint.
    pub fingerprint: String,
    /// Host asserts this observation represents genuinely new useful
    /// state.
    /// Progress.
    pub progress: bool,
}

/// Internal progress state with the bounded observation window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalProgress {
    pub(crate) useful_observations: u64,
    pub(crate) repeated_actions: u64,
    pub(crate) state: ProgressStateValue,
    pub(crate) last_progress_at_ms: Option<i64>,
    pub(crate) stalled_at_ms: Option<i64>,
    pub(crate) window: VecDeque<(String, bool)>,
}

/// Create the initial internal progress state.
pub fn create_internal_progress() -> InternalProgress {
    InternalProgress {
        useful_observations: 0,
        repeated_actions: 0,
        state: ProgressStateValue::Healthy,
        last_progress_at_ms: None,
        stalled_at_ms: None,
        window: VecDeque::new(),
    }
}

/// Snapshot of the internal progress state.
pub fn progress_snapshot(progress: &InternalProgress) -> ProgressState {
    ProgressState {
        state: progress.state,
        useful_observations: progress.useful_observations,
        repeated_actions: progress.repeated_actions,
        last_progress_at_ms: progress.last_progress_at_ms,
        stalled_at_ms: progress.stalled_at_ms,
    }
}

/// Observe one host fact and return the updated snapshot.
pub fn observe_progress(
    progress: &mut InternalProgress,
    observation: &HostObservation,
    now: i64,
) -> ProgressState {
    let key = format!("{}:{}", observation.action, observation.fingerprint);
    let already_in_window =
        progress.window.iter().any(|(entry, _)| *entry == key);
    let fresh = observation.progress || !already_in_window;
    if fresh {
        progress.useful_observations += 1;
        progress.last_progress_at_ms = Some(now);
    }
    progress.window.push_back((key.clone(), fresh));
    if progress.window.len() > limits::PROGRESS_WINDOW_SIZE {
        progress.window.pop_front();
    }
    let occurrences =
        progress.window.iter().filter(|(entry, _)| *entry == key).count()
            as u64;
    let useful_in_window =
        progress.window.iter().filter(|(_, useful)| *useful).count() as u64;
    progress.repeated_actions = occurrences;
    if occurrences >= limits::PROGRESS_STALLED_REPETITIONS
        || useful_in_window == 0
    {
        progress.state = ProgressStateValue::Stalled;
        if progress.stalled_at_ms.is_none() {
            progress.stalled_at_ms = Some(now);
        }
    } else if occurrences >= limits::PROGRESS_DEGRADED_REPETITIONS {
        progress.state = ProgressStateValue::Degraded;
        progress.stalled_at_ms = None;
    } else {
        progress.state = ProgressStateValue::Healthy;
        progress.stalled_at_ms = None;
    }
    progress_snapshot(progress)
}

#[cfg(test)]
mod tests {
    use super::{
        create_internal_progress, observe_progress, progress_snapshot,
    };
    use crate::task::model::ProgressStateValue;
    use crate::task::model::limits::{
        PROGRESS_DEGRADED_REPETITIONS, PROGRESS_STALLED_REPETITIONS,
    };

    fn observation(action: &str) -> super::HostObservation {
        super::HostObservation {
            action: action.to_owned(),
            fingerprint: "same".to_owned(),
            progress: false,
        }
    }

    #[test]
    fn repeated_identical_observations_degrade_then_stall() {
        let mut internal = create_internal_progress();
        let first = observe_progress(&mut internal, &observation("a"), 1);
        assert_eq!(first.state, ProgressStateValue::Healthy);
        assert_eq!(
            first.useful_observations, 1,
            "first observation is useful"
        );
        for _ in 1..PROGRESS_DEGRADED_REPETITIONS {
            observe_progress(&mut internal, &observation("a"), 1);
        }
        let degraded = observe_progress(&mut internal, &observation("a"), 1);
        assert_eq!(degraded.state, ProgressStateValue::Degraded);
        assert_eq!(
            degraded.repeated_actions,
            PROGRESS_DEGRADED_REPETITIONS + 1,
            "the fourth identical observation keeps the run count"
        );
        let mut stalled = degraded;
        for _ in PROGRESS_DEGRADED_REPETITIONS..PROGRESS_STALLED_REPETITIONS {
            stalled = observe_progress(&mut internal, &observation("a"), 1);
        }
        assert_eq!(stalled.state, ProgressStateValue::Stalled);
        assert!(stalled.stalled_at_ms.is_some());
    }

    #[test]
    fn new_useful_observation_recovers_healthy() {
        let mut internal = create_internal_progress();
        for _ in 0..PROGRESS_STALLED_REPETITIONS {
            observe_progress(&mut internal, &observation("a"), 1);
        }
        let recovered = observe_progress(
            &mut internal,
            &super::HostObservation {
                action: "b".to_owned(),
                fingerprint: "other".to_owned(),
                progress: true,
            },
            2,
        );
        assert_eq!(recovered.state, ProgressStateValue::Healthy);
        assert_eq!(recovered.useful_observations, 2);
        assert_eq!(recovered.last_progress_at_ms, Some(2));
        assert!(recovered.stalled_at_ms.is_none());
    }

    #[test]
    fn snapshot_never_borrows_the_window() {
        let mut internal = create_internal_progress();
        observe_progress(
            &mut internal,
            &super::HostObservation {
                action: "a".to_owned(),
                fingerprint: "f".to_owned(),
                progress: true,
            },
            1,
        );
        let snapshot = progress_snapshot(&internal);
        assert_eq!(snapshot.useful_observations, 1);
        assert_eq!(snapshot.repeated_actions, 1);
    }
}
