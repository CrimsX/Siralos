//! Read-only Godot scene/resource intelligence adapter (R8-6a).
//!
//! Mirrors `packages/adapters/src/godot/intelligence/scene-intelligence-service.ts`.
//! Static bounded workspace reads only: no Godot process, no mutation,
//! no checkpoint. Every derived model binds to the exact workspace
//! revision of the file state that was read.

use std::path::{Path, PathBuf};

use siralos_core::godot::scene::{
    GODOT_SCENE_LIMITS, GodotDependencyResult, GodotInspectionOutcome,
    GodotIntelligenceStatus, GodotProjectRelationshipResult,
    GodotRelationshipEntry, GodotRelationshipIndex, GodotRelationshipKind,
    GodotResourceInspectionResult, GodotSceneInspectionResult,
    ResPathResolution, build_scene_node_tree, parse_godot_resource,
    parse_godot_scene, resolve_res_path,
};
use siralos_core::identity::sha256_hex;
use siralos_core::workspace::revision::{
    WorkspaceRevisionRegistry, WorkspaceRevisionRegistryOptions,
};

use crate::workspace::fs::{
    BoundedFileRead, DEFAULT_EXCLUDED_DIRECTORIES, decode_utf8, looks_binary,
    read_complete_file_bounded,
};
use crate::workspace::list::excluded_component;
use crate::workspace::resolve::resolve_workspace_path;
use crate::workspace::root::{WorkspaceRootError, resolve_workspace_root};

/// Read-only Godot scene/resource intelligence service.
///
/// The single application-owned subsystem for current parsed Godot
/// semantic state. All inspection is static: bounded workspace reads
/// only. Every result binds to the exact workspace revision of the
/// file state that was read.
#[derive(Debug)]
pub struct GodotSceneIntelligenceService {
    /// Canonical workspace root.
    canonical_root: PathBuf,
    /// Fingerprint of the workspace root.
    workspace_relative_root_fingerprint: String,
    /// Session revision registry.
    revisions: WorkspaceRevisionRegistry,
    /// Relationship index.
    index: GodotRelationshipIndex,
    /// Maximum document bytes.
    max_document_bytes: usize,
}

