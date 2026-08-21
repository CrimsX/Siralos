//! API documentation generation fails closed and never spawns the
//! executable.
//!
//! The fixed probe would run `--dump-extension-api-with-docs` in a
//! Siralos-private probe directory with network denied and the workspace
//! excluded from readable roots; until launch can be mechanically bound
//! to the verified executable identity and the probe directory to a
//! verified parent, every generation reports a typed `unavailable`
//! outcome and nothing is created or deleted. An ordinary
//! `--dump-extension-api` result is never substituted.

use siralos_core::godot::{
    GodotEdition, GodotEngineProfile, GodotInstallation,
};
use siralos_core::identity::sha256_hex;

/// Fixed Siralos-owned API documentation generation invocation.
pub const GODOT_KNOWLEDGE_BASE_ARGUMENTS: [&str; 1] =
    ["--dump-extension-api-with-docs"];

/// The only argument tuple the with-docs probe may pass.
pub fn godot_knowledge_arguments() -> Vec<String> {
    GODOT_KNOWLEDGE_BASE_ARGUMENTS
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

/// Siralos-chosen aspects of the knowledge command bound by the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeCommandDigestParts {
    /// Executable SHA-256.
    pub executable_sha256: String,
    /// Fixed argument tuple.
    pub argument_template: Vec<String>,
    /// Knowledge profile id.
    pub profile_id: String,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Stdout limit in bytes.
    pub stdout_limit_bytes: u64,
    /// Stderr limit in bytes.
    pub stderr_limit_bytes: u64,
}

/// Deterministic digest over the fixed knowledge command.
pub fn compute_godot_knowledge_command_digest(
    parts: &GodotKnowledgeCommandDigestParts,
) -> String {
    let value = serde_json::json!({
        "executableSha256": parts.executable_sha256,
        "argumentTemplate": parts.argument_template,
        "workingDirectoryPolicy": "siralos-private-probe-directory",
        "profileId": parts.profile_id,
        "environmentPolicy": "minimal",
        "stdinPolicy": "closed",
        "networkPolicy": "denied",
        "timeoutMs": parts.timeout_ms,
        "stdoutLimitBytes": parts.stdout_limit_bytes,
        "stderrLimitBytes": parts.stderr_limit_bytes,
    });
    sha256_hex(value.to_string().as_bytes())
}

/// The generation request was cancelled before refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeCancelled {
    /// Bounded cancellation message.
    pub message: String,
}

/// Observable outcomes of the fail-closed knowledge runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotKnowledgeRunOutcome {
    /// The engine cannot generate documentation as specified.
    Unsupported {
        /// Bounded truthful reason.
        message: String,
    },
    /// Generation is unavailable under the current enforcement boundary.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Inputs to one generation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotKnowledgeRunRequest<'a> {
    /// Selected installation.
    pub installation: &'a GodotInstallation,
    /// Selected engine profile.
    pub engine_profile: &'a GodotEngineProfile,
    /// Host-owned cancellation observation; cancelled runs refuse before
    /// any precondition evaluation.
    pub cancelled: bool,
}

/// The fail-closed knowledge generation runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailClosedGodotKnowledgeRunner;

impl FailClosedGodotKnowledgeRunner {
    /// Reports whether documentation generation can run at all.
    #[must_use]
    pub fn is_available(&self) -> bool {
        false
    }

    /// Refuse the run without spawning anything or creating a probe
    /// directory.
    pub fn generate_documentation(
        &self,
        request: GodotKnowledgeRunRequest<'_>,
    ) -> Result<GodotKnowledgeRunOutcome, GodotKnowledgeCancelled> {
        if request.cancelled {
            return Err(GodotKnowledgeCancelled {
                message: "The Godot API documentation generation was aborted."
                    .to_owned(),
            });
        }
        if let Some(message) = require_knowledge_capabilities(request) {
            return Ok(GodotKnowledgeRunOutcome::Unsupported { message });
        }
        Ok(GodotKnowledgeRunOutcome::Unavailable {
            message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE.to_owned(),
        })
    }
}

/// Create the fail-closed knowledge generation runner.
pub fn create_godot_knowledge_runner() -> FailClosedGodotKnowledgeRunner {
    FailClosedGodotKnowledgeRunner
}

/// Truthful reason reported for every generation attempt while launch
/// and probe-directory lifecycle cannot be bound to verified objects.
pub const GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE: &str = "Exact-engine API documentation generation is unavailable: Node and the pinned sandbox runtime offer no exec-by-handle or directory-handle-relative primitive, so the staged executable's pathname is re-opened at spawn time and a same-user process could substitute different bytes between final verification and launch, and the Siralos-private probe directory cannot be created or cleaned up identity-bound. The verified fingerprint could then be attached to bytes that never execute. Generation fails closed and the executable is never spawned; no probe directory is created. It will become available only when a mechanically identity-bound launch and directory-lifecycle primitive exists.";

