//! Deterministic impact analyzer over an injected relationship source.
//!
//! Mirrors `packages/core/src/godot/impact/impact-analyzer.ts`: a pure
//! derivation that owns no relationships, no revisions, no filesystem,
//! and no process. Verified-vs-candidate confidence is preserved;
//! traversal is breadth-first, bounded, and cycle-safe; stale
//! relationships are excluded from related surfaces and disclosed as
//! diagnostics; absence of a static relationship is never claimed as
//! proof of runtime non-impact; identical inputs produce identical
//! manifests.

use std::collections::{HashMap, HashSet};

use super::model::{
    ImpactCompleteness, ImpactConfidence, ImpactDiagnostic,
    ImpactRegressionArea, ImpactRelation, ImpactRelationKind, ImpactSurface,
    ImpactSurfaceKind, ImpactValidationRecommendation, ReviewContextError,
    ReviewContextLimits, ReviewContextManifest, ValidationKind,
    ValidationPriority, validate_review_context_manifest,
};

/// One directed relationship edge provided by the relationship source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactEdge {
    /// Relationship kind.
    pub kind: ImpactRelationKind,
    /// Source endpoint of the recorded relationship.
    pub from_path: String,
    /// Target endpoint of the recorded relationship.
    pub to_path: String,
    /// True when the recorded relationship is no longer current (stale).
    pub stale: bool,
}

/// One serialized signal connection (static scene evidence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactSignalConnection {
    /// Signal name.
    pub signal: String,
    /// Emitting node path (scene-local).
    pub source_node: String,
    /// Receiving node path (scene-local).
    pub target_node: String,
    /// Receiving method name.
    pub target_method: String,
}

/// The relationship evidence source, implemented by adapters over the
/// revision-aware relationship index, the static project scan, and
/// bounded scene parsing ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â never over runtime entities Siralos cannot
/// prove. Synchronous by design: the core owns no async runtime.
pub trait ImpactRelationshipSource {
    /// Relationships the surface participates in as the source.
    fn outgoing(&self, path: &str) -> Vec<ImpactEdge>;
    /// Relationships pointing at the surface.
    fn incoming(&self, path: &str) -> Vec<ImpactEdge>;
    /// Serialized signal connections of one scene (bounded; empty for
    /// non-scenes).
    fn signal_connections(&self, path: &str) -> Vec<ImpactSignalConnection>;
    /// Autoload name when the path is a project autoload target.
    fn autoload_name(&self, path: &str) -> Option<String>;
    /// Workspace-relative main scene path, when known.
    fn main_scene(&self) -> Option<String>;
    /// Exact current workspace revision of the path, when known.
    fn current_revision(&self, path: &str) -> Option<String>;
    /// Bounded candidate test files plausibly covering the surface
    /// (heuristic; never verified coverage).
    fn candidate_tests(&self, path: &str) -> Vec<String>;
}

/// Inputs for one impact analysis.
pub struct AnalyzeImpactInput<'a> {
    /// Owning task id.
    pub task_id: &'a str,
    /// Task contract revision the analysis binds to.
    pub task_contract_revision: u64,
    /// Workspace-relative changed surfaces (overflow is truncated with a
    /// diagnostic).
    pub changed_paths: &'a [String],
    /// Injected relationship evidence source.
    pub source: &'a dyn ImpactRelationshipSource,
}

/// Byte-safe truncation: the validator rejects over-budget fields, so
/// the analyzer guarantees its own output fits its limits.
fn fit_bytes(text: &str, max_bytes: usize) -> String {
    let mut fitted = text.to_owned();
    while fitted.len() > max_bytes {
        fitted.pop();
    }
    fitted
}

fn is_test_script(path: &str) -> bool {
    let segment_is_test = |segment: &str| {
        segment == "tests"
            || segment == "test"
            || segment == "spec"
            || segment.starts_with("tests.")
            || segment.starts_with("test.")
            || segment.starts_with("spec.")
    };
    path.split('/').any(segment_is_test)
        || path.contains(".test.")
        || path.contains(".spec.")
}

fn surface_kind_of(
    path: &str,
    source: &dyn ImpactRelationshipSource,
) -> ImpactSurfaceKind {
    if source.autoload_name(path).is_some() {
        return ImpactSurfaceKind::Autoload;
    }
    if path == "project.godot" {
        return ImpactSurfaceKind::ProjectConfig;
    }
    let normalized = path.to_lowercase();
    if normalized.ends_with(".tscn") {
        return ImpactSurfaceKind::Scene;
    }
    if normalized.ends_with(".tres") || normalized.ends_with(".theme") {
        return ImpactSurfaceKind::Resource;
    }
    if normalized.ends_with(".gd") && is_test_script(path) {
        return ImpactSurfaceKind::Test;
    }
    ImpactSurfaceKind::Script
}