impl GodotSceneIntelligenceService {
    /// Create a new service for the given workspace root.
    ///
    /// Canonicalizes the root, computes the workspace fingerprint,
    /// creates the revision registry and the relationship index.
    ///
    /// # Errors
    ///
    /// Returns `WorkspaceRootError` when the root cannot be
    /// canonicalized or is not a directory.
    pub fn new(
        workspace_root: impl AsRef<Path>,
    ) -> Result<Self, WorkspaceRootError> {
        let canonical_root = resolve_workspace_root(workspace_root.as_ref())?;
        let workspace_relative_root_fingerprint =
            sha256_hex(canonical_root.to_string_lossy().as_bytes());
        let revisions =
            WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
                workspace_fingerprint: workspace_relative_root_fingerprint
                    .clone(),
                max_entries: None,
            })
            .expect(
                "revision registry construction succeeds with default limit",
            );
        let index = GodotRelationshipIndex::new(None);
        let max_document_bytes = GODOT_SCENE_LIMITS.max_document_bytes;
        Ok(Self {
            canonical_root,
            workspace_relative_root_fingerprint,
            revisions,
            index,
            max_document_bytes,
        })
    }

    /// Inspect a `.tscn` scene document.
    pub fn inspect_scene(&mut self, path: &str) -> GodotSceneInspectionResult {
        if path.contains('\0') {
            return GodotSceneInspectionResult {
                outcome: GodotInspectionOutcome {
                    status: GodotIntelligenceStatus::Denied,
                    message: Some("Path contains a null byte.".to_owned()),
                    path: path.to_owned(),
                    revision: None,
                    document: None,
                },
                tree: None,
            };
        }
        let resolved = match resolve_workspace_path(&self.canonical_root, path)
        {
            Ok(resolved) => resolved,
            Err(rejection) => {
                if is_lexically_contained(path) {
                    return GodotSceneInspectionResult {
                        outcome: GodotInspectionOutcome {
                            status: GodotIntelligenceStatus::NotFound,
                            message: Some(
                                "File does not exist in the workspace."
                                    .to_owned(),
                            ),
                            path: path.to_owned(),
                            revision: None,
                            document: None,
                        },
                        tree: None,
                    };
                }
                return GodotSceneInspectionResult {
                    outcome: GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Denied,
                        message: Some(rejection.to_string()),
                        path: path.to_owned(),
                        revision: None,
                        document: None,
                    },
                    tree: None,
                };
            }
        };
        if let Some(excluded) = excluded_component(
            &resolved.workspace_relative_path,
            &DEFAULT_EXCLUDED_DIRECTORIES,
        ) {
            return GodotSceneInspectionResult {
                outcome: GodotInspectionOutcome {
                    status: GodotIntelligenceStatus::Denied,
                    message: Some(format!(
                        "Path is inside the excluded directory {excluded}."
                    )),
                    path: path.to_owned(),
                    revision: None,
                    document: None,
                },
                tree: None,
            };
        }
        let bytes = match read_complete_file_bounded(
            &resolved.absolute_path,
            self.max_document_bytes,
        ) {
            BoundedFileRead::Complete(bytes) => bytes,
            BoundedFileRead::NotReadable => {
                return GodotSceneInspectionResult {
                    outcome: GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::NotFound,
                        message: Some(
                            "File does not exist in the workspace.".to_owned(),
                        ),
                        path: path.to_owned(),
                        revision: None,
                        document: None,
                    },
                    tree: None,
                };
            }
            BoundedFileRead::TooLarge => {
                return GodotSceneInspectionResult {
                    outcome: GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Unreadable,
                        message: Some(
                            "File exceeds the 8 MiB limit.".to_owned(),
                        ),
                        path: resolved.workspace_relative_path.clone(),
                        revision: None,
                        document: None,
                    },
                    tree: None,
                };
            }
            BoundedFileRead::IoError(error) => {
                return GodotSceneInspectionResult {
                    outcome: GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Failed,
                        message: Some(format!("Failed to read file: {error}")),
                        path: resolved.workspace_relative_path.clone(),
                        revision: None,
                        document: None,
                    },
                    tree: None,
                };
            }
        };
        if looks_binary(&bytes) {
            return GodotSceneInspectionResult {
                outcome: GodotInspectionOutcome {
                    status: GodotIntelligenceStatus::Unsupported,
                    message: Some("File appears to be binary.".to_owned()),
                    path: resolved.workspace_relative_path.clone(),
                    revision: None,
                    document: None,
                },
                tree: None,
            };
        }
        let Some(content) = decode_utf8(&bytes) else {
            return GodotSceneInspectionResult {
                outcome: GodotInspectionOutcome {
                    status: GodotIntelligenceStatus::Unreadable,
                    message: Some("File is not valid UTF-8.".to_owned()),
                    path: resolved.workspace_relative_path.clone(),
                    revision: None,
                    document: None,
                },
                tree: None,
            };
        };
        let sha256 = sha256_hex(&bytes);
        let revision =
            self.revisions.issue(&resolved.workspace_relative_path, &sha256);
        let parsed = parse_godot_scene(
            &content,
            &resolved.workspace_relative_path,
            Some(revision.clone()),
        );
        let tree = parsed.document.as_ref().map(build_scene_node_tree);
        if let Some(document) = parsed.document.as_ref() {
            let mut entries: Vec<GodotRelationshipEntry> = Vec::new();
            if let Some(base) = document.base_scene.as_ref() {
                if let Some(target_path) = base.resolved_path.as_ref() {
                    entries.push(GodotRelationshipEntry {
                        source_path: resolved.workspace_relative_path.clone(),
                        source_revision: Some(revision.clone()),
                        kind: GodotRelationshipKind::SceneInherits,
                        target_path: target_path.clone(),
                        target_uid: base.resource.uid.clone(),
                    });
                }
            }
            for node in &document.nodes {
                if let Some(instance) = node.instance.as_ref() {
                    if let Some(target_path) = instance.resolved_path.as_ref()
                    {
                        entries.push(GodotRelationshipEntry {
                            source_path: resolved
                                .workspace_relative_path
                                .clone(),
                            source_revision: Some(revision.clone()),
                            kind: GodotRelationshipKind::SceneInstances,
                            target_path: target_path.clone(),
                            target_uid: instance.resource.uid.clone(),
                        });
                    }
                }
                if let Some(script) = node.script.as_ref() {
                    if let Some(target_path) = script.resolved_path.as_ref() {
                        entries.push(GodotRelationshipEntry {
                            source_path: resolved
                                .workspace_relative_path
                                .clone(),
                            source_revision: Some(revision.clone()),
                            kind: GodotRelationshipKind::SceneUsesScript,
                            target_path: target_path.clone(),
                            target_uid: script.resource.uid.clone(),
                        });
                    }
                }
            }
            for external in &document.external_resources {
                if let Some(raw) = external.path.as_ref() {
                    if let ResPathResolution::Ok(target_path) =
                        resolve_res_path(raw)
                    {
                        entries.push(GodotRelationshipEntry {
                            source_path: resolved
                                .workspace_relative_path
                                .clone(),
                            source_revision: Some(revision.clone()),
                            kind: GodotRelationshipKind::ResourceReferences,
                            target_path,
                            target_uid: external.uid.clone(),
                        });
                    }
                }
            }
            self.index.record(&resolved.workspace_relative_path, entries);
        }
        GodotSceneInspectionResult {
            outcome: GodotInspectionOutcome {
                status: GodotIntelligenceStatus::Ok,
                message: None,
                path: resolved.workspace_relative_path.clone(),
                revision: Some(revision),
                document: Some(parsed),
            },
            tree,
        }
    }

    /// Inspect a `.tres` / `.theme` resource document.
    pub fn inspect_resource(
        &mut self,
        path: &str,
    ) -> GodotResourceInspectionResult {
        if path.contains('\0') {
            return GodotResourceInspectionResult(GodotInspectionOutcome {
                status: GodotIntelligenceStatus::Denied,
                message: Some("Path contains a null byte.".to_owned()),
                path: path.to_owned(),
                revision: None,
                document: None,
            });
        }
        let resolved = match resolve_workspace_path(&self.canonical_root, path)
        {
            Ok(resolved) => resolved,
            Err(rejection) => {
                if is_lexically_contained(path) {
                    return GodotResourceInspectionResult(
                        GodotInspectionOutcome {
                            status: GodotIntelligenceStatus::NotFound,
                            message: Some(
                                "File does not exist in the workspace."
                                    .to_owned(),
                            ),
                            path: path.to_owned(),
                            revision: None,
                            document: None,
                        },
                    );
                }
                return GodotResourceInspectionResult(
                    GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Denied,
                        message: Some(rejection.to_string()),
                        path: path.to_owned(),
                        revision: None,
                        document: None,
                    },
                );
            }
        };
        if let Some(excluded) = excluded_component(
            &resolved.workspace_relative_path,
            &DEFAULT_EXCLUDED_DIRECTORIES,
        ) {
            return GodotResourceInspectionResult(GodotInspectionOutcome {
                status: GodotIntelligenceStatus::Denied,
                message: Some(format!(
                    "Path is inside the excluded directory {excluded}."
                )),
                path: path.to_owned(),
                revision: None,
                document: None,
            });
        }
        let bytes = match read_complete_file_bounded(
            &resolved.absolute_path,
            self.max_document_bytes,
        ) {
            BoundedFileRead::Complete(bytes) => bytes,
            BoundedFileRead::NotReadable => {
                return GodotResourceInspectionResult(
                    GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::NotFound,
                        message: Some(
                            "File does not exist in the workspace.".to_owned(),
                        ),
                        path: path.to_owned(),
                        revision: None,
                        document: None,
                    },
                );
            }
            BoundedFileRead::TooLarge => {
                return GodotResourceInspectionResult(
                    GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Unreadable,
                        message: Some(
                            "File exceeds the 8 MiB limit.".to_owned(),
                        ),
                        path: resolved.workspace_relative_path.clone(),
                        revision: None,
                        document: None,
                    },
                );
            }
            BoundedFileRead::IoError(error) => {
                return GodotResourceInspectionResult(
                    GodotInspectionOutcome {
                        status: GodotIntelligenceStatus::Failed,
                        message: Some(format!("Failed to read file: {error}")),
                        path: resolved.workspace_relative_path.clone(),
                        revision: None,
                        document: None,
                    },
                );
            }
        };
        if looks_binary(&bytes) {
            return GodotResourceInspectionResult(GodotInspectionOutcome {
                status: GodotIntelligenceStatus::Unsupported,
                message: Some("File appears to be binary.".to_owned()),
                path: resolved.workspace_relative_path.clone(),
                revision: None,
                document: None,
            });
        }
        let Some(content) = decode_utf8(&bytes) else {
            return GodotResourceInspectionResult(GodotInspectionOutcome {
                status: GodotIntelligenceStatus::Unreadable,
                message: Some("File is not valid UTF-8.".to_owned()),
                path: resolved.workspace_relative_path.clone(),
                revision: None,
                document: None,
            });
        };
        let sha256 = sha256_hex(&bytes);
        let revision =
            self.revisions.issue(&resolved.workspace_relative_path, &sha256);
        let parsed = parse_godot_resource(
            &content,
            &resolved.workspace_relative_path,
            Some(revision.clone()),
        );
        if let Some(document) = parsed.document.as_ref() {
            let mut entries: Vec<GodotRelationshipEntry> = Vec::new();
            if let Some(script) = document.script.as_ref() {
                if let Some(target_path) = script.resolved_path.as_ref() {
                    entries.push(GodotRelationshipEntry {
                        source_path: resolved.workspace_relative_path.clone(),
                        source_revision: Some(revision.clone()),
                        kind: GodotRelationshipKind::ResourceReferences,
                        target_path: target_path.clone(),
                        target_uid: script.resource.uid.clone(),
                    });
                }
            }
            for external in &document.external_resources {
                if let Some(raw) = external.path.as_ref() {
                    if let ResPathResolution::Ok(target_path) =
                        resolve_res_path(raw)
                    {
                        entries.push(GodotRelationshipEntry {
                            source_path: resolved
                                .workspace_relative_path
                                .clone(),
                            source_revision: Some(revision.clone()),
                            kind: GodotRelationshipKind::ResourceReferences,
                            target_path,
                            target_uid: external.uid.clone(),
                        });
                    }
                }
            }
            self.index.record(&resolved.workspace_relative_path, entries);
        }
        GodotResourceInspectionResult(GodotInspectionOutcome {
            status: GodotIntelligenceStatus::Ok,
            message: None,
            path: resolved.workspace_relative_path.clone(),
            revision: Some(revision),
            document: Some(parsed),
        })
    }

    /// Stub dependencies query.
    ///
    /// Returns empty edges; checks existence via the relationship index
    /// and returns `NotFound` when no entry is known.
    #[must_use]
    pub fn dependencies(&self, path: &str) -> GodotDependencyResult {
        let deps = self.index.dependencies_of(path);
        if deps.is_empty() && self.index.referrers_of(path).is_empty() {
            return GodotDependencyResult {
                status: GodotIntelligenceStatus::NotFound,
                message: Some("No indexed relationships for path.".to_owned()),
                root_path: path.to_owned(),
                revision: None,
                edges: Vec::new(),
                referrers: Vec::new(),
                files_visited: 0,
                truncated_depth: false,
                truncated_files: false,
                cycle_detected: false,
                cycle_path: None,
            };
        }
        let revision = self.revisions.current_revision(path).map(String::from);
        let referrers = self.index.referrers_of(path);
        GodotDependencyResult {
            status: GodotIntelligenceStatus::Ok,
            message: None,
            root_path: path.to_owned(),
            revision,
            edges: Vec::new(),
            referrers,
            files_visited: 0,
            truncated_depth: false,
            truncated_files: false,
            cycle_detected: false,
            cycle_path: None,
        }
    }

    /// Stub project relationships query.
    #[must_use]
    pub fn project_relationships(&self) -> GodotProjectRelationshipResult {
        GodotProjectRelationshipResult {
            status: "ok".to_owned(),
            message: None,
            main_scene: None,
            autoloads: Vec::new(),
            input_actions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Return the workspace fingerprint.
    #[must_use]
    pub fn workspace_fingerprint(&self) -> &str {
        &self.workspace_relative_root_fingerprint
    }

    /// Return the canonical root.
    #[must_use]
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
}

/// Returns true when `path` is a lexically contained relative path.
///
/// A path is lexically contained when it is non-empty, contains no NUL,
/// is not absolute or drive-prefixed, and a lexical `.. ` walk never
/// escapes the workspace root.
#[must_use]
fn is_lexically_contained(path: &str) -> bool {
    if path.contains('\0') || path.is_empty() {
        return false;
    }
    if is_absolute_pattern(path) {
        return false;
    }
    let mut depth: usize = 0;
    for component in path.split(['/', '\\']) {
        match component {
            "" | "." => {}
            ".." => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            _ => {
                depth += 1;
            }
        }
    }
    true
}

/// Check whether `value` matches the absolute-path pattern.
#[must_use]
fn is_absolute_pattern(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    bytes.first().is_some_and(|byte| *byte == b'/' || *byte == b'\\')
}
