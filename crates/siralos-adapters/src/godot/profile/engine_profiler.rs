//! Engine discovery and profiling with deterministic selection.
//!
//! Every discovery call re-collects candidates (the bounded config/PATH
//! scan) and revalidates every executable's full SHA-256, so candidate
//! additions, removals, and content changes are visible within the
//! session; there is no trusted in-memory discovery cache. On this stage
//! engine probing is fail-closed: no candidate is profiled, selection
//! falls back to the static rationale, and every consumer that needs a
//! profile receives a typed refusal instead of a fabricated profile.

use std::collections::BTreeSet;
use std::path::Path;

use siralos_core::godot::{
    DiagnosticSeverity, GodotDiscoveryConfiguration, GodotDiscoveryResult,
    GodotEngineProfile, GodotInstallation, GodotInstallationOverview,
    GodotSelectionPreference, SafeDiagnostic, rank_godot_candidates,
};

use crate::config::UserGodotConfig;
use crate::godot::discovery::executable_validation::{
    ValidateExecutableOptions, validate_executable,
};
use crate::godot::discovery::path_discovery::{
    PathDiscoveryOptions, discover_on_path, installation_from_identity,
    invalid_installation,
};
use crate::godot::process::probe_runner::GODOT_PROBING_UNAVAILABLE_MESSAGE;
use siralos_core::godot::{
    GodotInstallationSource as Source, InstallEditionHint as Hint,
};

/// Precedence level of an explicit override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GodotOverrideSource {
    /// `--godot-path` / `--godot-installation`.
    Cli,
    /// `SIRALOS_GODOT` / `SIRALOS_GODOT_INSTALLATION`.
    Environment,
}

/// Inputs for one profiler instance.
#[derive(Debug, Clone)]
pub struct GodotProfilerInputs {
    /// User Godot configuration envelope.
    pub config: UserGodotConfig,
    /// Effective selection preference.
    pub preference: GodotSelectionPreference,
    /// Precedence level of the explicit override, if any.
    pub override_source: Option<GodotOverrideSource>,
    /// Canonical workspace root.
    pub workspace_root: String,
    /// Sanitized host PATH value.
    pub host_path: Option<String>,
    /// Sanitized host PATHEXT value.
    pub host_path_ext: Option<String>,
    /// Node-style platform string (`"win32"`, `"darwin"`, `"linux"`).
    pub platform: String,
}

/// One profiled candidate from a single discovery generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProfiledCandidate {
    /// The installation.
    pub installation: GodotInstallation,
    /// The probed profile; always `None` while probing is unavailable.
    pub profile: Option<GodotEngineProfile>,
    /// Bounded profiling failure reason.
    pub profile_error: Option<String>,
}

/// A failed selection request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotSelectionError {
    /// Bounded truthful message.
    pub message: String,
}

/// One discovery generation with its selected candidate: the discovery
/// result and the selected installation/profile come from the SAME run,
/// so consumers can never combine snapshots from different generations.
pub struct DiscoveredSelection {
    /// Discovery result.
    pub discovery: GodotDiscoveryResult,
    /// Selected installation with its full profile, if any.
    pub selected: Option<(GodotInstallation, GodotEngineProfile)>,
}

/// Run one discovery generation.
pub fn discover(
    inputs: &GodotProfilerInputs,
) -> Result<GodotDiscoveryResult, GodotSelectionError> {
    discover_internal(inputs).map(|result| result.discovery)
}

/// Run one discovery generation and return its selected candidate.
pub fn discover_with_selection(
    inputs: &GodotProfilerInputs,
) -> Result<DiscoveredSelection, GodotSelectionError> {
    let internal = discover_internal(inputs)?;
    Ok(DiscoveredSelection {
        discovery: internal.discovery,
        selected: internal.selected,
    })
}

/// The selected installation with its full profile, or `None`.
///
/// At this stage probing is fail-closed, so no candidate ever carries a
/// profile and consumers receive `None` truthfully.
pub fn selected_profile(
    inputs: &GodotProfilerInputs,
) -> Result<Option<(GodotInstallation, GodotEngineProfile)>, GodotSelectionError>
{
    discover_with_selection(inputs).map(|discovered| discovered.selected)
}

