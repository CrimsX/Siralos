//! Review-context manifest model and boundary validator.
//!
//! Mirrors `packages/core/src/godot/impact/review-context.ts`: the same
//! limits, the same validation order, and the same bounded, truthful
//! messages. Only the normalized shape produced by the impact analyzer
//! (or an equally disciplined caller) should reach this boundary; the
//! output is detached from the input.

use std::fmt;

/// Host-owned hard bounds for review-context manifests (never raised by
/// input). Mirrors `REVIEW_CONTEXT_LIMITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewContextLimits;

impl ReviewContextLimits {
    /// Maximum primary changed surfaces.
    pub const MAX_PRIMARY_CHANGES: usize = 16;
    /// Maximum related-surface relations.
    pub const MAX_RELATED_SURFACES: usize = 64;
    /// Maximum regression areas.
    pub const MAX_REGRESSION_AREAS: usize = 8;
    /// Maximum validation recommendations.
    pub const MAX_VALIDATION: usize = 12;
    /// Maximum evidence references.
    pub const MAX_EVIDENCE: usize = 32;
    /// Maximum diagnostics.
    pub const MAX_DIAGNOSTICS: usize = 16;
    /// Default traversal depth from the primary surfaces.
    pub const MAX_DEPTH: usize = 2;
    /// Default visited-surface bound (cycle-safe, breadth-first).
    pub const MAX_SURFACES_VISITED: usize = 64;
    /// Default visited-relation bound.
    pub const MAX_RELATIONS_VISITED: usize = 128;
    /// Candidate test surfaces per primary change (global cap).
    pub const MAX_CANDIDATE_TESTS: usize = 8;
    /// Maximum path bytes.
    pub const MAX_PATH_BYTES: usize = 1024;
    /// Maximum evidence-reference bytes.
    pub const MAX_EVIDENCE_REF_BYTES: usize = 256;
    /// Maximum note bytes.
    pub const MAX_NOTE_BYTES: usize = 512;
    /// Maximum regression-area reason/title bytes.
    pub const MAX_REASON_BYTES: usize = 512;
    /// Maximum validation rationale bytes.
    pub const MAX_RATIONALE_BYTES: usize = 512;
}

/// Validation failure at the review-context boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewContextError {
    /// Bounded truthful message (mirrors the oracle strings).
    pub message: String,
}

impl fmt::Display for ReviewContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ReviewContextError {}

fn error(message: impl Into<String>) -> ReviewContextError {
    ReviewContextError { message: message.into() }
}

/// Kind of one impacted surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImpactSurfaceKind {
    /// GDScript or other script file.
    Script,
    /// `.tscn` scene.
    Scene,
    /// `.tres`/`.theme` resource.
    Resource,
    /// Project autoload target.
    Autoload,
    /// Signal endpoint surface.
    SignalEndpoint,
    /// Test script surface.
    Test,
    /// `project.godot` configuration.
    ProjectConfig,
}

impl ImpactSurfaceKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Script => "script",
            Self::Scene => "scene",
            Self::Resource => "resource",
            Self::Autoload => "autoload",
            Self::SignalEndpoint => "signal-endpoint",
            Self::Test => "test",
            Self::ProjectConfig => "project-config",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "script" => Ok(Self::Script),
            "scene" => Ok(Self::Scene),
            "resource" => Ok(Self::Resource),
            "autoload" => Ok(Self::Autoload),
            "signal-endpoint" => Ok(Self::SignalEndpoint),
            "test" => Ok(Self::Test),
            "project-config" => Ok(Self::ProjectConfig),
            other => {
                Err(error(format!("Invalid impact surface kind: {other}")))
            }
        }
    }
}

/// Verified impact is evidence-backed; candidate impact is plausible but
/// unproven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactConfidence {
    /// Evidence-backed.
    Verified,
    /// Plausible but unproven.
    Candidate,
}

impl ImpactConfidence {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Candidate => "candidate",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "verified" => Ok(Self::Verified),
            "candidate" => Ok(Self::Candidate),
            other => Err(error(format!("Invalid impact confidence: {other}"))),
        }
    }
}