/// Insertion-ordered unique string collection (mirrors JavaScript Set
/// iteration order, which the oracle's determinism relies on).
#[derive(Default)]
struct OrderedStrings {
    entries: Vec<String>,
    seen: HashSet<String>,
}

impl OrderedStrings {
    fn insert(&mut self, value: String) -> bool {
        if self.seen.insert(value.clone()) {
            self.entries.push(value);
            true
        } else {
            false
        }
    }
}

#[derive(Clone)]
struct QueueEntry {
    path: String,
    depth: usize,
}

/// Derive the bounded review/validation context for the changed
/// surfaces. Deterministic: identical inputs produce identical
/// manifests.
pub fn analyze_impact(
    input: AnalyzeImpactInput<'_>,
) -> Result<ReviewContextManifest, ReviewContextError> {
    let mut changed_paths: Vec<String> = input
        .changed_paths
        .iter()
        .map(|path| path.trim().to_owned())
        .filter(|path| {
            !path.is_empty()
                && !path.contains('\\')
                && !path.starts_with('/')
                && !path.split('/').any(|segment| segment == "..")
        })
        .collect();
    if changed_paths.len() > ReviewContextLimits::MAX_PRIMARY_CHANGES {
        changed_paths.truncate(ReviewContextLimits::MAX_PRIMARY_CHANGES);
    }
    let mut diagnostics: Vec<ImpactDiagnostic> = Vec::new();
    if input.changed_paths.len() > ReviewContextLimits::MAX_PRIMARY_CHANGES {
        diagnostics.push(ImpactDiagnostic {
            code: "IMPACT.PRIMARY_BOUND".to_owned(),
            message: format!(
                "Impact analysis truncated to the first {} changed surfaces.",
                ReviewContextLimits::MAX_PRIMARY_CHANGES
            ),
        });
    }
    let primary_changes: Vec<ImpactSurface> = changed_paths
        .iter()
        .map(|path| {
            let kind = surface_kind_of(path, input.source);
            let autoload = input.source.autoload_name(path);
            ImpactSurface {
                path: path.clone(),
                kind,
                revision: input.source.current_revision(path),
                confidence: ImpactConfidence::Verified,
                evidence: "impact:changed-surface".to_owned(),
                note: autoload.map(|name| format!("project autoload: {name}")),
            }
        })
        .collect();

    let mut visited: HashSet<String> = HashSet::new();
    let mut related_surfaces: Vec<ImpactRelation> = Vec::new();
    // Undirected pair identity per relationship kind: each relationship
    // is recorded exactly once regardless of which endpoint is traversed
    // first, and distinct relationships to the same surface are all kept.
    let mut recorded_relation_keys: HashSet<String> = HashSet::new();
    let mut stale_pairs = OrderedStrings::default();
    let mut relations_visited = 0usize;
    let mut surfaces_visited = 0usize;
    let mut depth_bound_hit = false;
    let mut surface_bound_hit = false;
    let mut relation_bound_hit = false;

    'primaries: for primary in &changed_paths {
        if visited.contains(primary) {
            continue;
        }
        visited.insert(primary.clone());
        let mut queue = vec![QueueEntry { path: primary.clone(), depth: 0 }];
        while let Some(entry) = queue.first().cloned() {
            queue.remove(0);
            surfaces_visited += 1;
            let mut edges = input.source.outgoing(&entry.path);
            edges.extend(input.source.incoming(&entry.path));
            for edge in edges {
                relations_visited += 1;
                if relations_visited
                    > ReviewContextLimits::MAX_RELATIONS_VISITED
                {
                    relation_bound_hit = true;
                    break;
                }
                if edge.stale {
                    stale_pairs.insert(format!(
                        "{}->{}",
                        edge.from_path, edge.to_path
                    ));
                    continue;
                }
                if edge.from_path != entry.path && edge.to_path != entry.path {
                    continue;
                }
                // Normalize to traversal direction: source = the surface
                // being traversed, target = the other side.
                let source_path = entry.path.clone();
                let target_path = if edge.from_path == entry.path {
                    edge.to_path.clone()
                } else {
                    edge.from_path.clone()
                };
                let mut pair = [source_path.clone(), target_path.clone()];
                pair.sort();
                let pair_key = format!(
                    "{}\0{}\0{}",
                    edge.kind.as_str(),
                    pair[0],
                    pair[1]
                );
                if !recorded_relation_keys.contains(&pair_key) {
                    recorded_relation_keys.insert(pair_key);
                    let relation = ImpactRelation {
                        kind: edge.kind,
                        source_path: source_path.clone(),
                        target_path: target_path.clone(),
                        source_revision: input
                            .source
                            .current_revision(&source_path),
                        target_revision: input
                            .source
                            .current_revision(&target_path),
                        confidence: ImpactConfidence::Verified,
                        evidence: format!("index:{}", edge.kind.as_str()),
                        note: None,
                    };
                    if related_surfaces.len()
                        < ReviewContextLimits::MAX_RELATED_SURFACES
                    {
                        related_surfaces.push(relation);
                    } else {
                        relation_bound_hit = true;
                    }
                }
                if !visited.contains(&target_path)
                    && entry.depth < ReviewContextLimits::MAX_DEPTH
                {
                    if surfaces_visited
                        >= ReviewContextLimits::MAX_SURFACES_VISITED
                    {
                        surface_bound_hit = true;
                    } else {
                        visited.insert(target_path.clone());
                        queue.push(QueueEntry {
                            path: target_path.clone(),
                            depth: entry.depth + 1,
                        });
                    }
                }
                if entry.depth >= ReviewContextLimits::MAX_DEPTH
                    && !visited.contains(&target_path)
                {
                    depth_bound_hit = true;
                }
            }
            if relation_bound_hit {
                break 'primaries;
            }
        }
    }
    if depth_bound_hit || surface_bound_hit || relation_bound_hit {
        let mut parts: Vec<&str> = Vec::new();
        if depth_bound_hit {
            parts.push("depth bound reached");
        }
        if surface_bound_hit {
            parts.push("surface-count bound reached");
        }
        if relation_bound_hit {
            parts.push("relation-count bound reached");
        }
        diagnostics.push(ImpactDiagnostic {
            code: "IMPACT.TRAVERSAL_BOUND".to_owned(),
            message: parts.join(", "),
        });
    }

    // Serialized signal connections of related scenes (bounded, static).
    let mut scene_surfaces = OrderedStrings::default();
    for surface in &primary_changes {
        if surface_kind_of(&surface.path, input.source)
            == ImpactSurfaceKind::Scene
        {
            scene_surfaces.insert(surface.path.clone());
        }
    }
    for relation in &related_surfaces {
        if surface_kind_of(&relation.target_path, input.source)
            == ImpactSurfaceKind::Scene
        {
            scene_surfaces.insert(relation.target_path.clone());
        }
    }
    for scene_path in &scene_surfaces.entries {
        for connection in input.source.signal_connections(scene_path) {
            if related_surfaces.len()
                >= ReviewContextLimits::MAX_RELATED_SURFACES
            {
                relation_bound_hit = true;
                break;
            }
            related_surfaces.push(ImpactRelation {
                kind: ImpactRelationKind::SignalConnection,
                source_path: scene_path.clone(),
                target_path: scene_path.clone(),
                source_revision: input.source.current_revision(scene_path),
                target_revision: input.source.current_revision(scene_path),
                confidence: ImpactConfidence::Verified,
                evidence: "index:signal_connection".to_owned(),
                note: Some(format!(
                    "serialized connection {}: node {} -> node {}.{}",
                    connection.signal,
                    connection.source_node,
                    connection.target_node,
                    connection.target_method
                )),
            });
        }
    }

    // Candidate test surfaces (heuristic; never verified coverage).
    let mut candidate_test_paths: Vec<String> = Vec::new();
    for primary in &changed_paths {
        for test_path in input.source.candidate_tests(primary) {
            if candidate_test_paths.len()
                >= ReviewContextLimits::MAX_CANDIDATE_TESTS
            {
                break;
            }
            if candidate_test_paths.contains(&test_path)
                || &test_path == primary
            {
                continue;
            }
            candidate_test_paths.push(test_path.clone());
            if related_surfaces.len()
                < ReviewContextLimits::MAX_RELATED_SURFACES
            {
                related_surfaces.push(ImpactRelation {
                    kind: ImpactRelationKind::TestCovers,
                    source_path: primary.clone(),
                    target_path: test_path.clone(),
                    source_revision: input.source.current_revision(primary),
                    target_revision: input.source.current_revision(&test_path),
                    confidence: ImpactConfidence::Candidate,
                    evidence: "convention:test-surface".to_owned(),
                    note: None,
                });
            } else {
                relation_bound_hit = true;
            }
        }
    }

    // Staleness diagnostics (insertion order).
    for pair in &stale_pairs.entries {
        if diagnostics.len() >= ReviewContextLimits::MAX_DIAGNOSTICS {
            break;
        }
        diagnostics.push(ImpactDiagnostic {
            code: "IMPACT.STALE_RELATIONSHIP".to_owned(),
            message: format!(
                "Relationship {pair} is stale (its recorded source revision is no \
                 longer current); it was excluded from current impact."
            ),
        });
    }

    // Autoload/global reach: high-reach risk signal, never verified
    // impact on every project surface.
    let autoload_changed: Vec<String> = changed_paths
        .iter()
        .filter(|path| input.source.autoload_name(path).is_some())
        .cloned()
        .collect();
    if !autoload_changed.is_empty() {
        diagnostics.push(ImpactDiagnostic {
            code: "IMPACT.AUTOLOAD_GLOBAL".to_owned(),
            message: format!(
                "Changed autoload(s) {} have global reach that cannot be enumerated \
                 statically; impact beyond direct relations is candidate, not verified.",
                autoload_changed.join(", ")
            ),
        });
    }

    let regression_areas = build_regression_areas(
        &changed_paths,
        &related_surfaces,
        input.source,
    );
    let validation = build_validation_recommendations(
        &changed_paths,
        &related_surfaces,
        input.source,
        &autoload_changed,
    );
    let evidence = build_evidence_refs(&related_surfaces);

    let has_partial_condition = !stale_pairs.entries.is_empty()
        || !candidate_test_paths.is_empty()
        || !autoload_changed.is_empty()
        || validation.iter().any(|recommendation| {
            recommendation.priority
                == ValidationPriority::RuntimeEvidenceUnavailable
        });
    let has_bound_condition =
        depth_bound_hit || surface_bound_hit || relation_bound_hit;
    let completeness = if has_partial_condition {
        ImpactCompleteness::Partial
    } else if has_bound_condition {
        ImpactCompleteness::Bounded
    } else {
        ImpactCompleteness::Complete
    };

    validate_review_context_manifest(
        super::model::ReviewContextManifestInput {
            task_id: input.task_id.to_owned(),
            task_contract_revision: input.task_contract_revision,
            primary_changes,
            related_surfaces,
            regression_areas,
            validation,
            evidence,
            completeness,
            diagnostics,
        },
    )
}