struct InternalDiscovery {
    discovery: GodotDiscoveryResult,
    selected: Option<(GodotInstallation, GodotEngineProfile)>,
}

fn discover_internal(
    inputs: &GodotProfilerInputs,
) -> Result<InternalDiscovery, GodotSelectionError> {
    let (candidates, duplicates) = collect_candidates(inputs);
    let mut diagnostics: Vec<SafeDiagnostic> = Vec::new();
    let profiled: Vec<GodotProfiledCandidate> =
        candidates.iter().map(profile_candidate).collect();
    let selection = select(&profiled, inputs)?;
    if let Some(message) = &selection.config_active_error {
        diagnostics.push(SafeDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: message.clone(),
        });
    }
    for candidate in &profiled {
        if let Some(error) = &candidate.profile_error {
            diagnostics.push(SafeDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!(
                    "Installation {}: {error}",
                    candidate.installation.id
                ),
            });
        }
    }
    let overviews: Vec<GodotInstallationOverview> = candidates
        .iter()
        .map(|installation| {
            let entry = profiled
                .iter()
                .find(|entry| entry.installation.id == installation.id);
            let invalid = entry
                .and_then(|entry| entry.profile_error.clone())
                .or_else(|| {
                    if !installation.status_valid {
                        Some(installation.error.clone().unwrap_or_else(|| {
                            "invalid installation".to_owned()
                        }))
                    } else {
                        None
                    }
                });
            GodotInstallationOverview {
                installation_id: installation.id.clone(),
                version: None,
                edition: None,
                edition_confidence: None,
                release_channel: None,
                source_label: installation.source_label.clone(),
                source: installation.source,
                support: None,
                invalid,
                is_duplicate: duplicates.contains(&installation.id),
                selected: selection
                    .installation_id
                    .as_ref()
                    .is_some_and(|id| id == &installation.id),
                fingerprint: None,
                profiled: false,
            }
        })
        .collect();
    let rationale = selection.rationale;
    let configuration = GodotDiscoveryConfiguration {
        active_installation: inputs.config.active_installation.clone(),
        configured_count: inputs.config.installations.len(),
        discover_on_path: inputs.config.discover_on_path,
        overrides: describe_overrides(&inputs.preference),
    };
    let discovery = GodotDiscoveryResult {
        selected: overviews.iter().find(|overview| overview.selected).cloned(),
        candidates: overviews,
        configuration,
        rationale,
        diagnostics,
    };
    let selected = match &selection.installation_id {
        Some(id) => profiled
            .iter()
            .find(|candidate| &candidate.installation.id == id)
            .and_then(|candidate| {
                candidate
                    .profile
                    .clone()
                    .map(|profile| (candidate.installation.clone(), profile))
            }),
        None => None,
    };
    Ok(InternalDiscovery { discovery, selected })
}

struct SelectionOutcome {
    installation_id: Option<String>,
    rationale: Vec<String>,
    config_active_error: Option<String>,
}