/// Kind of one derived relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImpactRelationKind {
    /// Script attached to a scene node.
    ScriptAttachment,
    /// Scene inheritance edge.
    SceneInheritance,
    /// Scene instancing edge.
    SceneInstancing,
    /// Resource dependency edge.
    ResourceDependency,
    /// Script-to-script dependency.
    ScriptDependency,
    /// Serialized signal connection.
    SignalConnection,
    /// Autoload/global reference.
    AutoloadGlobal,
    /// Candidate test coverage (convention heuristic).
    TestCovers,
}

impl ImpactRelationKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScriptAttachment => "script_attachment",
            Self::SceneInheritance => "scene_inheritance",
            Self::SceneInstancing => "scene_instancing",
            Self::ResourceDependency => "resource_dependency",
            Self::ScriptDependency => "script_dependency",
            Self::SignalConnection => "signal_connection",
            Self::AutoloadGlobal => "autoload_global",
            Self::TestCovers => "test_covers",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "script_attachment" => Ok(Self::ScriptAttachment),
            "scene_inheritance" => Ok(Self::SceneInheritance),
            "scene_instancing" => Ok(Self::SceneInstancing),
            "resource_dependency" => Ok(Self::ResourceDependency),
            "script_dependency" => Ok(Self::ScriptDependency),
            "signal_connection" => Ok(Self::SignalConnection),
            "autoload_global" => Ok(Self::AutoloadGlobal),
            "test_covers" => Ok(Self::TestCovers),
            other => {
                Err(error(format!("Invalid impact relation kind: {other}")))
            }
        }
    }
}

/// Honest completeness classification of one manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactCompleteness {
    /// All reachable relations within bounds were included.
    Complete,
    /// A traversal bound truncated derivation.
    Bounded,
    /// Candidate/stale/autoload conditions make some impact unproven.
    Partial,
}

impl ImpactCompleteness {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Bounded => "bounded",
            Self::Partial => "partial",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "complete" => Ok(Self::Complete),
            "bounded" => Ok(Self::Bounded),
            "partial" => Ok(Self::Partial),
            other => Err(error(format!("Invalid completeness: {other}"))),
        }
    }
}

/// Priority of one validation recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationPriority {
    /// Must run before any mutation is applied.
    RequiredNow,
    /// Should run after application.
    Recommended,
    /// Cannot be satisfied until runtime evidence exists.
    RuntimeEvidenceUnavailable,
}

impl ValidationPriority {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequiredNow => "required_now",
            Self::Recommended => "recommended",
            Self::RuntimeEvidenceUnavailable => "runtime_evidence_unavailable",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "required_now" => Ok(Self::RequiredNow),
            "recommended" => Ok(Self::Recommended),
            "runtime_evidence_unavailable" => {
                Ok(Self::RuntimeEvidenceUnavailable)
            }
            other => {
                Err(error(format!("Invalid validation priority: {other}")))
            }
        }
    }
}

/// Kind of one structured validation recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationKind {
    /// Bounded check-only GDScript parse.
    GdscriptCheckOnly,
    /// Fresh language-server diagnostics after application.
    FreshLspDiagnostics,
    /// One identified test script.
    SpecificTestScript,
    /// Scene/resource reparse.
    SceneResourceParse,
    /// `project.godot` structure checks.
    ProjectConfigChecks,
    /// Broader repository validation.
    BroaderRepoValidation,
    /// Runtime validation (evidence unavailable at this stage).
    RuntimeValidation,
}

impl ValidationKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GdscriptCheckOnly => "gdscript_check_only",
            Self::FreshLspDiagnostics => "fresh_lsp_diagnostics",
            Self::SpecificTestScript => "specific_test_script",
            Self::SceneResourceParse => "scene_resource_parse",
            Self::ProjectConfigChecks => "project_config_checks",
            Self::BroaderRepoValidation => "broader_repo_validation",
            Self::RuntimeValidation => "runtime_validation",
        }
    }

    /// Parse a protocol string; mirrors the oracle rejection messages.
    pub fn parse(value: &str) -> Result<Self, ReviewContextError> {
        match value {
            "gdscript_check_only" => Ok(Self::GdscriptCheckOnly),
            "fresh_lsp_diagnostics" => Ok(Self::FreshLspDiagnostics),
            "specific_test_script" => Ok(Self::SpecificTestScript),
            "scene_resource_parse" => Ok(Self::SceneResourceParse),
            "project_config_checks" => Ok(Self::ProjectConfigChecks),
            "broader_repo_validation" => Ok(Self::BroaderRepoValidation),
            "runtime_validation" => Ok(Self::RuntimeValidation),
            other => Err(error(format!("Invalid validation kind: {other}"))),
        }
    }
}