struct AreaDefinition {
    id: &'static str,
    title: &'static str,
}

const REGRESSION_BY_KIND_ORDER: [(
    ImpactRelationKind,
    Option<AreaDefinition>,
); 8] = [
    (
        ImpactRelationKind::ScriptAttachment,
        Some(AreaDefinition {
            id: "REGRESSION.SCRIPT_BEHAVIOR",
            title: "Scene script behavior",
        }),
    ),
    (
        ImpactRelationKind::SceneInheritance,
        Some(AreaDefinition {
            id: "REGRESSION.SCENE_INHERITANCE",
            title: "Scene inheritance",
        }),
    ),
    (
        ImpactRelationKind::SceneInstancing,
        Some(AreaDefinition {
            id: "REGRESSION.SCENE_INSTANTIATION",
            title: "Scene instantiation",
        }),
    ),
    (
        ImpactRelationKind::ResourceDependency,
        Some(AreaDefinition {
            id: "REGRESSION.RESOURCE_LOADING",
            title: "Resource loading",
        }),
    ),
    (
        ImpactRelationKind::ScriptDependency,
        Some(AreaDefinition {
            id: "REGRESSION.SCRIPT_DEPENDENCIES",
            title: "GDScript dependencies",
        }),
    ),
    (
        ImpactRelationKind::SignalConnection,
        Some(AreaDefinition {
            id: "REGRESSION.SIGNAL_CALLBACKS",
            title: "Signal callback behavior",
        }),
    ),
    (
        ImpactRelationKind::AutoloadGlobal,
        Some(AreaDefinition {
            id: "REGRESSION.AUTOLOAD_GLOBAL_REACH",
            title: "Autoload/global state",
        }),
    ),
    (ImpactRelationKind::TestCovers, None),
];

