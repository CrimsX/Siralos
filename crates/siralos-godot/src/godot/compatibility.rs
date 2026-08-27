//! Conservative static compatibility between engine and project (R8).
//!
//! Mirrors `packages/core/src/godot/compatibility.ts`.

use super::engine_profile::{GodotEngineProfile, SiralosGodotSupport};
use super::project::{GodotLanguageProfile, GodotProjectProfile};

/// Compatibility status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotCompatibilityStatus {
    /// Compatible.
    Compatible,
    /// Likely compatible.
    LikelyCompatible,
    /// Engine older than project.
    EngineOlderThanProject,
    /// Major version mismatch.
    MajorVersionMismatch,
    /// Edition mismatch.
    EditionMismatch,
    /// Project version unknown.
    ProjectVersionUnknown,
    /// Engine unverified.
    EngineUnverified,
    /// No engine selected.
    NoEngine,
    /// No project detected.
    NoProject,
}

impl GodotCompatibilityStatus {
    /// Canonical kebab string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::LikelyCompatible => "likely-compatible",
            Self::EngineOlderThanProject => "engine-older-than-project",
            Self::MajorVersionMismatch => "major-version-mismatch",
            Self::EditionMismatch => "edition-mismatch",
            Self::ProjectVersionUnknown => "project-version-unknown",
            Self::EngineUnverified => "engine-unverified",
            Self::NoEngine => "no-engine",
            Self::NoProject => "no-project",
        }
    }
}

/// Severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompatibilitySeverity {
    /// Info.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
}

impl CompatibilitySeverity {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Static compatibility assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCompatibilityAssessment {
    /// Status.
    pub status: GodotCompatibilityStatus,
    /// Severity.
    pub severity: CompatibilitySeverity,
    /// Ordered reasons.
    pub reasons: Vec<String>,
}

/// Conservative static comparison. Declared project versions are non-authoritative.
#[must_use]
pub fn assess_godot_compatibility(
    engine: Option<&GodotEngineProfile>,
    project: &GodotProjectProfile,
) -> GodotCompatibilityAssessment {
    if !project.detected {
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::NoProject,
            severity: CompatibilitySeverity::Info,
            reasons: vec!["No project.godot exists at the workspace root; nothing to compare.".to_owned()],
        };
    }
    let Some(engine) = engine else {
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::NoEngine,
            severity: CompatibilitySeverity::Warning,
            reasons: vec!["The project was detected, but no trusted Godot installation is selected; compatibility cannot be assessed.".to_owned()],
        };
    };
    let mut reasons = Vec::new();
    if engine.support == SiralosGodotSupport::Verified {
        reasons.push(format!(
            "Siralos verified support: {} standard editor.",
            engine.version.raw
        ));
    } else {
        reasons.push(format!(
            "Siralos support: {:?} ({}).",
            engine.support, engine.version.raw
        ));
    }
    let Some(declared) = &project.declared_engine_version else {
        reasons.push("The project declares no engine feature; static compatibility is unknown.".to_owned());
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::ProjectVersionUnknown,
            severity: CompatibilitySeverity::Warning,
            reasons,
        };
    };
    if engine.version.major < declared.major {
        reasons.push(format!("The engine major ({}) is lower than the declared project major ({}).", engine.version.major, declared.major));
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::MajorVersionMismatch,
            severity: CompatibilitySeverity::Error,
            reasons,
        };
    }
    if engine.version.major > declared.major {
        reasons.push(format!("The engine major ({}) is newer than the declared project major ({}); migration-sensitive.", engine.version.major, declared.major));
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::LikelyCompatible,
            severity: CompatibilitySeverity::Warning,
            reasons,
        };
    }
    if engine.version.minor < declared.minor {
        reasons.push(format!("The engine minor ({}) is older than the declared project minor ({}).", engine.version.minor, declared.minor));
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::EngineOlderThanProject,
            severity: CompatibilitySeverity::Error,
            reasons,
        };
    }
    if engine.version.minor > declared.minor {
        reasons.push(format!("The engine minor ({}) is newer than the declared project minor ({}); migration-sensitive.", engine.version.minor, declared.minor));
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::LikelyCompatible,
            severity: CompatibilitySeverity::Warning,
            reasons,
        };
    }
    if matches!(
        engine.support,
        SiralosGodotSupport::UnsupportedMajor
            | SiralosGodotSupport::Invalid
            | SiralosGodotSupport::RuntimeOnly
    ) {
        reasons.push(
            "The selected engine is not a supported editor for Siralos."
                .to_owned(),
        );
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::EngineUnverified,
            severity: CompatibilitySeverity::Error,
            reasons,
        };
    }
    if engine.support != SiralosGodotSupport::Verified
        && engine.support != SiralosGodotSupport::CompatibleUntested
    {
        reasons.push(
            "The selected engine build is unverified for Siralos.".to_owned(),
        );
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::EngineUnverified,
            severity: CompatibilitySeverity::Warning,
            reasons,
        };
    }
    if project.language_profile == GodotLanguageProfile::Dotnet
        && engine.edition == super::engine_profile::GodotEdition::Standard
    {
        reasons.push("The project uses .NET, but the selected engine is the standard (non-.NET) editor.".to_owned());
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::EditionMismatch,
            severity: CompatibilitySeverity::Error,
            reasons,
        };
    }
    if project.language_profile == GodotLanguageProfile::Gdscript
        && engine.edition == super::engine_profile::GodotEdition::Dotnet
    {
        reasons.push("The project appears GDScript-only; a .NET engine is selected and remains unverified for Siralos.".to_owned());
        return GodotCompatibilityAssessment {
            status: GodotCompatibilityStatus::LikelyCompatible,
            severity: CompatibilitySeverity::Warning,
            reasons,
        };
    }
    reasons.push(format!("The engine ({}) matches the declared project version ({}) within the same minor line.", engine.version.raw, declared.raw));
    GodotCompatibilityAssessment {
        status: GodotCompatibilityStatus::Compatible,
        severity: CompatibilitySeverity::Info,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::super::project::create_empty_godot_project_profile;
    use super::assess_godot_compatibility;

    #[test]
    fn no_project_is_info() {
        let p = create_empty_godot_project_profile();
        let a = assess_godot_compatibility(None, &p);
        assert_eq!(a.status, super::GodotCompatibilityStatus::NoProject);
    }
}
