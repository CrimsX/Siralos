//! The fail-closed GDScript language-session service.
//!
//! On this stage engine startup is fail-closed, so no trusted engine can
//! ever be selected: every preparation truthfully reports `unsupported`
//! before requesting approval, opening a port, or creating a mirror, and
//! the bounded session status stays `unavailable`. The designed flow —
//! prepared-session freezing, one-time approval, revalidation, loopback
//! startup, framed queries — becomes reachable only when a mechanically
//! identity-bound launch primitive exists; that machinery lands with that
//! milestone rather than as unreachable code.

use siralos_core::godot::{
    EMPTY_GDSCRIPT_LSP_CAPABILITIES, GdScriptSessionState,
    GdScriptSessionStatus, GodotCheckPreparationStatus,
};

/// Truthful reason reported for every session attempt while launch and
/// mirror lifecycle cannot be bound to verified objects.
pub const GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE: &str = "The Godot GDScript language session is unavailable on this platform: the exact approved Godot editor cannot be launched against exactly the approved mirrored project bytes, the disposable mirror cannot be constructed or cleaned up identity-bound, and the loopback LSP channel cannot be tied to a verified process identity, because Node and the pinned sandbox runtime offer no exec-by-handle, directory-relative create, or delete-by-handle primitive. Nothing was created, no port was opened, no engine was launched.";

const NO_SELECTED_INSTALLATION_MESSAGE: &str = "No trusted Godot installation is selected; the language session cannot start.";

/// The operation was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotLspServiceCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

/// The fail-closed language-session service.
#[derive(Debug, Clone)]
pub struct GodotLspService {
    platform: String,
}

impl GodotLspService {
    /// Create the production service over the fail-closed stack.
    pub fn new(platform: impl Into<String>) -> Self {
        Self { platform: platform.into() }
    }

    /// Truthful platform-level support state.
    pub fn support(&self) -> siralos_core::godot::GodotDiagnosticsSupport {
        siralos_core::godot::GodotDiagnosticsSupport {
            state: siralos_core::godot::KnowledgeSupportState::Unavailable,
            reason: Some(GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE.to_owned()),
            platform: self.platform.clone(),
        }
    }

    /// Prepare a language session; refuses as `unsupported` before any
    /// approval, port allocation, or mirror work.
    pub fn prepare(
        &mut self,
        cancelled: bool,
    ) -> Result<
        siralos_core::godot::GodotCheckPreparationResult,
        GodotLspServiceCancelled,
    > {
        if cancelled {
            return Err(GodotLspServiceCancelled {
                message: "The Godot project operation was aborted.".to_owned(),
            });
        }
        Ok(siralos_core::godot::GodotCheckPreparationResult::NotReady {
            status: GodotCheckPreparationStatus::Unsupported,
            message: NO_SELECTED_INSTALLATION_MESSAGE.to_owned(),
        })
    }

    /// Bounded in-memory session state for CLI/provider rendering.
    pub fn status(&self) -> GdScriptSessionStatus {
        GdScriptSessionStatus {
            state: GdScriptSessionState::Unavailable,
            session_id: None,
            engine_version: None,
            project_name: None,
            started_at_ms: None,
            idle_ms: None,
            capabilities: EMPTY_GDSCRIPT_LSP_CAPABILITIES,
            open_document_count: 0,
            diagnostic_count: 0,
            network_isolation:
                siralos_core::godot::GdScriptNetworkIsolation::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE, GodotLspService};
    use siralos_core::godot::{
        GdScriptSessionState, GodotCheckPreparationResult,
        GodotCheckPreparationStatus, KnowledgeSupportState,
    };

    #[test]
    fn support_reports_unavailable_with_exact_reason() {
        let service = GodotLspService::new("win32");
        let support = service.support();
        assert_eq!(support.state, KnowledgeSupportState::Unavailable);
        assert_eq!(
            support.reason.as_deref(),
            Some(GODOT_LSP_EXECUTION_UNAVAILABLE_MESSAGE)
        );
    }

    #[test]
    fn prepare_refuses_as_unsupported_without_a_selected_engine() {
        let mut service = GodotLspService::new("win32");
        let result = service.prepare(false).expect("not cancelled");
        assert_eq!(
            result,
            GodotCheckPreparationResult::NotReady {
                status: GodotCheckPreparationStatus::Unsupported,
                message:
                    "No trusted Godot installation is selected; the language session cannot start."
                        .to_owned()
            }
        );
        let error = service.prepare(true).expect_err("cancelled");
        assert_eq!(error.message, "The Godot project operation was aborted.");
    }

    #[test]
    fn status_stays_unavailable_with_no_session_facts() {
        let service = GodotLspService::new("linux");
        let status = service.status();
        assert_eq!(status.state, GdScriptSessionState::Unavailable);
        assert_eq!(status.session_id, None);
        assert_eq!(status.engine_version, None);
        assert_eq!(status.open_document_count, 0);
    }
}
