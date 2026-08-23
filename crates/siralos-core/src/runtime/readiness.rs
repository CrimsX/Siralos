//! Runtime readiness manifests and fail-closed evaluation (Stage 3 —
//! Runtime Readiness & Operational Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/{modes,readiness}.ts` for the
//! readiness vocabulary. Readiness is deterministic from explicit
//! capability inputs and FAILS CLOSED: if the requested runtime mode
//! requires an isolation/security property that cannot be provided,
//! readiness is blocked and no execution request can proceed.
//! Evaluation never executes anything and never duplicates capability
//! doctor semantics — it reports the runtime-specific projection.

use super::RuntimeError;
use crate::determinism::stable_sort_by_key;
use crate::identity::{CanonicalValue, compute_artifact_digest};

fn object(entries: Vec<(&str, CanonicalValue)>) -> CanonicalValue {
    CanonicalValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string_value(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

/// Explicit runtime mode; Godot availability never implies visual mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// No display required.
    Headless,
    /// Requires a display and visual-capable sandbox support.
    Visual,
}

impl RuntimeMode {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Visual => "visual",
        }
    }

    /// Parse a protocol string; unknown modes are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "headless" => Some(Self::Headless),
            "visual" => Some(Self::Visual),
            _ => None,
        }
    }
}

/// The runtime-mode vocabulary in oracle order.
pub const RUNTIME_MODES: [&str; 2] = ["headless", "visual"];

/// Capability state of one readiness item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCapabilityState {
    /// Structurally supported but not yet provided.
    Supported,
    /// Provided and usable.
    Available,
    /// Provided through configuration.
    Configured,
    /// Usable with reduced guarantees.
    Degraded,
    /// Missing; blocks the run request when required.
    Blocked,
    /// Not implemented by this backend.
    Unsupported,
}

impl RuntimeCapabilityState {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Available => "available",
            Self::Configured => "configured",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unsupported => "unsupported",
        }
    }

    /// Parse a protocol string; unknown states are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "supported" => Some(Self::Supported),
            "available" => Some(Self::Available),
            "configured" => Some(Self::Configured),
            "degraded" => Some(Self::Degraded),
            "blocked" => Some(Self::Blocked),
            "unsupported" => Some(Self::Unsupported),
            _ => None,
        }
    }
}

/// One readiness checklist item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadinessItemId {
    /// Engine executable present.
    GodotExecutable,
    /// Engine fingerprint bound.
    GodotFingerprint,
    /// Project identity resolved.
    ProjectIdentity,
    /// Sandbox backend available.
    SandboxBackend,
    /// Process supervision supported.
    ProcessSupervision,
    /// Filesystem isolation available.
    FilesystemIsolation,
    /// User-data redirect available.
    UserDataIsolation,
    /// Network policy resolvable.
    NetworkPolicy,
    /// Artifact storage available.
    ArtifactStorage,
    /// Headless mode supported.
    HeadlessMode,
    /// Visual mode supported.
    VisualMode,
    /// Display present/unknown.
    Display,
    /// Resource-limit enforcement state.
    ResourceLimits,
}

impl ReadinessItemId {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GodotExecutable => "godot_executable",
            Self::GodotFingerprint => "godot_fingerprint",
            Self::ProjectIdentity => "project_identity",
            Self::SandboxBackend => "sandbox_backend",
            Self::ProcessSupervision => "process_supervision",
            Self::FilesystemIsolation => "filesystem_isolation",
            Self::UserDataIsolation => "user_data_isolation",
            Self::NetworkPolicy => "network_policy",
            Self::ArtifactStorage => "artifact_storage",
            Self::HeadlessMode => "headless_mode",
            Self::VisualMode => "visual_mode",
            Self::Display => "display",
            Self::ResourceLimits => "resource_limits",
        }
    }

    /// Parse a protocol string; unknown ids are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "godot_executable" => Some(Self::GodotExecutable),
            "godot_fingerprint" => Some(Self::GodotFingerprint),
            "project_identity" => Some(Self::ProjectIdentity),
            "sandbox_backend" => Some(Self::SandboxBackend),
            "process_supervision" => Some(Self::ProcessSupervision),
            "filesystem_isolation" => Some(Self::FilesystemIsolation),
            "user_data_isolation" => Some(Self::UserDataIsolation),
            "network_policy" => Some(Self::NetworkPolicy),
            "artifact_storage" => Some(Self::ArtifactStorage),
            "headless_mode" => Some(Self::HeadlessMode),
            "visual_mode" => Some(Self::VisualMode),
            "display" => Some(Self::Display),
            "resource_limits" => Some(Self::ResourceLimits),
            _ => None,
        }
    }
}

