//! Deterministic installation-selection policy (R8 Godot Stage-2 parity).
//!
//! Mirrors `packages/core/src/godot/selection.ts`.
//! Rank ordering: explicit CLI path > explicit CLI installation id >
//! explicit env path > explicit env installation id > configured active >
//! verified-baseline > compatible stable standard > compatible stable dotnet >
//! prerelease editor > no selection. Invalid, runtime-only, and Godot 3.x
//! are never selected.

use super::engine_profile::{
    GodotEdition, GodotEngineProfile, SiralosGodotSupport,
};
// GodotVersionStatus is used indirectly via rank_candidate; no direct import needed
use super::installations::GodotInstallation;

/// Selection preference supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotSelectionPreference {
    /// Explicit path supplied on the CLI.
    Path(String),
    /// Explicit installation id supplied on the CLI.
    InstallationId(String),
    /// Configured active installation.
    ConfigActive,
    /// Automatic selection.
    Auto,
    /// No selection.
    None,
}

/// One ranked candidate (installation + profile + rank).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRankedCandidate {
    /// The installation.
    pub installation: GodotInstallation,
    /// The engine profile for that installation.
    pub profile: GodotEngineProfile,
    /// Lower rank wins; `None` means not selectable.
    pub rank: Option<u64>,
}

/// Outcome of selection: the selected candidate and bounded rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSelectionOutcome {
    /// Selected candidate, if any.
    pub selected: Option<GodotRankedCandidate>,
    /// Bounded rationale.
    pub rationale: Vec<String>,
}

/// Rank constants matching the TypeScript oracle's `GODOT_SELECTION_RANKS`.
pub mod godot_selection_ranks {
    /// Explicit CLI path.
    pub const EXPLICIT_PATH: u64 = 1;
    /// Explicit CLI installation id.
    pub const EXPLICIT_INSTALLATION_ID: u64 = 2;
    /// Environment path.
    pub const ENVIRONMENT_PATH: u64 = 3;
    /// Environment installation id.
    pub const ENVIRONMENT_INSTALLATION_ID: u64 = 4;
    /// Configured active.
    pub const CONFIG_ACTIVE: u64 = 5;
    /// Verified baseline (4.7.1 stable standard editor).
    pub const VERIFIED_BASELINE: u64 = 6;
    /// Compatible stable standard editor.
    pub const COMPATIBLE_STABLE_STANDARD: u64 = 7;
    /// Compatible stable dotnet.
    pub const COMPATIBLE_STABLE_DOTNET: u64 = 8;
    /// Prerelease editor.
    pub const PRERELEASE_EDITOR: u64 = 9;
    /// No selection.
    pub const NONE: u64 = 10;
}

/// Rank one candidate's support into the selection rank (lower wins, `None` means not selectable).
#[must_use]
pub fn rank_candidate(profile: &GodotEngineProfile) -> Option<u64> {
    if profile.support == SiralosGodotSupport::RuntimeOnly
        || profile.support == SiralosGodotSupport::Invalid
    {
        return None;
    }
    if profile.support == SiralosGodotSupport::UnsupportedMajor {
        return None;
    }
    if profile.support == SiralosGodotSupport::Verified {
        return Some(godot_selection_ranks::VERIFIED_BASELINE);
    }
    if profile.support == SiralosGodotSupport::CompatibleUntested {
        if profile.release_channel
            == crate::godot::version::GodotReleaseChannel::Stable
        {
            return Some(if profile.edition == GodotEdition::Dotnet {
                godot_selection_ranks::COMPATIBLE_STABLE_DOTNET
            } else {
                godot_selection_ranks::COMPATIBLE_STABLE_STANDARD
            });
        }
        return Some(godot_selection_ranks::PRERELEASE_EDITOR);
    }
    Some(godot_selection_ranks::PRERELEASE_EDITOR)
}