fn build_regression_areas(
    changed_paths: &[String],
    relations: &[ImpactRelation],
    source: &dyn ImpactRelationshipSource,
) -> Vec<ImpactRegressionArea> {
    let mut areas: Vec<ImpactRegressionArea> = Vec::new();
    let mut surfaces_by_kind: HashMap<ImpactRelationKind, Vec<String>> =
        HashMap::new();
    for relation in relations {
        if relation.confidence != ImpactConfidence::Verified {
            continue;
        }
        let surfaces = surfaces_by_kind.entry(relation.kind).or_default();
        if !surfaces.contains(&relation.target_path) {
            surfaces.push(relation.target_path.clone());
        }
    }
    let push = |areas: &mut Vec<ImpactRegressionArea>,
                id: &str,
                title: &str,
                reason: &str,
                surfaces: &[String]| {
        if areas.len() >= ReviewContextLimits::MAX_REGRESSION_AREAS {
            return;
        }
        areas.push(ImpactRegressionArea {
            id: fit_bytes(id, ReviewContextLimits::MAX_EVIDENCE_REF_BYTES),
            title: fit_bytes(title, ReviewContextLimits::MAX_REASON_BYTES),
            reason: fit_bytes(reason, ReviewContextLimits::MAX_REASON_BYTES),
            surfaces: surfaces.iter().take(16).cloned().collect(),
        });
    };
    for (kind, definition) in REGRESSION_BY_KIND_ORDER {
        let Some(definition) = definition else {
            continue;
        };
        let Some(surfaces) = surfaces_by_kind.get(&kind) else {
            continue;
        };
        if surfaces.is_empty() {
            continue;
        }
        let preview: Vec<String> = surfaces.iter().take(4).cloned().collect();
        push(
            &mut areas,
            definition.id,
            definition.title,
            &format!(
                "{} related surface(s) via {}: {}",
                surfaces.len(),
                kind.as_str(),
                preview.join(", ")
            ),
            surfaces,
        );
    }
    if changed_paths.iter().any(|path| path == "project.godot") {
        push(
            &mut areas,
            "REGRESSION.PROJECT_CONFIG",
            "Project configuration",
            "project.godot changed: main scene, autoloads, and input actions may be affected.",
            &["project.godot".to_owned()],
        );
    }
    if let Some(main_scene) = source.main_scene() {
        if changed_paths.iter().any(|path| path == &main_scene) {
            push(
                &mut areas,
                "REGRESSION.MAIN_SCENE",
                "Main scene",
                "The project main scene changed; launch/startup surfaces are directly related.",
                &[main_scene],
            );
        }
    }
    areas
}