/// One readiness checklist entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessItem {
    /// Item id.
    pub id: ReadinessItemId,
    /// Capability state.
    pub state: RuntimeCapabilityState,
    /// Bounded truthful detail.
    pub detail: String,
}

/// Required item ids per runtime mode, in oracle order.
fn required_items(mode: RuntimeMode) -> &'static [ReadinessItemId] {
    match mode {
        RuntimeMode::Headless => &[
            ReadinessItemId::GodotExecutable,
            ReadinessItemId::GodotFingerprint,
            ReadinessItemId::ProjectIdentity,
            ReadinessItemId::SandboxBackend,
            ReadinessItemId::ProcessSupervision,
            ReadinessItemId::FilesystemIsolation,
            ReadinessItemId::UserDataIsolation,
            ReadinessItemId::NetworkPolicy,
            ReadinessItemId::ArtifactStorage,
            ReadinessItemId::HeadlessMode,
        ],
        RuntimeMode::Visual => &[
            ReadinessItemId::GodotExecutable,
            ReadinessItemId::GodotFingerprint,
            ReadinessItemId::ProjectIdentity,
            ReadinessItemId::SandboxBackend,
            ReadinessItemId::ProcessSupervision,
            ReadinessItemId::FilesystemIsolation,
            ReadinessItemId::UserDataIsolation,
            ReadinessItemId::NetworkPolicy,
            ReadinessItemId::ArtifactStorage,
            ReadinessItemId::VisualMode,
            ReadinessItemId::Display,
        ],
    }
}

/// Declared capability inputs for one evaluation.
#[derive(Debug, Clone, Default)]
pub struct RuntimeReadinessInput {
    /// Requested runtime mode.
    pub runtime_mode: Option<RuntimeMode>,
    /// Engine executable presence + fingerprint.
    pub godot_executable_available: bool,
    /// Engine fingerprint when bound.
    pub godot_executable_fingerprint: Option<String>,
    /// Project/workspace identity when resolved.
    pub project_identity: Option<String>,
    /// Sandbox backend presence.
    pub sandbox_backend_available: bool,
    /// Process supervision support.
    pub sandbox_supports_process_supervision: bool,
    /// Filesystem isolation availability.
    pub filesystem_isolation_available: bool,
    /// User-data redirect availability.
    pub user_data_redirect_available: bool,
    /// Network policy resolvability.
    pub network_policy_resolvable: bool,
    /// Artifact storage availability.
    pub artifact_storage_available: bool,
    /// Display availability (`None` = unknown).
    pub display_available: Option<bool>,
    /// Which resource limits the backend can enforce/observe.
    pub memory_limit_enforced: bool,
    /// Which resource limits the backend can enforce/observe (CPU).
    pub cpu_limit_enforced: bool,
}

impl RuntimeReadinessInput {
    fn fingerprint_detail(fingerprint: &str) -> String {
        format!(
            "fingerprint {}\u{2026}",
            fingerprint.chars().take(12).collect::<String>()
        )
    }
}

/// Deterministic fail-closed readiness manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeReadinessManifest {
    /// Evaluated mode.
    pub runtime_mode: RuntimeMode,
    /// Items in canonical id order.
    pub items: Vec<ReadinessItem>,
    /// True only when every required item is available/supported.
    pub ready: bool,
    /// Exactly why readiness is blocked (fail-closed evidence).
    pub blocked_reasons: Vec<String>,
    /// Digest over the canonical `RuntimeReadinessManifest v1` payload.
    pub digest: String,
}