/// One changed surface with its exact revision and confidence. Text
/// fields may carry untrimmed input pre-validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactSurface {
    /// Workspace-relative path.
    pub path: String,
    /// Surface kind.
    pub kind: ImpactSurfaceKind,
    /// Exact workspace revision of the inspected state, when known.
    pub revision: Option<String>,
    /// Evidence confidence.
    pub confidence: ImpactConfidence,
    /// Bounded evidence reference in `kind:ref` form.
    pub evidence: String,
    /// Optional bounded note.
    pub note: Option<String>,
}

/// One derived relationship between a changed surface and a related
/// surface. Text fields may carry untrimmed input pre-validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRelation {
    /// Relationship kind.
    pub kind: ImpactRelationKind,
    /// The changed surface (source of the impact).
    pub source_path: String,
    /// The related surface (potentially impacted).
    pub target_path: String,
    /// Source revision when known.
    pub source_revision: Option<String>,
    /// Target revision when known.
    pub target_revision: Option<String>,
    /// Evidence confidence.
    pub confidence: ImpactConfidence,
    /// Bounded evidence reference.
    pub evidence: String,
    /// Optional bounded note.
    pub note: Option<String>,
}

/// One evidence-backed regression area (never generic boilerplate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRegressionArea {
    /// Stable area id.
    pub id: String,
    /// Area title.
    pub title: String,
    /// Why this area is relevant, tied to observed relations.
    pub reason: String,
    /// Bounded related surface paths backing the area.
    pub surfaces: Vec<String>,
}

/// One structured validation recommendation derived from observed
/// impact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactValidationRecommendation {
    /// Recommendation kind.
    pub kind: ValidationKind,
    /// Priority.
    pub priority: ValidationPriority,
    /// Bounded rationale.
    pub rationale: String,
    /// Surfaces the recommendation applies to (bounded).
    pub surfaces: Vec<String>,
}

/// Honest limitation/uncertainty disclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactDiagnostic {
    /// Stable code, e.g. `IMPACT.TRAVERSAL_BOUND`.
    pub code: String,
    /// Bounded truthful message.
    pub message: String,
}

/// Immutable derived review/validation context for one task. Revision-
/// and evidence-bound; never task authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewContextManifest {
    /// Owning task id.
    pub task_id: String,
    /// Task contract revision the analysis binds to.
    pub task_contract_revision: u64,
    /// Primary changed surfaces.
    pub primary_changes: Vec<ImpactSurface>,
    /// Derived relationships to potentially impacted surfaces.
    pub related_surfaces: Vec<ImpactRelation>,
    /// Evidence-backed regression areas.
    pub regression_areas: Vec<ImpactRegressionArea>,
    /// Structured validation recommendations.
    pub validation: Vec<ImpactValidationRecommendation>,
    /// Bounded evidence references backing the manifest.
    pub evidence: Vec<String>,
    /// Honest completeness classification.
    pub completeness: ImpactCompleteness,
    /// Honest limitation disclosures.
    pub diagnostics: Vec<ImpactDiagnostic>,
}

/// Raw manifest parts accepted by the boundary validator. Fields mirror
/// [`ReviewContextManifest`] but text may be untrimmed/unbounded; the
/// validator produces the detached normalized manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewContextManifestInput {
    /// Owning task id (untrimmed).
    pub task_id: String,
    /// Positive contract revision.
    pub task_contract_revision: u64,
    /// Primary changes.
    pub primary_changes: Vec<ImpactSurface>,
    /// Related surfaces.
    pub related_surfaces: Vec<ImpactRelation>,
    /// Regression areas.
    pub regression_areas: Vec<ImpactRegressionArea>,
    /// Validation recommendations.
    pub validation: Vec<ImpactValidationRecommendation>,
    /// Evidence references.
    pub evidence: Vec<String>,
    /// Completeness classification.
    pub completeness: ImpactCompleteness,
    /// Diagnostics.
    pub diagnostics: Vec<ImpactDiagnostic>,
}