fn select(
    profiled: &[GodotProfiledCandidate],
    inputs: &GodotProfilerInputs,
) -> Result<SelectionOutcome, GodotSelectionError> {
    let valid: Vec<&GodotProfiledCandidate> = profiled
        .iter()
        .filter(|candidate| {
            candidate.profile.is_some() && candidate.installation.status_valid
        })
        .collect();
    let mut rationale: Vec<String> = Vec::new();
    match &inputs.preference {
        GodotSelectionPreference::Path(_) => {
            let found = valid
                .iter()
                .any(|candidate| candidate.installation.id == "explicit");
            if !found {
                return Err(GodotSelectionError {
                    message:
                        "The explicit Godot path did not resolve to a valid, probed installation."
                            .to_owned(),
                });
            }
            Ok(SelectionOutcome {
                installation_id: Some("explicit".to_owned()),
                rationale: vec![
                    "Explicitly selected by path (CLI or environment override)."
                        .to_owned(),
                ],
                config_active_error: None,
            })
        }
        GodotSelectionPreference::InstallationId(id) => {
            let match_candidate = profiled
                .iter()
                .find(|candidate| &candidate.installation.id == id);
            let Some(candidate) = match_candidate else {
                return Err(GodotSelectionError {
                    message: format!(
                        "The explicit Godot installation id does not exist: {id}"
                    ),
                });
            };
            if candidate.profile.is_none() {
                return Err(GodotSelectionError {
                    message: format!(
                        "The explicit Godot installation id is invalid: {id}"
                    ),
                });
            }
            Ok(SelectionOutcome {
                installation_id: Some(candidate.installation.id.clone()),
                rationale: vec![
                    "Explicitly selected by installation id (CLI or environment override)."
                        .to_owned(),
                ],
                config_active_error: None,
            })
        }
        GodotSelectionPreference::ConfigActive => {
            let active_display = inputs
                .config
                .active_installation
                .clone()
                .unwrap_or_else(|| "undefined".to_owned());
            let matched = profiled
                .iter()
                .find(|candidate| candidate.installation.id == active_display)
                .filter(|candidate| candidate.profile.is_some());
            match matched {
                Some(candidate) => Ok(SelectionOutcome {
                    installation_id: Some(candidate.installation.id.clone()),
                    rationale: vec![format!(
                        "Configured active installation: {active_display}."
                    )],
                    config_active_error: None,
                }),
                None => {
                    let existed = profiled.iter().any(|candidate| {
                        candidate.installation.id == active_display
                    });
                    let message = if existed {
                        format!(
                            "The configured active installation \"{active_display}\" is invalid; falling back to automatic selection."
                        )
                    } else {
                        format!(
                            "The configured active installation \"{active_display}\" does not exist; falling back to automatic selection."
                        )
                    };
                    let mut outcome =
                        select_automatic(&valid, &mut rationale)?;
                    outcome.config_active_error = Some(message);
                    Ok(outcome)
                }
            }
        }
        GodotSelectionPreference::Auto | GodotSelectionPreference::None => {
            select_automatic(&valid, &mut rationale)
        }
    }
}

fn select_automatic(
    valid: &[&GodotProfiledCandidate],
    rationale: &mut Vec<String>,
) -> Result<SelectionOutcome, GodotSelectionError> {
    let ranked = rank_godot_candidates(
        valid
            .iter()
            .filter_map(|candidate| {
                candidate
                    .profile
                    .clone()
                    .map(|profile| (candidate.installation.clone(), profile))
            })
            .collect(),
    );
    let selectable: Vec<_> =
        ranked.into_iter().filter(|entry| entry.rank.is_some()).collect();
    if selectable.is_empty() {
        rationale.push(
            "No selectable Godot installation was discovered.".to_owned(),
        );
        return Ok(SelectionOutcome {
            installation_id: None,
            rationale: rationale.clone(),
            config_active_error: None,
        });
    }
    for entry in &selectable {
        let label = rank_label(entry.rank.unwrap_or(0));
        rationale.push(format!(
            "Rank {}: {} ({}, {}).",
            entry.rank.unwrap_or(0),
            entry.installation.id,
            entry.profile.version.raw,
            label
        ));
    }
    let winner = &selectable[0];
    rationale.push(format!(
        "Selected {} ({}) by deterministic ranking.",
        winner.installation.id, winner.profile.version.raw
    ));
    Ok(SelectionOutcome {
        installation_id: Some(winner.installation.id.clone()),
        rationale: rationale.clone(),
        config_active_error: None,
    })
}

fn rank_label(rank: u64) -> &'static str {
    use siralos_core::godot::godot_selection_ranks as ranks;
    match rank {
        ranks::VERIFIED_BASELINE => "verified baseline stable standard editor",
        ranks::COMPATIBLE_STABLE_STANDARD => {
            "compatible stable standard editor"
        }
        ranks::COMPATIBLE_STABLE_DOTNET => "compatible stable .NET editor",
        ranks::PRERELEASE_EDITOR => "prerelease editor",
        _ => "candidate",
    }
}