fn build_validation_recommendations(
    changed_paths: &[String],
    relations: &[ImpactRelation],
    source: &dyn ImpactRelationshipSource,
    autoload_changed: &[String],
) -> Vec<ImpactValidationRecommendation> {
    struct Entry {
        kind: ValidationKind,
        priority: ValidationPriority,
        rationale: String,
        surfaces: Vec<String>,
        seen: HashSet<String>,
    }
    let mut entries: Vec<Entry> = Vec::new();
    let push = |entries: &mut Vec<Entry>,
                kind: ValidationKind,
                priority: ValidationPriority,
                rationale: &str,
                surfaces: &[String]| {
        let entry = entries
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.priority == priority);
        match entry {
            Some(entry) => {
                entry.rationale = fit_bytes(
                    &format!("{} {rationale}", entry.rationale),
                    ReviewContextLimits::MAX_RATIONALE_BYTES,
                );
            }
            None => entries.push(Entry {
                kind,
                priority,
                rationale: fit_bytes(
                    rationale,
                    ReviewContextLimits::MAX_RATIONALE_BYTES,
                ),
                surfaces: Vec::new(),
                seen: HashSet::new(),
            }),
        }
        let entry = entries
            .iter_mut()
            .find(|entry| entry.kind == kind && entry.priority == priority)
            .expect("entry just inserted");
        for surface in surfaces {
            if entry.seen.insert(surface.clone()) {
                entry.surfaces.push(surface.clone());
            }
        }
    };
    let mut script_surfaces: Vec<String> = Vec::new();
    for path in changed_paths {
        if surface_kind_of(path, source) == ImpactSurfaceKind::Script
            && !script_surfaces.contains(path)
        {
            script_surfaces.push(path.clone());
        }
    }
    for relation in relations {
        if relation.kind == ImpactRelationKind::ScriptAttachment
            && !script_surfaces.contains(&relation.target_path)
        {
            script_surfaces.push(relation.target_path.clone());
        }
    }
    let changed_scene_resource: Vec<String> = changed_paths
        .iter()
        .filter(|path| {
            matches!(
                surface_kind_of(path, source),
                ImpactSurfaceKind::Scene | ImpactSurfaceKind::Resource
            )
        })
        .cloned()
        .collect();
    let mut related_scene_resource: Vec<String> = Vec::new();
    for relation in relations {
        if matches!(
            relation.kind,
            ImpactRelationKind::SceneInheritance
                | ImpactRelationKind::SceneInstancing
                | ImpactRelationKind::ResourceDependency
        ) && matches!(
            surface_kind_of(&relation.target_path, source),
            ImpactSurfaceKind::Scene | ImpactSurfaceKind::Resource
        ) && !related_scene_resource.contains(&relation.target_path)
        {
            related_scene_resource.push(relation.target_path.clone());
        }
    }
    let mut test_surfaces: Vec<String> = Vec::new();
    for relation in relations {
        if relation.kind == ImpactRelationKind::TestCovers
            && !test_surfaces.contains(&relation.target_path)
        {
            test_surfaces.push(relation.target_path.clone());
        }
    }
    let has_signals = relations
        .iter()
        .any(|relation| relation.kind == ImpactRelationKind::SignalConnection);
    if !script_surfaces.is_empty() {
        push(
            &mut entries,
            ValidationKind::GdscriptCheckOnly,
            ValidationPriority::RequiredNow,
            &format!(
                "{} script surface(s) changed or attached; check-only parse is required \
                 before any mutation is applied.",
                script_surfaces.len()
            ),
            &script_surfaces,
        );
        push(
            &mut entries,
            ValidationKind::FreshLspDiagnostics,
            ValidationPriority::Recommended,
            "Script changes warrant fresh language-server diagnostics after application.",
            &script_surfaces,
        );
    }
    if !changed_scene_resource.is_empty() {
        push(
            &mut entries,
            ValidationKind::SceneResourceParse,
            ValidationPriority::RequiredNow,
            &format!(
                "{} scene/resource surface(s) changed; reparse validation is required.",
                changed_scene_resource.len()
            ),
            &changed_scene_resource,
        );
    }
    if !related_scene_resource.is_empty() {
        push(
            &mut entries,
            ValidationKind::SceneResourceParse,
            ValidationPriority::Recommended,
            &format!(
                "{} related scene/resource surface(s) should reparse after the change.",
                related_scene_resource.len()
            ),
            &related_scene_resource,
        );
    }
    if changed_paths.iter().any(|path| path == "project.godot") {
        push(
            &mut entries,
            ValidationKind::ProjectConfigChecks,
            ValidationPriority::RequiredNow,
            "project.godot changed; main scene, autoload, and input-action structure must revalidate.",
            &["project.godot".to_owned()],
        );
    }
    if !autoload_changed.is_empty() {
        let autoload_list: Vec<String> =
            autoload_changed.iter().take(8).cloned().collect();
        push(
            &mut entries,
            ValidationKind::BroaderRepoValidation,
            ValidationPriority::Recommended,
            &format!(
                "Changed autoload(s) {} have global reach; broader repository validation is \
                 recommended (impact beyond direct relations is candidate).",
                autoload_list.join(", ")
            ),
            &autoload_changed[..autoload_changed
                .len()
                .min(ReviewContextLimits::MAX_RELATED_SURFACES)],
        );
    }
    if !test_surfaces.is_empty() {
        push(
            &mut entries,
            ValidationKind::SpecificTestScript,
            ValidationPriority::Recommended,
            &format!(
                "{} candidate test surface(s) identified by convention; run them to confirm \
                 coverage (candidate, not verified).",
                test_surfaces.len()
            ),
            &test_surfaces,
        );
    }
    if has_signals || !autoload_changed.is_empty() {
        push(
            &mut entries,
            ValidationKind::RuntimeValidation,
            ValidationPriority::RuntimeEvidenceUnavailable,
            "Signal callbacks and autoload reach cannot be proven statically; runtime \
             validation is required when runtime evidence becomes available.",
            &[],
        );
    }
    let mut result: Vec<ImpactValidationRecommendation> = entries
        .into_iter()
        .map(|entry| ImpactValidationRecommendation {
            kind: entry.kind,
            priority: entry.priority,
            rationale: entry.rationale,
            surfaces: entry
                .surfaces
                .into_iter()
                .take(ReviewContextLimits::MAX_RELATED_SURFACES)
                .collect(),
        })
        .collect();
    result.truncate(ReviewContextLimits::MAX_VALIDATION);
    result
}