fn require_bounded_text(
    value: &str,
    max_bytes: usize,
    field: &str,
) -> Result<String, ReviewContextError> {
    let text = value.trim();
    if text.is_empty() {
        return Err(error(format!("{field} must not be empty.")));
    }
    if text.len() > max_bytes {
        return Err(error(format!(
            "{field} exceeds {max_bytes} UTF-8 bytes."
        )));
    }
    Ok(text.to_owned())
}

fn optional_bounded_text(
    value: Option<&str>,
    max_bytes: usize,
    field: &str,
) -> Result<Option<String>, ReviewContextError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.trim();
    if text.is_empty() {
        return Err(error(format!(
            "{field} must not be empty when provided."
        )));
    }
    if text.len() > max_bytes {
        return Err(error(format!(
            "{field} exceeds {max_bytes} UTF-8 bytes."
        )));
    }
    Ok(Some(text.to_owned()))
}

fn validate_path(
    path: &str,
    field: &str,
) -> Result<String, ReviewContextError> {
    let text = require_bounded_text(
        path,
        ReviewContextLimits::MAX_PATH_BYTES,
        field,
    )?;
    let drive_prefix = text.len() >= 2
        && text.as_bytes()[1] == b':'
        && text.as_bytes()[0].is_ascii_alphabetic();
    if text.contains('\\')
        || text.starts_with('/')
        || drive_prefix
        || text.contains('\0')
    {
        return Err(error(format!(
            "{field} must be a workspace-relative path: {text}"
        )));
    }
    if text.split('/').any(|segment| segment == "..") {
        return Err(error(format!(
            "{field} must not traverse parents: {text}"
        )));
    }
    Ok(text)
}

fn copy_bounded_strings(
    values: &[String],
    max: usize,
    max_bytes: usize,
    field: &str,
) -> Result<Vec<String>, ReviewContextError> {
    if values.len() > max {
        return Err(error(format!("{field} accepts at most {max} entries.")));
    }
    values
        .iter()
        .map(|value| {
            require_bounded_text(value, max_bytes, &format!("{field} entry"))
        })
        .collect()
}