/// Evaluate readiness for the declared capabilities. Equivalent inputs
/// produce the same manifest; any missing required isolation property
/// blocks the whole run request.
///
/// # Errors
///
/// Returns an error only if the digest primitive rejects its own
/// artifact type, which cannot happen for these constants.
pub fn evaluate_runtime_readiness(
    input: &RuntimeReadinessInput,
) -> Result<RuntimeReadinessManifest, RuntimeError> {
    let mode = input.runtime_mode.unwrap_or(RuntimeMode::Headless);
    let mut items = vec![
        ReadinessItem {
            id: ReadinessItemId::GodotExecutable,
            state: if input.godot_executable_available {
                RuntimeCapabilityState::Available
            } else {
                RuntimeCapabilityState::Blocked
            },
            // Detail strings mirror the oracle verbatim (see the
            // architecture-exemption note for this module family).
            detail: if input.godot_executable_available {
                "Godot executable present".to_owned()
            } else {
                "Godot executable unavailable".to_owned()
            },
        },
        ReadinessItem {
            id: ReadinessItemId::GodotFingerprint,
            state: match &input.godot_executable_fingerprint {
                None => RuntimeCapabilityState::Unsupported,
                Some(_) => RuntimeCapabilityState::Available,
            },
            detail: match &input.godot_executable_fingerprint {
                None => "no current Godot fingerprint".to_owned(),
                Some(fingerprint) => {
                    RuntimeReadinessInput::fingerprint_detail(fingerprint)
                }
            },
        },
    ];
    items.push(ReadinessItem {
        id: ReadinessItemId::ProjectIdentity,
        state: if input.project_identity.is_none() {
            RuntimeCapabilityState::Blocked
        } else {
            RuntimeCapabilityState::Available
        },
        detail: if input.project_identity.is_none() {
            "no project identity".to_owned()
        } else {
            "project identity resolved".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::SandboxBackend,
        state: if input.sandbox_backend_available {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Blocked
        },
        detail: if input.sandbox_backend_available {
            "sandbox backend available".to_owned()
        } else {
            "sandbox backend unavailable".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::ProcessSupervision,
        state: if input.sandbox_supports_process_supervision {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Unsupported
        },
        detail: if input.sandbox_supports_process_supervision {
            "process supervision supported".to_owned()
        } else {
            "process supervision unsupported".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::FilesystemIsolation,
        state: if input.filesystem_isolation_available {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Blocked
        },
        detail: if input.filesystem_isolation_available {
            "filesystem isolation available".to_owned()
        } else {
            "filesystem isolation unavailable".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::UserDataIsolation,
        state: if input.user_data_redirect_available {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Blocked
        },
        detail: if input.user_data_redirect_available {
            "user-data redirect available".to_owned()
        } else {
            "user-data redirect unavailable".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::NetworkPolicy,
        state: if input.network_policy_resolvable {
            RuntimeCapabilityState::Configured
        } else {
            RuntimeCapabilityState::Blocked
        },
        detail: if input.network_policy_resolvable {
            "network policy resolvable".to_owned()
        } else {
            "network policy unresolvable".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::ArtifactStorage,
        state: if input.artifact_storage_available {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Blocked
        },
        detail: if input.artifact_storage_available {
            "artifact storage available".to_owned()
        } else {
            "artifact storage unavailable".to_owned()
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::HeadlessMode,
        state: RuntimeCapabilityState::Available,
        detail: "headless runtime mode supported by the readiness contract"
            .to_owned(),
    });
    let display_state = match input.display_available {
        Some(false) => RuntimeCapabilityState::Blocked,
        None => RuntimeCapabilityState::Degraded,
        Some(true) => RuntimeCapabilityState::Available,
    };
    items.push(ReadinessItem {
        id: ReadinessItemId::VisualMode,
        state: display_state,
        detail: match input.display_available {
            Some(false) => "visual mode blocked: no display".to_owned(),
            None => {
                "visual mode degraded: display availability unknown".to_owned()
            }
            Some(true) => "visual mode available: display present".to_owned(),
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::Display,
        state: display_state,
        detail: match input.display_available {
            Some(false) => "no display".to_owned(),
            None => "display unknown".to_owned(),
            Some(true) => "display available".to_owned(),
        },
    });
    items.push(ReadinessItem {
        id: ReadinessItemId::ResourceLimits,
        state: if input.memory_limit_enforced || input.cpu_limit_enforced {
            RuntimeCapabilityState::Available
        } else {
            RuntimeCapabilityState::Degraded
        },
        detail: format!(
            "memory {}; cpu {}",
            if input.memory_limit_enforced {
                "enforced"
            } else {
                "not enforced"
            },
            if input.cpu_limit_enforced { "enforced" } else { "not enforced" }
        ),
    });
    let mut blocked_reasons = Vec::new();
    for required_id in required_items(mode) {
        if let Some(entry) =
            items.iter().find(|candidate| candidate.id == *required_id)
        {
            if matches!(
                entry.state,
                RuntimeCapabilityState::Blocked
                    | RuntimeCapabilityState::Unsupported
            ) {
                blocked_reasons.push(format!(
                    "{}: {}",
                    required_id.as_str(),
                    entry.detail
                ));
            }
        }
    }
    let ready = blocked_reasons.is_empty();
    let ordered =
        stable_sort_by_key(&items, |entry| entry.id.as_str().to_owned());
    let payload = object(vec![
        ("runtimeMode", string_value(mode.as_str())),
        (
            "items",
            CanonicalValue::Array(
                ordered
                    .iter()
                    .map(|entry| {
                        object(vec![
                            ("id", string_value(entry.id.as_str())),
                            ("state", string_value(entry.state.as_str())),
                            ("detail", string_value(&entry.detail)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let digest =
        compute_artifact_digest("RuntimeReadinessManifest", 1, &payload)
            .map_err(|error| RuntimeError { message: error.message })?;
    Ok(RuntimeReadinessManifest {
        runtime_mode: mode,
        items: ordered,
        ready,
        blocked_reasons,
        digest: digest.value,
    })
}

/// Fail-closed gate: readiness must be ready before any execution
/// request.
#[must_use]
pub fn execution_allowed(manifest: &RuntimeReadinessManifest) -> bool {
    manifest.ready
}

/// Bounded human-readable readiness projection (em-dash separated).
#[must_use]
pub fn render_runtime_readiness(
    manifest: &RuntimeReadinessManifest,
) -> String {
    let mut lines = vec![format!(
        "Runtime readiness ({}): {}",
        manifest.runtime_mode.as_str(),
        if manifest.ready { "ready" } else { "BLOCKED" }
    )];
    for entry in &manifest.items {
        lines.push(format!(
            "  {}: {} \u{2014} {}",
            entry.id.as_str(),
            entry.state.as_str(),
            entry.detail
        ));
    }
    if !manifest.ready {
        lines.push("Blocked because:".to_owned());
        for reason in &manifest.blocked_reasons {
            lines.push(format!("  - {reason}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        RuntimeCapabilityState, RuntimeMode, RuntimeReadinessInput,
        evaluate_runtime_readiness, execution_allowed,
        render_runtime_readiness,
    };
    use crate::runtime::doctor::{
        DoctorCapabilities, build_runtime_readiness_diagnostic,
    };

    fn fully_capable() -> RuntimeReadinessInput {
        RuntimeReadinessInput {
            runtime_mode: Some(RuntimeMode::Headless),
            godot_executable_available: true,
            godot_executable_fingerprint: Some("abcdef1234567890".to_owned()),
            project_identity: Some("proj-1".to_owned()),
            sandbox_backend_available: true,
            sandbox_supports_process_supervision: true,
            filesystem_isolation_available: true,
            user_data_redirect_available: true,
            network_policy_resolvable: true,
            artifact_storage_available: true,
            display_available: Some(true),
            memory_limit_enforced: false,
            cpu_limit_enforced: false,
        }
    }

    #[test]
    fn capable_headless_input_is_ready_with_stable_digest() {
        let manifest =
            evaluate_runtime_readiness(&fully_capable()).expect("manifest");
        assert!(manifest.ready);
        assert!(execution_allowed(&manifest));
        assert_eq!(manifest.blocked_reasons.len(), 0);
        assert_eq!(manifest.items.len(), 13);
        assert_eq!(manifest.digest.len(), 64);
        let again =
            evaluate_runtime_readiness(&fully_capable()).expect("manifest");
        assert_eq!(again.digest, manifest.digest);
        // Items are canonically ordered by id string.
        for window in manifest.items.windows(2) {
            assert!(window[0].id.as_str() < window[1].id.as_str());
        }
    }

    #[test]
    fn blocked_inputs_fail_closed_in_required_order() {
        let mut input = fully_capable();
        input.godot_executable_available = false;
        input.filesystem_isolation_available = false;
        input.sandbox_supports_process_supervision = false;
        let manifest = evaluate_runtime_readiness(&input).expect("manifest");
        assert!(!manifest.ready);
        assert!(!execution_allowed(&manifest));
        // Blocked reasons follow the REQUIRED list order, not item order.
        assert_eq!(
            manifest.blocked_reasons,
            vec![
                "godot_executable: Godot executable unavailable".to_owned(),
                "process_supervision: process supervision unsupported"
                    .to_owned(),
                "filesystem_isolation: filesystem isolation unavailable"
                    .to_owned(),
            ]
        );
        assert!(
            render_runtime_readiness(&manifest)
                .starts_with("Runtime readiness (headless): BLOCKED")
        );
        assert!(render_runtime_readiness(&manifest).contains(
            "  godot_executable: blocked \u{2014} Godot executable unavailable"
        ));
    }

    #[test]
    fn visual_mode_degrades_without_a_display_but_stays_ready() {
        let mut input = fully_capable();
        input.runtime_mode = Some(RuntimeMode::Visual);
        input.display_available = None;
        let manifest = evaluate_runtime_readiness(&input).expect("manifest");
        assert!(manifest.ready, "degraded is not blocking");
        let display = manifest
            .items
            .iter()
            .find(|item| item.id.as_str() == "display")
            .expect("display item");
        assert_eq!(display.state, RuntimeCapabilityState::Degraded);
        // A missing display is blocking for visual mode.
        input.display_available = Some(false);
        let blocked = evaluate_runtime_readiness(&input).expect("manifest");
        assert!(!blocked.ready);
        assert!(
            blocked
                .blocked_reasons
                .contains(&"display: no display".to_owned())
        );
    }

    #[test]
    fn doctor_projects_both_modes_from_one_capability_set() {
        let diagnostic =
            build_runtime_readiness_diagnostic(&DoctorCapabilities {
                godot_executable_available: true,
                godot_executable_fingerprint: Some(
                    "abcdef1234567890".to_owned(),
                ),
                project_identity: Some("proj-1".to_owned()),
                sandbox_available: true,
                process_supervision_supported: true,
                filesystem_isolation_available: true,
                user_data_redirect_available: true,
                network_policy_resolvable: true,
                artifact_storage_available: true,
                display_available: Some(true),
            })
            .expect("diagnostic");
        assert!(diagnostic.headless.ready);
        assert!(diagnostic.visual.ready);
        assert_ne!(diagnostic.headless.digest, diagnostic.visual.digest);
        let offline = DoctorCapabilities {
            display_available: Some(false),
            ..DoctorCapabilities::default()
        };
        let blocked =
            build_runtime_readiness_diagnostic(&offline).expect("diagnostic");
        assert!(!blocked.headless.ready);
        assert!(!blocked.visual.ready);
    }
}
