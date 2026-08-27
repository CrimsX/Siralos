//! Unified development surface routing (Stage 3 milestone 11,
//! ADR 0027).
//!
//! Mirrors `packages/core/src/godot/development/development-surface.ts`.
//! The host determines which Godot surfaces a task requires from
//! host-observed state only — the request text, verified/candidate
//! touchpoints, and the project surface inventory — never from model
//! claims. The two request-signal matchers emulate the oracle's
//! case-insensitive JavaScript regular expressions including their `\b`
//! boundary semantics; no regex dependency enters the core.

use super::DevelopmentError;

/// Which Godot surfaces a development task routes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentSurfaceKind {
    /// Only GDScript surfaces.
    ScriptOnly,
    /// Only scene/resource surfaces.
    NativeOnly,
    /// Both script and scene/resource surfaces.
    Mixed,
    /// No Godot surface evidence.
    None,
}

impl DevelopmentSurfaceKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScriptOnly => "script_only",
            Self::NativeOnly => "native_only",
            Self::Mixed => "mixed",
            Self::None => "none",
        }
    }
}

/// One host-observed candidate or verified touchpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentSurfaceTouchpoint {
    /// Workspace-relative path.
    pub path: String,
    /// Distinction preserved from workspace scope (never promoted).
    pub status: DevelopmentTouchpointStatus,
}

/// Touchpoint status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevelopmentTouchpointStatus {
    /// Verified by the host.
    Verified,
    /// Candidate only.
    Candidate,
}

impl DevelopmentTouchpointStatus {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Candidate => "candidate",
        }
    }
}

/// Bounded project surface inventory from static inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectSurfaces {
    /// The project references scenes.
    pub has_scenes: bool,
    /// The project references resources.
    pub has_resources: bool,
    /// The project contains GDScript.
    pub has_scripts: bool,
}

/// Inputs for one surface-routing decision.
pub struct DevelopmentSurfaceInput<'a> {
    /// Request text (host-observed).
    pub request: &'a str,
    /// Host-observed verified/candidate touchpoints (may be empty).
    pub touchpoints: &'a [DevelopmentSurfaceTouchpoint],
    /// Bounded project inventory, when derived.
    pub project_surfaces: Option<ProjectSurfaces>,
}

/// One deterministic routing decision with its recorded evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentSurfaceDecision {
    /// Routing kind.
    pub kind: DevelopmentSurfaceKind,
    /// Human-readable routing rationale (bounded, deterministic).
    pub rationale: String,
    /// Host-observed evidence the decision used.
    pub evidence: Vec<String>,
}

/// Path-based surface detection: `.tscn`/`.tres` are native, `.gd` is
/// script. Case-sensitive, mirroring the oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSurface {
    /// GDScript file.
    Script,
    /// Scene or resource file.
    Native,
    /// Anything else.
    Other,
}