/// Validate and detach a review-context manifest at a runtime boundary:
/// only the normalized shape produced by the impact analyzer can become
/// authoritative. Mirrors the oracle's validation order and messages.
pub fn validate_review_context_manifest(
    input: ReviewContextManifestInput,
) -> Result<ReviewContextManifest, ReviewContextError> {
    let task_id = require_bounded_text(
        &input.task_id,
        ReviewContextLimits::MAX_PATH_BYTES,
        "A task id",
    )?;
    if input.task_contract_revision < 1 {
        return Err(error(
            "A review context requires a positive safe-integer task contract revision.",
        ));
    }
    let mut primary_changes = Vec::with_capacity(input.primary_changes.len());
    for (index, surface) in input.primary_changes.iter().enumerate() {
        let path =
            validate_path(&surface.path, &format!("Primary surface {index}"))?;
        primary_changes.push(ImpactSurface {
            path,
            kind: surface.kind,
            revision: surface.revision.clone(),
            confidence: surface.confidence,
            evidence: require_bounded_text(
                &surface.evidence,
                ReviewContextLimits::MAX_EVIDENCE_REF_BYTES,
                &format!("Evidence for {}", surface.path.trim()),
            )?,
            note: optional_bounded_text(
                surface.note.as_deref(),
                ReviewContextLimits::MAX_NOTE_BYTES,
                &format!("Note for {}", surface.path.trim()),
            )?,
        });
    }
    if primary_changes.len() > ReviewContextLimits::MAX_PRIMARY_CHANGES {
        return Err(error(format!(
            "A review context accepts at most {} primary changes.",
            ReviewContextLimits::MAX_PRIMARY_CHANGES
        )));
    }
    let mut related_surfaces =
        Vec::with_capacity(input.related_surfaces.len());
    for (index, relation) in input.related_surfaces.iter().enumerate() {
        related_surfaces.push(ImpactRelation {
            kind: relation.kind,
            source_path: validate_path(
                &relation.source_path,
                &format!("Relation {index} source"),
            )?,
            target_path: validate_path(
                &relation.target_path,
                &format!("Relation {index} target"),
            )?,
            source_revision: relation.source_revision.clone(),
            target_revision: relation.target_revision.clone(),
            confidence: relation.confidence,
            evidence: require_bounded_text(
                &relation.evidence,
                ReviewContextLimits::MAX_EVIDENCE_REF_BYTES,
                &format!("Relation {index} evidence"),
            )?,
            note: optional_bounded_text(
                relation.note.as_deref(),
                ReviewContextLimits::MAX_NOTE_BYTES,
                &format!("Relation {index} note"),
            )?,
        });
    }
    if related_surfaces.len() > ReviewContextLimits::MAX_RELATED_SURFACES {
        return Err(error(format!(
            "A review context accepts at most {} related surfaces.",
            ReviewContextLimits::MAX_RELATED_SURFACES
        )));
    }
    let mut regression_areas =
        Vec::with_capacity(input.regression_areas.len());
    for area in &input.regression_areas {
        regression_areas.push(ImpactRegressionArea {
            id: require_bounded_text(
                &area.id,
                ReviewContextLimits::MAX_EVIDENCE_REF_BYTES,
                "A regression area id",
            )?,
            title: require_bounded_text(
                &area.title,
                ReviewContextLimits::MAX_REASON_BYTES,
                "A regression area title",
            )?,
            reason: require_bounded_text(
                &area.reason,
                ReviewContextLimits::MAX_REASON_BYTES,
                "A regression area reason",
            )?,
            surfaces: copy_bounded_strings(
                &area.surfaces,
                ReviewContextLimits::MAX_RELATED_SURFACES,
                ReviewContextLimits::MAX_PATH_BYTES,
                "A regression area surface",
            )?,
        });
    }
    if regression_areas.len() > ReviewContextLimits::MAX_REGRESSION_AREAS {
        return Err(error(format!(
            "A review context accepts at most {} regression areas.",
            ReviewContextLimits::MAX_REGRESSION_AREAS
        )));
    }
    let mut validation = Vec::with_capacity(input.validation.len());
    for recommendation in &input.validation {
        validation.push(ImpactValidationRecommendation {
            kind: recommendation.kind,
            priority: recommendation.priority,
            rationale: require_bounded_text(
                &recommendation.rationale,
                ReviewContextLimits::MAX_RATIONALE_BYTES,
                "A validation rationale",
            )?,
            surfaces: copy_bounded_strings(
                &recommendation.surfaces,
                ReviewContextLimits::MAX_RELATED_SURFACES,
                ReviewContextLimits::MAX_PATH_BYTES,
                "A validation surface",
            )?,
        });
    }
    if validation.len() > ReviewContextLimits::MAX_VALIDATION {
        return Err(error(format!(
            "A review context accepts at most {} validation recommendations.",
            ReviewContextLimits::MAX_VALIDATION
        )));
    }
    let evidence = copy_bounded_strings(
        &input.evidence,
        ReviewContextLimits::MAX_EVIDENCE,
        ReviewContextLimits::MAX_EVIDENCE_REF_BYTES,
        "Evidence",
    )?;
    let mut diagnostics = Vec::with_capacity(input.diagnostics.len());
    for diagnostic in &input.diagnostics {
        diagnostics.push(ImpactDiagnostic {
            code: require_bounded_text(
                &diagnostic.code,
                ReviewContextLimits::MAX_EVIDENCE_REF_BYTES,
                "A diagnostic code",
            )?,
            message: require_bounded_text(
                &diagnostic.message,
                ReviewContextLimits::MAX_REASON_BYTES,
                "A diagnostic message",
            )?,
        });
    }
    if diagnostics.len() > ReviewContextLimits::MAX_DIAGNOSTICS {
        return Err(error(format!(
            "A review context accepts at most {} diagnostics.",
            ReviewContextLimits::MAX_DIAGNOSTICS
        )));
    }
    Ok(ReviewContextManifest {
        task_id,
        task_contract_revision: input.task_contract_revision,
        primary_changes,
        related_surfaces,
        regression_areas,
        validation,
        evidence,
        completeness: input.completeness,
        diagnostics,
    })
}