/// Rank candidates deterministically using their profiles.
#[must_use]
pub fn rank_godot_candidates(
    candidates: Vec<(GodotInstallation, GodotEngineProfile)>,
) -> Vec<GodotRankedCandidate> {
    let mut ranked: Vec<GodotRankedCandidate> = candidates
        .into_iter()
        .map(|(installation, profile)| {
            let rank = rank_candidate(&profile);
            GodotRankedCandidate { installation, profile, rank }
        })
        .collect();
    ranked.sort_by(|left, right| {
        let left_rank = left.rank.unwrap_or(godot_selection_ranks::NONE);
        let right_rank = right.rank.unwrap_or(godot_selection_ranks::NONE);
        if left_rank != right_rank {
            return left_rank.cmp(&right_rank);
        }
        let left_stable = if left.profile.release_channel
            == super::version::GodotReleaseChannel::Stable
        {
            0
        } else {
            1
        };
        let right_stable = if right.profile.release_channel
            == super::version::GodotReleaseChannel::Stable
        {
            0
        } else {
            1
        };
        if left_stable != right_stable {
            return left_stable.cmp(&right_stable);
        }
        let left_patch = left.profile.version.patch.unwrap_or(0);
        let right_patch = right.profile.version.patch.unwrap_or(0);
        if left_patch != right_patch {
            return right_patch.cmp(&left_patch);
        }
        let left_edition = if left.profile.edition
            == super::engine_profile::GodotEdition::Standard
        {
            0
        } else {
            1
        };
        let right_edition = if right.profile.edition
            == super::engine_profile::GodotEdition::Standard
        {
            0
        } else {
            1
        };
        if left_edition != right_edition {
            return left_edition.cmp(&right_edition);
        }
        left.installation
            .canonical_path
            .cmp(&right.installation.canonical_path)
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::{godot_selection_ranks, rank_godot_candidates};
    use crate::godot::capabilities::empty_godot_command_capabilities;
    use crate::godot::engine_profile::{
        GodotEdition, GodotEditionConfidence, GodotEngineProfile,
        SiralosGodotSupport,
    };
    use crate::godot::installations::{
        GodotEditionHint, GodotInstallation, GodotInstallationSource,
    };
    use crate::godot::version::{
        GodotReleaseChannel, GodotVersion, GodotVersionStatus,
    };

    fn profile_for(
        support: SiralosGodotSupport,
        patch: Option<u64>,
    ) -> GodotEngineProfile {
        GodotEngineProfile {
            installation_id: "test".to_owned(),
            fingerprint: "abc12345".to_owned(),
            version: GodotVersion {
                raw: "4.7.1".to_owned(),
                major: 4,
                minor: 7,
                patch,
                status: GodotVersionStatus::Stable,
                status_number: None,
                build: Some("official".to_owned()),
                commit: None,
            },
            edition: GodotEdition::Standard,
            edition_confidence: GodotEditionConfidence::High,
            release_channel: GodotReleaseChannel::Stable,
            capabilities: empty_godot_command_capabilities(),
            verified_capabilities: vec![],
            degraded_capabilities: vec![],
            executable_sha256: "a".repeat(64),
            api_dump_sha256: None,
            support,
            diagnostics: vec![],
        }
    }

    fn installation(path: &str) -> GodotInstallation {
        GodotInstallation {
            id: format!("id-{path}"),
            source_label: "test".to_owned(),
            source: GodotInstallationSource::Path,
            canonical_path: path.to_owned(),
            size_bytes: 1024,
            modified_at_ms: 0,
            sha256: "a".repeat(64),
            edition_hint: GodotEditionHint::Unknown,
            status_valid: true,
            error: None,
        }
    }

    #[test]
    fn runtime_only_is_not_selectable() {
        let ranked = rank_godot_candidates(vec![(
            installation("/a"),
            profile_for(SiralosGodotSupport::RuntimeOnly, Some(1)),
        )]);
        assert_eq!(ranked[0].rank, None);
    }

    #[test]
    fn verified_ranks_before_compatible() {
        let ranked = rank_godot_candidates(vec![
            (
                installation("/b"),
                profile_for(SiralosGodotSupport::CompatibleUntested, Some(1)),
            ),
            (
                installation("/a"),
                profile_for(SiralosGodotSupport::Verified, Some(1)),
            ),
        ]);
        assert_eq!(
            ranked[0].rank,
            Some(godot_selection_ranks::VERIFIED_BASELINE)
        );
        assert_eq!(ranked[0].installation.canonical_path, "/a");
    }
}