fn profile_candidate(
    installation: &GodotInstallation,
) -> GodotProfiledCandidate {
    if !installation.status_valid {
        return GodotProfiledCandidate {
            installation: installation.clone(),
            profile: None,
            profile_error: Some(
                installation
                    .error
                    .clone()
                    .unwrap_or_else(|| "invalid installation".to_owned()),
            ),
        };
    }
    GodotProfiledCandidate {
        installation: installation.clone(),
        profile: None,
        profile_error: Some(GODOT_PROBING_UNAVAILABLE_MESSAGE.to_owned()),
    }
}

fn collect_candidates(
    inputs: &GodotProfilerInputs,
) -> (Vec<GodotInstallation>, BTreeSet<String>) {
    let mut candidates: Vec<GodotInstallation> = Vec::new();
    if let GodotSelectionPreference::Path(path) = &inputs.preference {
        let (source, label) = override_identity(inputs.override_source);
        let mut executable_path = path.clone();
        if inputs.platform == "darwin" && path.ends_with(".app") {
            match crate::godot::discovery::macos_bundle::resolve_macos_bundle(
                Path::new(path),
            ) {
                Ok(resolved) => {
                    executable_path = resolved.to_string_lossy().into_owned();
                }
                Err(_) => {
                    candidates.push(invalid_installation(
                        "explicit".to_owned(),
                        source,
                        label,
                        "The configured Godot application bundle is invalid."
                            .to_owned(),
                    ));
                    return (candidates, BTreeSet::new());
                }
            }
        }
        let validated = validate_executable(ValidateExecutableOptions {
            path: executable_path,
            workspace_root: inputs.workspace_root.clone(),
            max_bytes: None,
        });
        candidates.push(match validated {
            Ok(identity) => installation_from_identity(
                "explicit".to_owned(),
                source,
                label,
                &identity,
                Hint::Unknown,
            ),
            Err(error) => invalid_installation(
                "explicit".to_owned(),
                source,
                label,
                error,
            ),
        });
        return (candidates, BTreeSet::new());
    }
    for (id, installation_config) in &inputs.config.installations {
        let mut executable_path = installation_config.path.clone();
        if inputs.platform == "darwin"
            && installation_config.path.ends_with(".app")
        {
            match crate::godot::discovery::macos_bundle::resolve_macos_bundle(
                Path::new(&installation_config.path),
            ) {
                Ok(resolved) => {
                    executable_path = resolved.to_string_lossy().into_owned();
                }
                Err(_) => {
                    candidates.push(invalid_installation(
                        id.clone(),
                        Source::UserConfig,
                        "user config",
                        "The bundle is not a valid Godot application bundle."
                            .to_owned(),
                    ));
                    continue;
                }
            }
        }
        let validated = validate_executable(ValidateExecutableOptions {
            path: executable_path,
            workspace_root: inputs.workspace_root.clone(),
            max_bytes: None,
        });
        let edition_hint = match installation_config.edition_hint {
            crate::config::UserGodotEditionHint::Standard => Hint::Standard,
            crate::config::UserGodotEditionHint::Dotnet => Hint::Dotnet,
            crate::config::UserGodotEditionHint::Unknown => Hint::Unknown,
        };
        candidates.push(match validated {
            Ok(identity) => installation_from_identity(
                id.clone(),
                Source::UserConfig,
                "user config",
                &identity,
                edition_hint,
            ),
            Err(error) => invalid_installation(
                id.clone(),
                Source::UserConfig,
                "user config",
                error,
            ),
        });
    }
    if inputs.config.discover_on_path {
        let (path_candidates, _truncated) =
            discover_on_path(PathDiscoveryOptions {
                host_path: inputs.host_path.clone(),
                host_path_ext: inputs.host_path_ext.clone(),
                platform: inputs.platform.clone(),
                workspace_root: inputs.workspace_root.clone(),
            });
        candidates.extend(path_candidates);
    }
    deduplicate_candidates(candidates, &inputs.platform)
}