fn require_knowledge_capabilities(
    request: GodotKnowledgeRunRequest<'_>,
) -> Option<String> {
    if !request.installation.status_valid {
        return Some(
            "The installation is invalid; rediscovery is required.".to_owned(),
        );
    }
    if request.engine_profile.edition == GodotEdition::RuntimeOnly {
        return Some(
            "The selected executable is runtime-only; it cannot generate the extension API documentation."
                .to_owned(),
        );
    }
    if !request.engine_profile.capabilities.extension_api_with_docs_dump {
        return Some(
            "The selected Godot version does not advertise --dump-extension-api-with-docs; exact-engine API documentation is unsupported and an ordinary --dump-extension-api result is never substituted."
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_KNOWLEDGE_BASE_ARGUMENTS,
        GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE,
        GodotKnowledgeCommandDigestParts, GodotKnowledgeRunOutcome,
        compute_godot_knowledge_command_digest, create_godot_knowledge_runner,
        godot_knowledge_arguments,
    };
    use crate::godot::process::recovery_runner::test_support::{
        engine_profile, installation,
    };
    use siralos_core::godot::{GodotCapabilityKey, GodotEdition};

    #[test]
    fn fixed_with_docs_tuple_only() {
        assert_eq!(GODOT_KNOWLEDGE_BASE_ARGUMENTS.len(), 1);
        assert_eq!(
            godot_knowledge_arguments(),
            ["--dump-extension-api-with-docs"]
        );
    }

    #[test]
    fn digest_is_deterministic_and_binds_inputs() {
        let parts = || GodotKnowledgeCommandDigestParts {
            executable_sha256: "a".repeat(64),
            argument_template: godot_knowledge_arguments(),
            profile_id: "godot-knowledge-probe-offline".to_owned(),
            timeout_ms: 60_000,
            stdout_limit_bytes: 1024 * 1024,
            stderr_limit_bytes: 1024 * 1024,
        };
        let base = compute_godot_knowledge_command_digest(&parts());
        assert_eq!(base, compute_godot_knowledge_command_digest(&parts()));
        let changed_profile = GodotKnowledgeCommandDigestParts {
            profile_id: "other".to_owned(),
            ..parts()
        };
        assert_ne!(
            compute_godot_knowledge_command_digest(&changed_profile),
            base
        );
    }

    #[test]
    fn reports_unavailable_without_launching() {
        let runner = create_godot_knowledge_runner();
        assert!(!runner.is_available());
        let valid_installation = installation(true);
        let mut profile = engine_profile(GodotEdition::Standard);
        GodotCapabilityKey::ExtensionApiWithDocsDump
            .apply(&mut profile.capabilities, true);
        let outcome = runner
            .generate_documentation(super::GodotKnowledgeRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotKnowledgeRunOutcome::Unavailable {
                message: GODOT_KNOWLEDGE_GENERATION_UNAVAILABLE_MESSAGE
                    .to_owned()
            }
        );
    }

    #[test]
    fn rejects_engines_without_the_capability_as_unsupported() {
        let runner = create_godot_knowledge_runner();
        let valid_installation = installation(true);
        let profile = engine_profile(GodotEdition::Standard);
        let outcome = runner
            .generate_documentation(super::GodotKnowledgeRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert_eq!(
            outcome,
            GodotKnowledgeRunOutcome::Unsupported {
                message: "The selected Godot version does not advertise --dump-extension-api-with-docs; exact-engine API documentation is unsupported and an ordinary --dump-extension-api result is never substituted.".to_owned()
            }
        );
    }

    #[test]
    fn runtime_only_engines_are_unsupported() {
        let runner = create_godot_knowledge_runner();
        let valid_installation = installation(true);
        let profile = engine_profile(GodotEdition::RuntimeOnly);
        let outcome = runner
            .generate_documentation(super::GodotKnowledgeRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: false,
            })
            .expect("not cancelled");
        assert!(matches!(
            outcome,
            GodotKnowledgeRunOutcome::Unsupported { .. }
        ));
    }

    #[test]
    fn refuses_cancelled_runs_before_preconditions() {
        let runner = create_godot_knowledge_runner();
        let valid_installation = installation(true);
        let profile = engine_profile(GodotEdition::Standard);
        let error = runner
            .generate_documentation(super::GodotKnowledgeRunRequest {
                installation: &valid_installation,
                engine_profile: &profile,
                cancelled: true,
            })
            .unwrap_err();
        assert_eq!(
            error.message,
            "The Godot API documentation generation was aborted."
        );
    }
}
