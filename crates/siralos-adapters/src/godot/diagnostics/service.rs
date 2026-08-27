//! Authoritative read-only GDScript diagnostics service.
//!
//! On this stage engine probing is fail-closed, so no trusted engine can
//! ever be selected: every preparation truthfully reports `unsupported`
//! before requesting approval, execution refuses before creating a mirror
//! or launching Godot, and nothing is ever created or deleted. The
//! designed flow — static validation, digest freezing, one-time approval,
//! revalidation, sequential mirrored checks — becomes reachable only when
//! a mechanically identity-bound launch primitive exists; the deeper
//! machinery lands with that milestone rather than as unreachable code.

use siralos_godot::godot::{
    GodotCheckPreparationResult, GodotCheckPreparationStatus,
    GodotDiagnosticsExecutionContext, GodotDiagnosticsRequest,
    GodotDiagnosticsState, GodotDiagnosticsStatus, GodotDiagnosticsSupport,
    GodotProjectCheckResult, GodotProjectCheckRunStatus,
    PreparedGDScriptCheck,
};

/// Truthful reason reported for every check execution attempt while
/// launch and mirror lifecycle cannot be bound to verified objects.
pub const GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE: &str = "GDScript check-only diagnostics are unavailable on this platform: the exact approved Godot identity cannot be launched against exactly the approved mirrored script bytes, the disposable mirror cannot be constructed with exactly the approved bytes, and its cleanup cannot be bound to the exact created objects, because Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive. Nothing was created, nothing was deleted, and no engine was launched.";

const NO_SELECTED_INSTALLATION_MESSAGE: &str = "No trusted Godot installation is selected; GDScript diagnostics cannot run.";

const UNKNOWN_PREPARED_CHECK_MESSAGE: &str =
    "The prepared check is not valid for this session; prepare a new check.";

/// The operation was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDiagnosticsCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

/// The fail-closed GDScript diagnostics service.
#[derive(Debug, Clone)]
pub struct GodotDiagnosticsService {
    platform: String,
}

impl GodotDiagnosticsService {
    /// Create the production service over the fail-closed stack.
    pub fn new(platform: impl Into<String>) -> Self {
        Self { platform: platform.into() }
    }

    /// Truthful platform-level support state.
    pub fn support(&self) -> GodotDiagnosticsSupport {
        GodotDiagnosticsSupport {
            state: siralos_godot::godot::KnowledgeSupportState::Unavailable,
            reason: Some(GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE.to_owned()),
            platform: self.platform.clone(),
        }
    }

    /// Validate the requested scripts and freeze the prepared check.
    ///
    /// Without a selectable trusted engine this refuses as `unsupported`
    /// before any approval, mirror, or digest work.
    pub fn prepare(
        &mut self,
        _request: &GodotDiagnosticsRequest,
        cancelled: bool,
    ) -> Result<GodotCheckPreparationResult, GodotDiagnosticsCancelled> {
        if cancelled {
            return Err(GodotDiagnosticsCancelled {
                message: "The Godot project operation was aborted.".to_owned(),
            });
        }
        Ok(GodotCheckPreparationResult::NotReady {
            status: GodotCheckPreparationStatus::Unsupported,
            message: NO_SELECTED_INSTALLATION_MESSAGE.to_owned(),
        })
    }

    /// Execute one prepared check under the host-approved digest.
    ///
    /// No plan can exist while preparation is closed, so every unknown
    /// handle fails without claiming success; once a probe-capable
    /// milestone makes preparation reachable, this revalidates the fresh
    /// state against the frozen digest and then refuses with a typed
    /// `unavailable` outcome unless the platform can mechanically bind
    /// execution to the approved bytes.
    pub fn execute(
        &mut self,
        _check: &PreparedGDScriptCheck,
        _context: &GodotDiagnosticsExecutionContext,
    ) -> GodotProjectCheckResult {
        GodotProjectCheckResult::NotChecked {
            status: GodotProjectCheckRunStatus::Failed,
            message: UNKNOWN_PREPARED_CHECK_MESSAGE.to_owned(),
        }
    }

    /// Bounded in-memory diagnostics state.
    pub fn status(&self) -> GodotDiagnosticsStatus {
        GodotDiagnosticsStatus {
            state: GodotDiagnosticsState::Untrusted,
            last_result: None,
            last_manifest_digest: None,
            last_engine_version: None,
        }
    }

    /// Dispose all prepared checks (session shutdown, denial,
    /// supersession).
    pub fn dispose_all(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE, GodotDiagnosticsService,
    };
    use siralos_godot::godot::KnowledgeSupportState;
    use siralos_godot::godot::{
        GodotCheckPreparationResult, GodotCheckPreparationStatus,
        GodotDiagnosticsExecutionContext, GodotDiagnosticsRequest,
        GodotDiagnosticsState, GodotProjectCheckResult,
        GodotProjectCheckRunStatus, PreparedGDScriptCheck,
    };

    #[test]
    fn support_reports_unavailable_with_exact_reason() {
        let service = GodotDiagnosticsService::new("win32");
        let support = service.support();
        assert_eq!(support.state, KnowledgeSupportState::Unavailable);
        assert_eq!(
            support.reason.as_deref(),
            Some(GODOT_CHECK_EXECUTION_UNAVAILABLE_MESSAGE)
        );
        assert_eq!(support.platform, "win32");
    }

    #[test]
    fn prepare_refuses_as_unsupported_without_a_selected_engine() {
        let mut service = GodotDiagnosticsService::new("win32");
        let single = service
            .prepare(
                &GodotDiagnosticsRequest {
                    paths: Some(vec!["src/player.gd".to_owned()]),
                },
                false,
            )
            .expect("not cancelled");
        let project_wide = service
            .prepare(&GodotDiagnosticsRequest { paths: None }, false)
            .expect("not cancelled");
        for result in [single, project_wide] {
            assert_eq!(
                result,
                GodotCheckPreparationResult::NotReady {
                    status: GodotCheckPreparationStatus::Unsupported,
                    message:
                        "No trusted Godot installation is selected; GDScript diagnostics cannot run."
                            .to_owned()
                }
            );
        }
    }

    #[test]
    fn cancellation_propagates_before_refusal() {
        let mut service = GodotDiagnosticsService::new("win32");
        let error = service
            .prepare(&GodotDiagnosticsRequest::default(), true)
            .expect_err("cancelled");
        assert_eq!(error.message, "The Godot project operation was aborted.");
    }

    #[test]
    fn execute_fails_unknown_handles_without_claiming_success() {
        let mut service = GodotDiagnosticsService::new("win32");
        let check = PreparedGDScriptCheck::create(1);
        let context = GodotDiagnosticsExecutionContext {
            approved_digest: "a".repeat(64),
            cancelled: false,
        };
        let outcome = service.execute(&check, &context);
        assert_eq!(
            outcome,
            GodotProjectCheckResult::NotChecked {
                status: GodotProjectCheckRunStatus::Failed,
                message:
                    "The prepared check is not valid for this session; prepare a new check."
                        .to_owned()
            }
        );
    }

    #[test]
    fn status_is_untrusted_and_dispose_all_is_safe() {
        let mut service = GodotDiagnosticsService::new("linux");
        assert_eq!(service.status().state, GodotDiagnosticsState::Untrusted);
        assert_eq!(service.status().last_result, None);
        service.dispose_all();
        assert_eq!(service.status().state, GodotDiagnosticsState::Untrusted);
    }
}