/// Classify one path into its development surface.
#[must_use]
pub fn classify_development_surface_path(path: &str) -> PathSurface {
    if path.ends_with(".tscn") || path.ends_with(".tres") {
        return PathSurface::Native;
    }
    if path.ends_with(".gd") {
        return PathSurface::Script;
    }
    PathSurface::Other
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Emulate `/\b(?:…)\b/i` for a fixed literal alternative list: any
/// occurrence whose start and end both sit on JS word boundaries.
fn literal_alternation_matches(text: &str, alternatives: &[&str]) -> bool {
    let bytes = text.as_bytes();
    let lower = text.to_lowercase();
    for alternative in alternatives {
        let token = alternative.to_lowercase();
        let token_bytes = token.as_bytes();
        if token_bytes.is_empty() || token_bytes.len() > lower.len() {
            continue;
        }
        for index in 0..=lower.len() - token_bytes.len() {
            if &lower[index..index + token_bytes.len()] != token.as_str() {
                continue;
            }
            let before =
                if index == 0 { None } else { Some(bytes[index - 1]) };
            let after = bytes.get(index + token_bytes.len()).copied();
            let left = before.map(is_word_byte).unwrap_or(false);
            let right = after.map(is_word_byte).unwrap_or(false);
            let first_is_word = is_word_byte(token_bytes[0]);
            let last_is_word =
                is_word_byte(token_bytes[token_bytes.len() - 1]);
            if left != first_is_word && right != last_is_word {
                return true;
            }
        }
    }
    false
}

/// Emulate the `[A-Za-z0-9_/-]+\.gd` alternative: a run of path-class
/// characters ending in a boundary-checked `.gd`.
fn gd_file_reference_matches(text: &str) -> bool {
    let bytes = text.as_bytes();
    let lower = text.to_lowercase();
    let needle = b".gd";
    let is_run_class = |byte: u8| {
        byte.is_ascii_alphanumeric()
            || byte == b'_'
            || byte == b'/'
            || byte == b'-'
    };
    for index in 0..lower.len().saturating_sub(2) {
        if &lower.as_bytes()[index..index + 3] != needle {
            continue;
        }
        let after = bytes.get(index + 3).copied();
        let right = after.map(is_word_byte).unwrap_or(false);
        if right {
            continue;
        }
        // JavaScript tries every start position inside the class run, so
        // a boundary at ANY run start (with a boundary after `.gd`)
        // matches — not just the maximal one.
        let mut start = index;
        loop {
            let left = if start == 0 {
                false
            } else {
                is_word_byte(bytes[start - 1])
            };
            let first_is_word = is_word_byte(bytes[start]);
            if left != first_is_word {
                return true;
            }
            if start == 0 || !is_run_class(bytes[start - 1]) {
                break;
            }
            start -= 1;
        }
    }
    false
}

fn export_var_matches(text: &str) -> bool {
    let lower = text.to_lowercase();
    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(relative) = lower[search_from..].find("export") {
        let start = search_from + relative;
        let after_export = start + "export".len();
        let mut cursor = after_export;
        while cursor < bytes.len()
            && (bytes[cursor] == b' ' || bytes[cursor] == b'\t')
        {
            cursor += 1;
        }
        if cursor > after_export && lower[cursor..].starts_with("var") {
            let end = cursor + "var".len();
            let before =
                if start == 0 { None } else { Some(bytes[start - 1]) };
            let after = bytes.get(end).copied();
            let left = before.map(is_word_byte).unwrap_or(false);
            let right = after.map(is_word_byte).unwrap_or(false);
            if left && !right {
                return true;
            }
        }
        search_from = start + 1;
    }
    false
}

fn script_signal_matches(request: &str) -> bool {
    gd_file_reference_matches(request)
        || literal_alternation_matches(request, &["gdscript", "@export"])
        || export_var_matches(request)
}

/// Deterministic host-owned surface classification. Native involvement
/// is triggered by native touchpoints or an explicit scene/resource
/// request reference; script involvement by script touchpoints or
/// explicit GDScript request terminology. The decision never comes from
/// a model assertion.
pub fn classify_development_surface(
    input: DevelopmentSurfaceInput<'_>,
) -> DevelopmentSurfaceDecision {
    let mut evidence: Vec<String> = Vec::new();
    let touches_script = input.touchpoints.iter().find(|touchpoint| {
        classify_development_surface_path(&touchpoint.path)
            == PathSurface::Script
    });
    if let Some(touchpoint) = touches_script {
        evidence.push(format!(
            "touchpoint {} ({}) is GDScript",
            touchpoint.path,
            touchpoint.status.as_str()
        ));
    }
    let touches_native = input.touchpoints.iter().find(|touchpoint| {
        classify_development_surface_path(&touchpoint.path)
            == PathSurface::Native
    });
    if let Some(touchpoint) = touches_native {
        evidence.push(format!(
            "touchpoint {} ({}) is scene/resource",
            touchpoint.path,
            touchpoint.status.as_str()
        ));
    }
    let request_mentions_native = literal_alternation_matches(
        input.request,
        &[
            ".tscn",
            ".tres",
            "scene",
            "resource",
            "node",
            "property",
            "signal",
            "autoload",
            "project.godot",
        ],
    );
    if request_mentions_native {
        evidence
            .push("request references scene/resource terminology".to_owned());
    }
    let request_mentions_script = script_signal_matches(input.request);
    if request_mentions_script {
        evidence.push("request references GDScript terminology".to_owned());
    }
    let project_has_native = input
        .project_surfaces
        .is_some_and(|surfaces| surfaces.has_scenes || surfaces.has_resources);
    if project_has_native {
        evidence.push(
            "project surface inventory includes scenes/resources".to_owned(),
        );
    }
    let project_has_scripts =
        input.project_surfaces.is_some_and(|surfaces| surfaces.has_scripts);
    if project_has_scripts {
        evidence.push("project surface inventory includes scripts".to_owned());
    }

    let native = touches_native.is_some()
        || request_mentions_native
        || project_has_native;
    let script = touches_script.is_some()
        || request_mentions_script
        || project_has_scripts;

    let (kind, rationale) = if native && script {
        (
            DevelopmentSurfaceKind::Mixed,
            "Host-observed evidence shows both GDScript and scene/resource surfaces; the task routes to the unified mixed-workflow path.",
        )
    } else if native {
        (
            DevelopmentSurfaceKind::NativeOnly,
            "Host-observed evidence shows only scene/resource surfaces; no script change is routed.",
        )
    } else if script {
        (
            DevelopmentSurfaceKind::ScriptOnly,
            "Host-observed evidence shows only GDScript surfaces; the task keeps the existing script path.",
        )
    } else {
        (
            DevelopmentSurfaceKind::None,
            "No Godot surface evidence is host-observed; no mutation surface is routed for this request.",
        )
    };
    DevelopmentSurfaceDecision {
        kind,
        rationale: rationale.to_owned(),
        evidence,
    }
}

const _: Option<DevelopmentError> = None;

#[cfg(test)]
mod tests {
    use super::{
        DevelopmentSurfaceInput, DevelopmentSurfaceKind,
        DevelopmentSurfaceTouchpoint, DevelopmentTouchpointStatus,
        PathSurface, ProjectSurfaces, classify_development_surface,
        classify_development_surface_path,
    };

    fn touchpoint(path: &str) -> DevelopmentSurfaceTouchpoint {
        DevelopmentSurfaceTouchpoint {
            path: path.to_owned(),
            status: DevelopmentTouchpointStatus::Verified,
        }
    }

    #[test]
    fn paths_classify_by_extension() {
        assert_eq!(
            classify_development_surface_path("res://a.tscn"),
            PathSurface::Native
        );
        assert_eq!(
            classify_development_surface_path("res://b.tres"),
            PathSurface::Native
        );
        assert_eq!(
            classify_development_surface_path("res://c.gd"),
            PathSurface::Script
        );
        assert_eq!(
            classify_development_surface_path("res://d.txt"),
            PathSurface::Other
        );
    }

    #[test]
    fn mixed_touchpoints_route_to_the_mixed_path_with_ordered_evidence() {
        let decision = classify_development_surface(DevelopmentSurfaceInput {
            request: "update the HUD",
            touchpoints: &[
                touchpoint("res://hud.tscn"),
                touchpoint("res://hud.gd"),
            ],
            project_surfaces: None,
        });
        assert_eq!(decision.kind, DevelopmentSurfaceKind::Mixed);
        // The oracle evaluates script touchpoints before native ones;
        // this request carries no scene/resource terminology of its own.
        assert_eq!(
            decision.evidence,
            vec![
                "touchpoint res://hud.gd (verified) is GDScript",
                "touchpoint res://hud.tscn (verified) is scene/resource",
            ]
        );
        assert!(decision.rationale.contains("mixed-workflow"));
    }

    #[test]
    fn request_terminology_routes_without_touchpoints() {
        let script_only =
            classify_development_surface(DevelopmentSurfaceInput {
                request: "refactor res://enemy.gd to add gdscript helpers",
                touchpoints: &[],
                project_surfaces: None,
            });
        assert_eq!(script_only.kind, DevelopmentSurfaceKind::ScriptOnly);
        let native_only =
            classify_development_surface(DevelopmentSurfaceInput {
                request: "wire the pressed signal on main.tscn",
                touchpoints: &[],
                project_surfaces: None,
            });
        assert_eq!(native_only.kind, DevelopmentSurfaceKind::NativeOnly);
    }

    #[test]
    fn no_evidence_routes_none() {
        let decision = classify_development_surface(DevelopmentSurfaceInput {
            request: "summarize the repository",
            touchpoints: &[],
            project_surfaces: None,
        });
        assert_eq!(decision.kind, DevelopmentSurfaceKind::None);
        assert!(decision.evidence.is_empty());
        assert!(decision.rationale.contains("No Godot surface evidence"));
    }

    #[test]
    fn project_inventory_flags_are_recorded_as_evidence() {
        let decision = classify_development_surface(DevelopmentSurfaceInput {
            request: "do the thing",
            touchpoints: &[],
            project_surfaces: Some(ProjectSurfaces {
                has_scenes: true,
                has_resources: false,
                has_scripts: true,
            }),
        });
        assert_eq!(decision.kind, DevelopmentSurfaceKind::Mixed);
        assert_eq!(
            decision.evidence,
            vec![
                "project surface inventory includes scenes/resources",
                "project surface inventory includes scripts",
            ]
        );
    }
}