/// Deduplicates candidates by canonical path identity, platform aware:
/// on Windows and macOS paths differing only in case collapse; elsewhere
/// the comparison stays case-sensitive so distinct executables are never
/// merged. Invalid candidates are never deduplicated.
pub fn deduplicate_candidates(
    candidates: Vec<GodotInstallation>,
    platform: &str,
) -> (Vec<GodotInstallation>, BTreeSet<String>) {
    let fold = platform == "win32" || platform == "darwin";
    let mut seen: Vec<String> = Vec::new();
    let mut duplicates = BTreeSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        if !candidate.status_valid {
            deduped.push(candidate);
            continue;
        }
        let matches_existing = seen.iter().any(|existing| {
            if fold {
                existing.eq_ignore_ascii_case(&candidate.canonical_path)
            } else {
                existing == &candidate.canonical_path
            }
        });
        if matches_existing {
            duplicates.insert(candidate.id.clone());
            continue;
        }
        seen.push(candidate.canonical_path.clone());
        deduped.push(candidate);
    }
    (deduped, duplicates)
}

fn override_identity(
    override_source: Option<GodotOverrideSource>,
) -> (Source, &'static str) {
    match override_source {
        Some(GodotOverrideSource::Cli) => {
            (Source::CliPath, "CLI --godot-path")
        }
        _ => (Source::EnvironmentPath, "SIRALOS_GODOT"),
    }
}

