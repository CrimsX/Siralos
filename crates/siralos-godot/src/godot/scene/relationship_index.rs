//! Read-only Godot relationship index (R8).
//!
//! Mirrors `packages/core/src/godot/scene/relationship-index.ts`.

use std::collections::HashMap;

use super::limits::GODOT_SCENE_LIMITS;

/// Kind of a relationship edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotRelationshipKind {
    /// Scene inherits from another scene.
    SceneInherits,
    /// Scene instances another scene.
    SceneInstances,
    /// Scene uses a script.
    SceneUsesScript,
    /// Resource references another resource.
    ResourceReferences,
    /// Project main scene.
    ProjectMainScene,
    /// Project autoload.
    ProjectAutoload,
}

impl GodotRelationshipKind {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SceneInherits => "scene_inherits",
            Self::SceneInstances => "scene_instances",
            Self::SceneUsesScript => "scene_uses_script",
            Self::ResourceReferences => "resource_references",
            Self::ProjectMainScene => "project_main_scene",
            Self::ProjectAutoload => "project_autoload",
        }
    }
}

/// One relationship entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotRelationshipEntry {
    /// Workspace-relative source path.
    pub source_path: String,
    /// Revision handle when parsed.
    pub source_revision: Option<String>,
    /// Kind.
    pub kind: GodotRelationshipKind,
    /// Workspace-relative resolved target path.
    pub target_path: String,
    /// Target `uid://` when both path and UID are known.
    pub target_uid: Option<String>,
}

/// Bounded, session-scoped relationship index.
#[derive(Debug, Clone)]
pub struct GodotRelationshipIndex {
    max_entries: usize,
    by_source: HashMap<String, Vec<GodotRelationshipEntry>>,
    by_target: HashMap<String, Vec<GodotRelationshipEntry>>,
    order: Vec<String>,
    total: usize,
}

impl GodotRelationshipIndex {
    /// Create a new index.
    #[must_use]
    pub fn new(max_entries: Option<usize>) -> Self {
        Self {
            max_entries: max_entries
                .unwrap_or(GODOT_SCENE_LIMITS.max_index_entries),
            by_source: HashMap::new(),
            by_target: HashMap::new(),
            order: Vec::new(),
            total: 0,
        }
    }

    /// Replace all entries for `source_path`.
    pub fn record(
        &mut self,
        source_path: &str,
        entries: Vec<GodotRelationshipEntry>,
    ) {
        self.remove_source(source_path);
        if entries.is_empty() {
            return;
        }
        self.by_source.insert(source_path.to_owned(), entries.clone());
        self.order.push(source_path.to_owned());
        for entry in &entries {
            self.by_target
                .entry(entry.target_path.clone())
                .or_default()
                .push(entry.clone());
        }
        self.total += entries.len();
        self.evict_if_needed();
    }

    /// Immediate outgoing relationships of a source document.
    #[must_use]
    pub fn dependencies_of(
        &self,
        source_path: &str,
    ) -> Vec<GodotRelationshipEntry> {
        self.by_source.get(source_path).cloned().unwrap_or_default()
    }

    /// Incoming relationships: which parsed documents reference `target_path`.
    #[must_use]
    pub fn referrers_of(
        &self,
        target_path: &str,
    ) -> Vec<GodotRelationshipEntry> {
        self.by_target.get(target_path).cloned().unwrap_or_default()
    }

    /// True when the entry's recorded revision is not the current one.
    #[must_use]
    pub fn is_stale(
        &self,
        entry: &GodotRelationshipEntry,
        current_revision: Option<&str>,
    ) -> bool {
        match &entry.source_revision {
            Some(rev) => Some(rev.as_str()) != current_revision,
            None => false,
        }
    }

    /// Total entry count.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.total
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.by_source.clear();
        self.by_target.clear();
        self.order.clear();
        self.total = 0;
    }

    fn remove_source(&mut self, source_path: &str) {
        let Some(entries) = self.by_source.remove(source_path) else {
            return;
        };
        if let Some(pos) = self.order.iter().position(|s| s == source_path) {
            self.order.remove(pos);
        }
        for entry in &entries {
            if let Some(targets) = self.by_target.get_mut(&entry.target_path) {
                targets.retain(|c| c.source_path != source_path);
                if targets.is_empty() {
                    self.by_target.remove(&entry.target_path);
                }
            }
        }
        self.total = self.total.saturating_sub(entries.len());
    }

    fn evict_if_needed(&mut self) {
        while self.total > self.max_entries {
            let Some(oldest) = self.order.first().cloned() else {
                break;
            };
            self.remove_source(&oldest);
        }
    }
}

impl Default for GodotRelationshipIndex {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GodotRelationshipEntry, GodotRelationshipIndex, GodotRelationshipKind,
    };

    fn entry(source: &str, target: &str) -> GodotRelationshipEntry {
        GodotRelationshipEntry {
            source_path: source.to_owned(),
            source_revision: Some("rev_abc".to_owned()),
            kind: GodotRelationshipKind::SceneUsesScript,
            target_path: target.to_owned(),
            target_uid: None,
        }
    }

    #[test]
    fn records_and_queries() {
        let mut idx = GodotRelationshipIndex::new(Some(10));
        idx.record("a.tscn", vec![entry("a.tscn", "b.gd")]);
        assert_eq!(idx.dependencies_of("a.tscn").len(), 1);
        assert_eq!(idx.referrers_of("b.gd").len(), 1);
        assert_eq!(idx.size(), 1);
    }

    #[test]
    fn stale_detection() {
        let mut idx = GodotRelationshipIndex::new(Some(10));
        idx.record("a.tscn", vec![entry("a.tscn", "b.gd")]);
        let e = idx.dependencies_of("a.tscn")[0].clone();
        assert!(idx.is_stale(&e, Some("rev_xyz")));
        assert!(!idx.is_stale(&e, Some("rev_abc")));
    }
}