fn build_evidence_refs(relations: &[ImpactRelation]) -> Vec<String> {
    let mut refs = OrderedStrings::default();
    refs.insert("impact:changed-surface".to_owned());
    for relation in relations {
        refs.insert(relation.evidence.clone());
    }
    refs.entries.into_iter().take(ReviewContextLimits::MAX_EVIDENCE).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        AnalyzeImpactInput, ImpactEdge, ImpactRelationshipSource,
        ImpactSignalConnection, analyze_impact,
    };
    use crate::godot::impact::model::{
        ImpactCompleteness, ImpactRelationKind, ImpactSurfaceKind,
        ValidationKind, ValidationPriority,
    };
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeSource {
        edges: HashMap<String, Vec<ImpactEdge>>,
        signals: HashMap<String, Vec<ImpactSignalConnection>>,
        autoloads: HashMap<String, String>,
        tests: HashMap<String, Vec<String>>,
        main_scene: Option<String>,
        revisions: HashMap<String, String>,
    }

    impl FakeSource {
        fn edge(kind: ImpactRelationKind, from: &str, to: &str) -> ImpactEdge {
            ImpactEdge {
                kind,
                from_path: from.to_owned(),
                to_path: to.to_owned(),
                stale: false,
            }
        }

        fn with_edges(mut self, path: &str, edges: Vec<ImpactEdge>) -> Self {
            self.edges.insert(path.to_owned(), edges);
            self
        }

        fn with_signals(
            mut self,
            path: &str,
            connections: Vec<ImpactSignalConnection>,
        ) -> Self {
            self.signals.insert(path.to_owned(), connections);
            self
        }

        fn with_tests(mut self, path: &str, tests: Vec<String>) -> Self {
            self.tests.insert(path.to_owned(), tests);
            self
        }
    }

    impl ImpactRelationshipSource for FakeSource {
        fn outgoing(&self, path: &str) -> Vec<ImpactEdge> {
            self.edges.get(path).cloned().unwrap_or_default()
        }

        fn incoming(&self, path: &str) -> Vec<ImpactEdge> {
            self.edges
                .values()
                .flatten()
                .filter(|edge| edge.to_path == path)
                .cloned()
                .collect()
        }

        fn signal_connections(
            &self,
            path: &str,
        ) -> Vec<ImpactSignalConnection> {
            self.signals.get(path).cloned().unwrap_or_default()
        }

        fn autoload_name(&self, path: &str) -> Option<String> {
            self.autoloads.get(path).cloned()
        }

        fn main_scene(&self) -> Option<String> {
            self.main_scene.clone()
        }

        fn current_revision(&self, path: &str) -> Option<String> {
            self.revisions.get(path).cloned()
        }

        fn candidate_tests(&self, path: &str) -> Vec<String> {
            self.tests.get(path).cloned().unwrap_or_default()
        }
    }

    fn run(
        source: &FakeSource,
        changed: &[&str],
    ) -> crate::godot::impact::model::ReviewContextManifest {
        let owned: Vec<String> =
            changed.iter().map(|path| (*path).to_owned()).collect();
        analyze_impact(AnalyzeImpactInput {
            task_id: "task-1",
            task_contract_revision: 3,
            changed_paths: &owned,
            source,
        })
        .expect("analysis succeeds")
    }

    #[test]
    fn verified_relations_are_recorded_once_with_evidence_refs() {
        let source = FakeSource::default()
            .with_edges(
                "res://player.gd",
                vec![FakeSource::edge(
                    ImpactRelationKind::ScriptAttachment,
                    "res://player.gd",
                    "res://player.tscn",
                )],
            )
            .with_edges(
                "res://player.tscn",
                vec![FakeSource::edge(
                    ImpactRelationKind::ScriptAttachment,
                    "res://player.gd",
                    "res://player.tscn",
                )],
            );
        let manifest = run(&source, &["res://player.gd"]);
        assert_eq!(manifest.task_id, "task-1");
        assert_eq!(manifest.primary_changes.len(), 1);
        assert_eq!(
            manifest.primary_changes[0].kind,
            ImpactSurfaceKind::Script
        );
        assert_eq!(
            manifest.primary_changes[0].evidence,
            "impact:changed-surface"
        );
        // The reverse traversal must not duplicate the relationship.
        assert_eq!(manifest.related_surfaces.len(), 1);
        assert_eq!(
            manifest.related_surfaces[0].target_path,
            "res://player.tscn"
        );
        assert_eq!(
            manifest.related_surfaces[0].evidence,
            "index:script_attachment"
        );
        assert_eq!(
            manifest.evidence,
            vec!["impact:changed-surface", "index:script_attachment"]
        );
        assert_eq!(manifest.completeness, ImpactCompleteness::Complete);
        assert!(manifest.diagnostics.is_empty());
    }

    #[test]
    fn stale_relationships_are_excluded_and_disclosed() {
        let mut stale = FakeSource::edge(
            ImpactRelationKind::ScriptDependency,
            "res://a.gd",
            "res://b.gd",
        );
        stale.stale = true;
        let source =
            FakeSource::default().with_edges("res://a.gd", vec![stale]);
        let manifest = run(&source, &["res://a.gd"]);
        assert!(manifest.related_surfaces.is_empty());
        assert_eq!(manifest.diagnostics.len(), 1);
        assert_eq!(manifest.diagnostics[0].code, "IMPACT.STALE_RELATIONSHIP");
        assert_eq!(
            manifest.diagnostics[0].message,
            "Relationship res://a.gd->res://b.gd is stale (its recorded source revision is no longer current); it was excluded from current impact."
        );
        assert_eq!(manifest.completeness, ImpactCompleteness::Partial);
    }

    #[test]
    fn depth_bound_is_reported_for_deep_chains() {
        let source = FakeSource::default()
            .with_edges(
                "res://a.gd",
                vec![FakeSource::edge(
                    ImpactRelationKind::ScriptDependency,
                    "res://a.gd",
                    "res://b.gd",
                )],
            )
            .with_edges(
                "res://b.gd",
                vec![FakeSource::edge(
                    ImpactRelationKind::ScriptDependency,
                    "res://b.gd",
                    "res://c.gd",
                )],
            )
            .with_edges(
                "res://c.gd",
                vec![FakeSource::edge(
                    ImpactRelationKind::ScriptDependency,
                    "res://c.gd",
                    "res://d.gd",
                )],
            );
        let manifest = run(&source, &["res://a.gd"]);
        assert!(
            manifest
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "IMPACT.TRAVERSAL_BOUND"
                    && diagnostic.message == "depth bound reached")
        );
        assert_eq!(manifest.completeness, ImpactCompleteness::Bounded);
    }

    #[test]
    fn scene_signal_connections_carry_serialized_notes() {
        let source = FakeSource::default().with_signals(
            "res://main.tscn",
            vec![ImpactSignalConnection {
                signal: "pressed".to_owned(),
                source_node: "Button".to_owned(),
                target_node: "Root".to_owned(),
                target_method: "_on_pressed".to_owned(),
            }],
        );
        let manifest = run(&source, &["res://main.tscn"]);
        let connection = manifest
            .related_surfaces
            .iter()
            .find(|relation| {
                relation.kind == ImpactRelationKind::SignalConnection
            })
            .expect("signal relation recorded");
        assert_eq!(connection.source_path, "res://main.tscn");
        assert_eq!(
            connection.note.as_deref(),
            Some(
                "serialized connection pressed: node Button -> node Root._on_pressed"
            )
        );
        assert_eq!(manifest.primary_changes[0].kind, ImpactSurfaceKind::Scene);
    }

    #[test]
    fn candidate_tests_stay_candidate_and_drive_partial_completeness() {
        let source = FakeSource::default().with_tests(
            "res://player.gd",
            vec![
                "res://tests/player_test.gd".to_owned(),
                "res://tests/player_test.gd".to_owned(),
                "res://player.gd".to_owned(),
            ],
        );
        let manifest = run(&source, &["res://player.gd"]);
        let coverage = manifest
            .related_surfaces
            .iter()
            .find(|relation| relation.kind == ImpactRelationKind::TestCovers)
            .expect("candidate test recorded");
        assert_eq!(coverage.target_path, "res://tests/player_test.gd");
        assert_eq!(
            coverage.confidence,
            crate::godot::impact::model::ImpactConfidence::Candidate
        );
        assert_eq!(manifest.regression_areas.len(), 0);
        let specific = manifest
            .validation
            .iter()
            .find(|recommendation| {
                recommendation.kind == ValidationKind::SpecificTestScript
            })
            .expect("test recommendation");
        assert_eq!(specific.priority, ValidationPriority::Recommended);
        assert_eq!(manifest.completeness, ImpactCompleteness::Partial);
    }

    #[test]
    fn autoloads_disclose_global_reach_and_runtime_gap() {
        let mut source = FakeSource::default();
        source
            .autoloads
            .insert("res://globals/state.gd".to_owned(), "State".to_owned());
        let manifest = run(&source, &["res://globals/state.gd"]);
        assert_eq!(
            manifest.primary_changes[0].kind,
            ImpactSurfaceKind::Autoload
        );
        assert_eq!(
            manifest.primary_changes[0].note.as_deref(),
            Some("project autoload: State")
        );
        assert!(
            manifest
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "IMPACT.AUTOLOAD_GLOBAL")
        );
        let runtime = manifest
            .validation
            .iter()
            .find(|recommendation| {
                recommendation.kind == ValidationKind::RuntimeValidation
            })
            .expect("runtime recommendation");
        assert_eq!(
            runtime.priority,
            ValidationPriority::RuntimeEvidenceUnavailable
        );
        assert!(runtime.surfaces.is_empty());
        assert_eq!(manifest.completeness, ImpactCompleteness::Partial);
    }

    #[test]
    fn project_config_and_main_scene_get_dedicated_areas() {
        let source = FakeSource {
            main_scene: Some("res://main.tscn".to_owned()),
            ..FakeSource::default()
        };
        let manifest = run(&source, &["project.godot", "res://main.tscn"]);
        let ids: Vec<&str> = manifest
            .regression_areas
            .iter()
            .map(|area| area.id.as_str())
            .collect();
        assert!(ids.contains(&"REGRESSION.PROJECT_CONFIG"));
        assert!(ids.contains(&"REGRESSION.MAIN_SCENE"));
        assert!(
            manifest
                .validation
                .iter()
                .any(|recommendation| recommendation.kind
                    == ValidationKind::ProjectConfigChecks)
        );
    }

    #[test]
    fn identical_inputs_produce_identical_manifests() {
        let source = FakeSource::default().with_edges(
            "res://a.gd",
            vec![FakeSource::edge(
                ImpactRelationKind::ScriptAttachment,
                "res://a.gd",
                "res://a.tscn",
            )],
        );
        let first = run(&source, &["res://a.gd"]);
        let second = run(&source, &["res://a.gd"]);
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_changed_paths_are_filtered_before_analysis() {
        let source = FakeSource::default();
        let manifest = run(&source, &["/abs/path", "..\\escape", "", "ok.gd"]);
        assert_eq!(manifest.primary_changes.len(), 1);
        assert_eq!(manifest.primary_changes[0].path, "ok.gd");
    }
}