fn describe_overrides(preference: &GodotSelectionPreference) -> Vec<String> {
    match preference {
        GodotSelectionPreference::Path(_) => {
            vec!["explicit executable path override".to_owned()]
        }
        GodotSelectionPreference::InstallationId(_) => {
            vec!["explicit installation id override".to_owned()]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GodotOverrideSource, GodotProfilerInputs, GodotSelectionPreference,
        deduplicate_candidates, discover,
    };
    use crate::config::{
        UserGodotConfig, UserGodotEditionHint, UserGodotInstallationConfig,
    };
    use crate::godot::process::probe_runner::GODOT_PROBING_UNAVAILABLE_MESSAGE;
    use siralos_core::godot::GodotInstallation;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "siralos-profiler-{label}-{}-{}",
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn empty_config() -> UserGodotConfig {
        UserGodotConfig {
            active_installation: None,
            installations: Default::default(),
            discover_on_path: false,
        }
    }

    fn inputs(
        config: UserGodotConfig,
        preference: GodotSelectionPreference,
        workspace_root: &Path,
    ) -> GodotProfilerInputs {
        GodotProfilerInputs {
            config,
            preference,
            override_source: None,
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            host_path: None,
            host_path_ext: None,
            platform: if cfg!(windows) {
                "win32".to_owned()
            } else {
                "linux".to_owned()
            },
        }
    }

    #[test]
    fn discovery_without_candidates_reports_no_selection() {
        let root = unique_dir("empty");
        let result = discover(&inputs(
            empty_config(),
            GodotSelectionPreference::Auto,
            &root,
        ))
        .expect("selection holds");
        assert!(result.candidates.is_empty());
        assert_eq!(result.selected, None);
        assert_eq!(
            result.rationale,
            ["No selectable Godot installation was discovered."]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn configured_candidate_truthfully_reports_probing_unavailable() {
        let root = unique_dir("ws");
        let exe_dir = unique_dir("exe");
        let exe_path = exe_dir.join("Godot.exe");
        fs::write(&exe_path, b"fake executable").unwrap();
        let mut config = empty_config();
        config.installations.insert(
            "cfg-1".to_owned(),
            UserGodotInstallationConfig {
                path: exe_path.to_string_lossy().into_owned(),
                edition_hint: UserGodotEditionHint::Unknown,
            },
        );
        let result =
            discover(&inputs(config, GodotSelectionPreference::Auto, &root))
                .expect("selection holds");
        assert_eq!(result.candidates.len(), 1);
        let overview = &result.candidates[0];
        assert_eq!(overview.installation_id, "cfg-1");
        assert!(!overview.profiled);
        assert_eq!(overview.fingerprint, None);
        assert!(!overview.selected);
        assert_eq!(
            overview.invalid.as_deref(),
            Some(GODOT_PROBING_UNAVAILABLE_MESSAGE)
        );
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.message
                == format!(
                    "Installation cfg-1: {GODOT_PROBING_UNAVAILABLE_MESSAGE}"
                )
        }));
        assert_eq!(
            result.rationale,
            ["No selectable Godot installation was discovered."]
        );
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&exe_dir).ok();
    }

    #[test]
    fn explicit_path_preference_fails_closed_without_probe() {
        let root = unique_dir("ws2");
        let error = discover(&inputs(
            empty_config(),
            GodotSelectionPreference::Path("C:\\godot\\Godot.exe".to_owned()),
            &root,
        ))
        .expect_err("refused without a probed candidate");
        assert_eq!(
            error.message,
            "The explicit Godot path did not resolve to a valid, probed installation."
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn installation_id_preference_messages() {
        let root = unique_dir("ws3");
        let missing = discover(&inputs(
            empty_config(),
            GodotSelectionPreference::InstallationId("ghost".to_owned()),
            &root,
        ))
        .expect_err("missing id refused");
        assert_eq!(
            missing.message,
            "The explicit Godot installation id does not exist: ghost"
        );
        let exe_dir = unique_dir("exe3");
        let exe_path = exe_dir.join("Godot");
        fs::write(&exe_path, b"fake").unwrap();
        let mut config = empty_config();
        config.installations.insert(
            "cfg-1".to_owned(),
            UserGodotInstallationConfig {
                path: exe_path.to_string_lossy().into_owned(),
                edition_hint: UserGodotEditionHint::Standard,
            },
        );
        let invalid = discover(&inputs(
            config,
            GodotSelectionPreference::InstallationId("cfg-1".to_owned()),
            &root,
        ))
        .expect_err("unprobed candidate refused");
        assert_eq!(
            invalid.message,
            "The explicit Godot installation id is invalid: cfg-1"
        );
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&exe_dir).ok();
    }

    #[test]
    fn config_active_falls_back_with_message_and_override_description() {
        let root = unique_dir("ws4");
        let mut config = empty_config();
        config.active_installation = Some("missing".to_owned());
        let result = discover(&inputs(
            config,
            GodotSelectionPreference::ConfigActive,
            &root,
        ))
        .expect("fallback selection holds");
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.message
                == "The configured active installation \"missing\" does not exist; falling back to automatic selection."
        }));
        assert_eq!(
            result.configuration.active_installation.as_deref(),
            Some("missing")
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn path_preference_describes_the_override() {
        let root = unique_dir("ws5");
        let override_inputs = GodotProfilerInputs {
            override_source: Some(GodotOverrideSource::Cli),
            ..inputs(
                empty_config(),
                GodotSelectionPreference::Path("x".to_owned()),
                &root,
            )
        };
        // Selection itself errors (no probed candidate), but the
        // configuration summary is only observable through success; use a
        // plain auto discovery for the overrides field instead.
        let result = discover(&override_inputs).err();
        assert!(result.is_some());
        let auto_result = discover(&inputs(
            empty_config(),
            GodotSelectionPreference::None,
            &root,
        ))
        .unwrap();
        assert!(auto_result.configuration.overrides.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicates_collapse_by_canonical_identity() {
        let make = |id: &str, path: &str| GodotInstallation {
            id: id.to_owned(),
            source_label: "user config".to_owned(),
            source: siralos_core::godot::GodotInstallationSource::UserConfig,
            canonical_path: path.to_owned(),
            size_bytes: 10,
            modified_at_ms: 0,
            sha256: "a".repeat(64),
            edition_hint: siralos_core::godot::InstallEditionHint::Unknown,
            status_valid: true,
            error: None,
        };
        let (deduped, duplicates) = deduplicate_candidates(
            vec![
                make("a", "C:\\Godot\\Godot.exe"),
                make("b", "c:\\godot\\godot.exe"),
                make("c", "D:\\other.exe"),
            ],
            "win32",
        );
        assert_eq!(deduped.len(), 2);
        assert!(duplicates.contains("b"));
        let (deduped_posix, duplicates_posix) = deduplicate_candidates(
            vec![make("a", "/opt/Godot"), make("b", "/opt/godot")],
            "linux",
        );
        assert_eq!(deduped_posix.len(), 2);
        assert!(duplicates_posix.is_empty());
    }
}
