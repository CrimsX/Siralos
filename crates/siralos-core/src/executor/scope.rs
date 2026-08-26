//! WorkspaceScope and ActiveWorkingSet (harness context optimization,
//! ADR 0023; Stage 3R R13.4).
//!
//! A task's derived execution scope is distinct from the current step's
//! working set. `WorkspaceScope` is DERIVED execution scope, never a
//! security authority: verified files carry evidence and were actually
//! inspected; candidate files are merely potentially relevant. Source-
//! context budgets control CONTEXT, not repository access, and eviction
//! demotes exact views to summaries while retaining revision identity
//! and evidence references — authoritative evidence is never deleted.

/// Source-file confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFileConfidence {
    /// Inspected with evidence.
    Verified,
    /// Potentially relevant.
    Candidate,
}

impl SourceFileConfidence {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceFileConfidence::Verified => "verified",
            SourceFileConfidence::Candidate => "candidate",
        }
    }
}

/// Which representation of a file currently occupies source context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceView {
    /// Exact bytes in context.
    Exact,
    /// Structural representation.
    Structural,
    /// Summary representation.
    Summary,
    /// Nothing in context.
    None,
}

impl SourceView {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceView::Exact => "exact",
            SourceView::Structural => "structural",
            SourceView::Summary => "summary",
            SourceView::None => "none",
        }
    }
}

/// One scoped source-file reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileRef {
    /// Workspace-relative path.
    pub path: String,
    /// Confidence.
    pub confidence: SourceFileConfidence,
    /// Current source view.
    pub view: SourceView,
    /// Required `rev_` handle for verified files.
    pub revision: Option<String>,
    /// Required evidence reference for verified files.
    pub evidence: Option<String>,
    /// Bounded reason the file entered the verified set.
    pub reason: Option<String>,
}

/// Recorded candidate-to-verified promotion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePromotionRecord {
    /// Promoted path.
    pub path: String,
    /// The evidence that justified promotion (required).
    pub evidence: String,
    /// Exact inspected revision handle (required).
    pub revision: String,
    /// Bounded relevance reason.
    pub reason: String,
}

/// Source-context budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceContextBudget {
    /// Maximum exact source files in model context at once.
    pub max_active_exact_files: usize,
    /// Maximum total exact source bytes.
    pub max_exact_bytes: usize,
    /// Maximum structural/summary representations retained.
    pub max_structural_summaries: usize,
    /// Maximum candidate files tracked.
    pub max_candidate_files: usize,
    /// Maximum retained historical views.
    pub max_retained_historical_views: usize,
}

/// Default conservative budget (host-owned).
pub const DEFAULT_WORKSPACE_CONTEXT_BUDGET: WorkspaceContextBudget =
    WorkspaceContextBudget {
        max_active_exact_files: 4,
        max_exact_bytes: 32 * 1024,
        max_structural_summaries: 12,
        max_candidate_files: 16,
        max_retained_historical_views: 4,
    };

/// Default source-context exclusions: noisy generated/vendor paths are
/// suppressed from default discovery. Context exclusion is NOT security
/// denial — these paths may still be read when the task needs them.
pub const DEFAULT_SOURCE_EXCLUSIONS: &[&str] = &[
    "node_modules/",
    "dist/",
    "build/",
    "coverage/",
    ".git/",
    ".godot/",
    "generated/",
    "out/",
];

/// Whether a workspace-relative path is excluded from default discovery.
/// Matches path prefixes so `node_modules/foo` is excluded by
/// `node_modules/`. Deterministic; never a security denial.
pub fn is_excluded_source_path(path: &str, exclusions: &[&str]) -> bool {
    let normalized = path.strip_prefix("./").unwrap_or(path);
    exclusions.iter().any(|exclusion| {
        let prefix = exclusion.strip_prefix("./").unwrap_or(exclusion);
        let prefixed = format!("{prefix}/");
        let with_slash =
            if prefix.ends_with('/') { prefix } else { prefixed.as_str() };
        normalized == prefix || normalized.starts_with(with_slash)
    })
}

/// Derived task workspace scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceScope {
    /// Verified files.
    pub verified_files: Vec<SourceFileRef>,
    /// Candidate files.
    pub candidate_files: Vec<SourceFileRef>,
    /// Workspace-relative roots where new files may be created (derived).
    pub allowed_create_roots: Vec<String>,
    /// Paths excluded from default discovery.
    pub excluded_paths: Vec<String>,
    /// Context budget.
    pub budget: WorkspaceContextBudget,
    /// Observable promotion history.
    pub promotions: Vec<ScopePromotionRecord>,
}

/// One eviction record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionRecord {
    /// Evicted path.
    pub path: String,
    /// The view dropped from exact context.
    pub dropped_view: SourceView,
    /// The view retained after eviction.
    pub retained_view: SourceView,
    /// Deterministic eviction reason.
    pub reason: String,
}

/// Hard bounds for workspace-scope models (never raised by input).
pub struct WorkspaceScopeLimits;

impl WorkspaceScopeLimits {
    /// Maximum verified files.
    pub const MAX_VERIFIED_FILES: usize = 64;
    /// Maximum candidate files.
    pub const MAX_CANDIDATE_FILES: usize = 64;
    /// Maximum create roots.
    pub const MAX_CREATE_ROOTS: usize = 8;
    /// Maximum excluded paths.
    pub const MAX_EXCLUDED_PATHS: usize = 32;
    /// Maximum promotions.
    pub const MAX_PROMOTIONS: usize = 128;
    /// Maximum path bytes.
    pub const MAX_PATH_BYTES: usize = 1024;
    /// Maximum evidence bytes.
    pub const MAX_EVIDENCE_BYTES: usize = 256;
    /// Maximum reason bytes.
    pub const MAX_REASON_BYTES: usize = 512;
}

/// Inclusion reasons for active working-set files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileInclusionReason {
    /// Direct task target.
    DirectTaskTarget,
    /// Dependency.
    Dependency,
    /// Test counterpart.
    TestCounterpart,
    /// Architecture owner.
    ArchitectureOwner,
    /// Validation target.
    ValidationTarget,
    /// Candidate under investigation.
    CandidateUnderInvestigation,
}

impl FileInclusionReason {
    /// Exact reference phrase.
    pub fn as_str(self) -> &'static str {
        match self {
            FileInclusionReason::DirectTaskTarget => "direct task target",
            FileInclusionReason::Dependency => "dependency",
            FileInclusionReason::TestCounterpart => "test counterpart",
            FileInclusionReason::ArchitectureOwner => "architecture owner",
            FileInclusionReason::ValidationTarget => "validation target",
            FileInclusionReason::CandidateUnderInvestigation => {
                "candidate under investigation"
            }
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "direct task target" => Self::DirectTaskTarget,
            "dependency" => Self::Dependency,
            "test counterpart" => Self::TestCounterpart,
            "architecture owner" => Self::ArchitectureOwner,
            "validation target" => Self::ValidationTarget,
            "candidate under investigation" => {
                Self::CandidateUnderInvestigation
            }
            _ => return None,
        })
    }
}

/// Working-set file creation input; the inclusion reason is raw text so
/// invalid reasons carry the exact reference rejection.
pub struct ActiveFileInput<'a> {
    /// Workspace-relative path.
    pub path: &'a str,
    /// Raw inclusion-reason text.
    pub reason: &'a str,
    /// Current view.
    pub view: SourceView,
    /// Optional revision handle.
    pub revision: Option<&'a str>,
}

/// One active working-set file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFile {
    /// Workspace-relative path.
    pub path: String,
    /// Inclusion reason.
    pub reason: FileInclusionReason,
    /// Current view.
    pub view: SourceView,
    /// Optional revision handle.
    pub revision: Option<String>,
}

/// The current plan step's bounded working set (a subset of task scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkingSet {
    /// Plan step id this set belongs to.
    pub step_id: String,
    /// Files with inclusion reasons.
    pub files: Vec<ActiveFile>,
}

/// Hard bounds for active working sets.
pub struct ActiveWorkingSetLimits;

impl ActiveWorkingSetLimits {
    /// Maximum files.
    pub const MAX_FILES: usize = 8;
    /// Maximum step-id bytes.
    pub const MAX_STEP_ID_BYTES: usize = 128;
}

const REVISION_HANDLE_PREFIX: &str = "rev_";

fn is_revision_handle(value: &str) -> bool {
    let Some(rest) = value.strip_prefix(REVISION_HANDLE_PREFIX) else {
        return false;
    };
    rest.len() == 32
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn require_bounded_text(
    value: &str,
    max_bytes: usize,
    field: &str,
) -> Result<String, String> {
    let text = value.trim();
    if text.is_empty() {
        return Err(format!("{field} must not be empty."));
    }
    if text.len() > max_bytes {
        return Err(format!("{field} exceeds {max_bytes} UTF-8 bytes."));
    }
    Ok(text.to_owned())
}

fn validate_path(path: &str) -> Result<String, String> {
    let text = require_bounded_text(
        path,
        WorkspaceScopeLimits::MAX_PATH_BYTES,
        "A source path",
    )?;
    let bytes = text.as_bytes();
    let looks_absolute = text.starts_with('/')
        || (bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.get(1) == Some(&b':'));
    if text.contains('\\') || looks_absolute || text.contains('\0') {
        return Err(format!(
            "Source paths must be workspace-relative (no drive, absolute, or backslash): {text}"
        ));
    }
    if text.split('/').any(|segment| segment == "..") {
        return Err(format!("Source paths must not traverse parents: {text}"));
    }
    Ok(text)
}

fn validate_file_ref(input: &SourceFileRef) -> Result<SourceFileRef, String> {
    let path = validate_path(&input.path)?;
    let confidence = input.confidence;
    let revision = match &input.revision {
        Some(value) => Some(require_bounded_text(
            value,
            128,
            &format!("Revision for {path}"),
        )?),
        None => None,
    };
    let evidence = match &input.evidence {
        Some(value) => Some(require_bounded_text(
            value,
            WorkspaceScopeLimits::MAX_EVIDENCE_BYTES,
            &format!("Evidence for {path}"),
        )?),
        None => None,
    };
    let reason = match &input.reason {
        Some(value) => Some(require_bounded_text(
            value,
            WorkspaceScopeLimits::MAX_REASON_BYTES,
            &format!("Reason for {path}"),
        )?),
        None => None,
    };
    if confidence == SourceFileConfidence::Verified {
        let Some(handle) = &revision else {
            return Err(format!(
                "Verified file {path} requires an exact revision handle (rev_ + 32 hex)."
            ));
        };
        if !is_revision_handle(handle) {
            return Err(format!(
                "Verified file {path} requires an exact revision handle (rev_ + 32 hex)."
            ));
        }
        if evidence.is_none() {
            return Err(format!(
                "Verified file {path} requires evidence; a guess is never verified."
            ));
        }
    }
    Ok(SourceFileRef {
        path,
        confidence,
        view: input.view,
        revision,
        evidence,
        reason,
    })
}

/// Creation input for [`create_workspace_scope`].
pub struct CreateWorkspaceScopeInput<'a> {
    /// Verified file refs.
    pub verified_files: &'a [SourceFileRef],
    /// Candidate file refs.
    pub candidate_files: &'a [SourceFileRef],
    /// Allowed create roots.
    pub allowed_create_roots: &'a [String],
    /// Excluded paths.
    pub excluded_paths: &'a [String],
    /// Budget override.
    pub budget: Option<WorkspaceContextBudget>,
    /// Promotion history.
    pub promotions: &'a [ScopePromotionRecord],
}

/// Create the derived task scope. Deterministic, validated, immutable.
///
/// # Errors
///
/// Exact reference messages.
pub fn create_workspace_scope(
    input: &CreateWorkspaceScopeInput<'_>,
) -> Result<WorkspaceScope, String> {
    if input.verified_files.len() > WorkspaceScopeLimits::MAX_VERIFIED_FILES {
        return Err(format!(
            "A workspace scope accepts at most {} verified files.",
            WorkspaceScopeLimits::MAX_VERIFIED_FILES
        ));
    }
    if input.candidate_files.len() > WorkspaceScopeLimits::MAX_CANDIDATE_FILES
    {
        return Err(format!(
            "A workspace scope accepts at most {} candidate files.",
            WorkspaceScopeLimits::MAX_CANDIDATE_FILES
        ));
    }
    let mut verified_paths: Vec<String> = Vec::new();
    let mut candidate_paths: Vec<String> = Vec::new();
    let mut verified_files = Vec::new();
    for file in input.verified_files {
        let reference = validate_file_ref(file)?;
        if verified_paths.contains(&reference.path) {
            return Err(format!(
                "Duplicate verified file: {}",
                reference.path
            ));
        }
        verified_paths.push(reference.path.clone());
        verified_files.push(reference);
    }
    let mut candidate_files = Vec::new();
    for file in input.candidate_files {
        let reference = validate_file_ref(file)?;
        if verified_paths.contains(&reference.path)
            || candidate_paths.contains(&reference.path)
        {
            return Err(format!(
                "Duplicate file across scope sets: {}",
                reference.path
            ));
        }
        candidate_paths.push(reference.path.clone());
        candidate_files.push(reference);
    }
    let mut allowed_create_roots = Vec::new();
    for root in input.allowed_create_roots {
        allowed_create_roots.push(validate_path(root)?);
    }
    if allowed_create_roots.len() > WorkspaceScopeLimits::MAX_CREATE_ROOTS {
        return Err(format!(
            "A workspace scope accepts at most {} create roots.",
            WorkspaceScopeLimits::MAX_CREATE_ROOTS
        ));
    }
    let mut excluded_paths = Vec::new();
    for path in input.excluded_paths {
        excluded_paths.push(validate_path(path)?);
    }
    if excluded_paths.len() > WorkspaceScopeLimits::MAX_EXCLUDED_PATHS {
        return Err(format!(
            "A workspace scope accepts at most {} excluded paths.",
            WorkspaceScopeLimits::MAX_EXCLUDED_PATHS
        ));
    }
    let mut promotions = Vec::new();
    for record in input.promotions {
        let path = validate_path(&record.path)?;
        let evidence = require_bounded_text(
            &record.evidence,
            WorkspaceScopeLimits::MAX_EVIDENCE_BYTES,
            "Promotion evidence",
        )?;
        let revision =
            require_bounded_text(&record.revision, 128, "Promotion revision")?;
        if !is_revision_handle(&revision) {
            return Err(format!(
                "Promotion for {path} requires an exact revision handle."
            ));
        }
        let reason = require_bounded_text(
            &record.reason,
            WorkspaceScopeLimits::MAX_REASON_BYTES,
            "Promotion reason",
        )?;
        promotions.push(ScopePromotionRecord {
            path,
            evidence,
            revision,
            reason,
        });
    }
    if promotions.len() > WorkspaceScopeLimits::MAX_PROMOTIONS {
        return Err(format!(
            "A workspace scope accepts at most {} promotion records.",
            WorkspaceScopeLimits::MAX_PROMOTIONS
        ));
    }
    let budget = input.budget.unwrap_or(DEFAULT_WORKSPACE_CONTEXT_BUDGET);
    validate_budget(&budget)?;
    Ok(WorkspaceScope {
        verified_files,
        candidate_files,
        allowed_create_roots,
        excluded_paths,
        budget,
        promotions,
    })
}

fn validate_budget(budget: &WorkspaceContextBudget) -> Result<(), String> {
    let fields = [
        ("maxActiveExactFiles", budget.max_active_exact_files),
        ("maxExactBytes", budget.max_exact_bytes),
        ("maxStructuralSummaries", budget.max_structural_summaries),
        ("maxCandidateFiles", budget.max_candidate_files),
        ("maxRetainedHistoricalViews", budget.max_retained_historical_views),
    ];
    for (field, value) in fields {
        if value < 1 {
            return Err(format!(
                "Budget {field} must be a positive safe integer."
            ));
        }
    }
    Ok(())
}

/// Add a candidate file to the scope; duplicates are ignored and the
/// oldest candidate detail is dropped over the candidate budget.
///
/// # Errors
///
/// Path validation failures.
pub fn add_candidate_file(
    scope: &WorkspaceScope,
    path: &str,
    note: Option<&str>,
) -> Result<WorkspaceScope, String> {
    let reference = validate_file_ref(&SourceFileRef {
        path: path.to_owned(),
        confidence: SourceFileConfidence::Candidate,
        view: SourceView::None,
        revision: None,
        evidence: None,
        reason: note.map(str::to_owned),
    })?;
    if scope.verified_files.iter().any(|file| file.path == reference.path) {
        return Ok(scope.clone());
    }
    if scope.candidate_files.iter().any(|file| file.path == reference.path) {
        return Ok(scope.clone());
    }
    let mut candidate_files = scope.candidate_files.clone();
    candidate_files.push(reference);
    if candidate_files.len() > scope.budget.max_candidate_files {
        // Budgets control context, not discovery authority: drop the
        // oldest candidate detail.
        candidate_files.remove(0);
    }
    Ok(WorkspaceScope { candidate_files, ..scope.clone() })
}

/// Promotion request for [`promote_candidate_file`].
pub struct PromotionRequest {
    /// Justifying evidence reference.
    pub evidence: String,
    /// Exact inspected revision handle.
    pub revision: String,
    /// Bounded relevance reason.
    pub reason: String,
}

/// Promote a candidate to verified; promotion REQUIRES evidence and an
/// exact revision handle. Returns the updated scope plus the recorded
/// promotion.
///
/// # Errors
///
/// Unknown candidate or invalid promotion fields.
pub fn promote_candidate_file(
    scope: &WorkspaceScope,
    path: &str,
    promotion: &PromotionRequest,
) -> Result<(WorkspaceScope, ScopePromotionRecord), String> {
    if !scope.candidate_files.iter().any(|file| file.path == path) {
        return Err(format!("Cannot promote unknown candidate: {path}"));
    }
    let evidence = require_bounded_text(
        &promotion.evidence,
        WorkspaceScopeLimits::MAX_EVIDENCE_BYTES,
        "Promotion evidence",
    )?;
    let revision =
        require_bounded_text(&promotion.revision, 128, "Promotion revision")?;
    if !is_revision_handle(&revision) {
        return Err(format!(
            "Promotion for {path} requires an exact revision handle."
        ));
    }
    let reason = require_bounded_text(
        &promotion.reason,
        WorkspaceScopeLimits::MAX_REASON_BYTES,
        "Promotion reason",
    )?;
    let record = ScopePromotionRecord {
        path: path.to_owned(),
        evidence,
        revision: revision.clone(),
        reason,
    };
    let mut verified_files = scope.verified_files.clone();
    verified_files.push(SourceFileRef {
        path: path.to_owned(),
        confidence: SourceFileConfidence::Verified,
        view: SourceView::Structural,
        revision: Some(record.revision.clone()),
        evidence: Some(record.evidence.clone()),
        reason: Some(record.reason.clone()),
    });
    if verified_files.len() > WorkspaceScopeLimits::MAX_VERIFIED_FILES {
        return Err(format!(
            "A workspace scope accepts at most {} verified files.",
            WorkspaceScopeLimits::MAX_VERIFIED_FILES
        ));
    }
    let candidate_files: Vec<SourceFileRef> = scope
        .candidate_files
        .iter()
        .filter(|file| file.path != path)
        .cloned()
        .collect();
    let mut promotions = scope.promotions.clone();
    promotions.push(record.clone());
    let promotions = if promotions.len() > WorkspaceScopeLimits::MAX_PROMOTIONS
    {
        promotions[promotions.len() - WorkspaceScopeLimits::MAX_PROMOTIONS..]
            .to_vec()
    } else {
        promotions
    };
    Ok((
        WorkspaceScope {
            verified_files,
            candidate_files,
            promotions,
            ..scope.clone()
        },
        record,
    ))
}

/// Update the representation a file occupies in source context.
///
/// # Errors
///
/// Unknown file or invalid view.
pub fn set_file_view(
    scope: &WorkspaceScope,
    path: &str,
    view: SourceView,
) -> Result<WorkspaceScope, String> {
    let set = |files: &[SourceFileRef]| -> Vec<SourceFileRef> {
        files
            .iter()
            .map(|file| {
                if file.path == path {
                    SourceFileRef { view, ..file.clone() }
                } else {
                    file.clone()
                }
            })
            .collect()
    };
    let in_verified =
        scope.verified_files.iter().any(|file| file.path == path);
    let in_candidates =
        scope.candidate_files.iter().any(|file| file.path == path);
    if !in_verified && !in_candidates {
        return Err(format!("Cannot set view for unknown file: {path}"));
    }
    Ok(WorkspaceScope {
        verified_files: if in_verified {
            set(&scope.verified_files)
        } else {
            scope.verified_files.clone()
        },
        candidate_files: if in_candidates {
            set(&scope.candidate_files)
        } else {
            scope.candidate_files.clone()
        },
        ..scope.clone()
    })
}

/// Working-set creation input.
pub struct CreateActiveWorkingSetInput<'a> {
    /// Plan step id.
    pub step_id: &'a str,
    /// Files with inclusion reasons.
    pub files: &'a [ActiveFileInput<'a>],
}

/// Create the current step's bounded working set.
///
/// # Errors
///
/// Exact reference messages (bounds, ids, reasons, views).
pub fn create_active_working_set(
    input: &CreateActiveWorkingSetInput<'_>,
) -> Result<ActiveWorkingSet, String> {
    let step_id = require_bounded_text(
        input.step_id,
        ActiveWorkingSetLimits::MAX_STEP_ID_BYTES,
        "A step id",
    )?;
    if input.files.len() > ActiveWorkingSetLimits::MAX_FILES {
        return Err(format!(
            "An active working set accepts at most {} files.",
            ActiveWorkingSetLimits::MAX_FILES
        ));
    }
    let reasons = [
        FileInclusionReason::DirectTaskTarget,
        FileInclusionReason::Dependency,
        FileInclusionReason::TestCounterpart,
        FileInclusionReason::ArchitectureOwner,
        FileInclusionReason::ValidationTarget,
        FileInclusionReason::CandidateUnderInvestigation,
    ];
    let mut files = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for file in input.files {
        let path = validate_path(file.path)?;
        if seen.contains(&path) {
            return Err(format!("Duplicate active file: {path}"));
        }
        seen.push(path.clone());
        let Some(reason) = FileInclusionReason::parse(file.reason) else {
            return Err(format!(
                "Invalid inclusion reason for {path}: {}",
                file.reason
            ));
        };
        let _ = reasons.contains(&reason);
        if matches!(file.view, SourceView::None) {
            return Err(format!(
                "Invalid working-set view for {path}: {}",
                file.view.as_str()
            ));
        }
        let revision = match file.revision {
            Some(value) => {
                let bounded = require_bounded_text(
                    value,
                    128,
                    &format!("Revision for {path}"),
                )?;
                if !is_revision_handle(&bounded) {
                    return Err(format!(
                        "Active file {path} has an invalid revision handle."
                    ));
                }
                Some(bounded)
            }
            None => None,
        };
        files.push(ActiveFile { path, reason, view: file.view, revision });
    }
    Ok(ActiveWorkingSet { step_id, files })
}

/// Input for [`evict_low_value_context`].
pub struct EvictLowValueContextInput<'a> {
    /// Current scope.
    pub scope: &'a WorkspaceScope,
    /// Current working set, when any.
    pub working_set: Option<&'a ActiveWorkingSet>,
    /// Host-observed exact byte counts per path.
    pub exact_bytes_of: &'a [(String, usize)],
}

/// Deterministic budget enforcement: demote low-value exact views until
/// the budget holds, always retaining revision identity and evidence.
///
/// # Errors
///
/// Invalid reported byte counts.
pub fn evict_low_value_context(
    input: &EvictLowValueContextInput<'_>,
) -> Result<(WorkspaceScope, Vec<EvictionRecord>), String> {
    let scope = input.scope;
    let working_paths: Vec<&str> = input
        .working_set
        .map(|set| set.files.iter().map(|file| file.path.as_str()).collect())
        .unwrap_or_default();
    let exact_files: Vec<SourceFileRef> = scope
        .verified_files
        .iter()
        .filter(|file| file.view == SourceView::Exact)
        .cloned()
        .collect();
    let candidate_exact: Vec<SourceFileRef> = scope
        .candidate_files
        .iter()
        .filter(|file| file.view == SourceView::Exact)
        .cloned()
        .collect();
    let mut exact_count = exact_files.len() + candidate_exact.len();
    let mut exact_bytes = 0usize;
    let mut estimated_bytes: Vec<(String, usize)> = Vec::new();
    for file in exact_files.iter().chain(candidate_exact.iter()) {
        let reported =
            input.exact_bytes_of.iter().find(|(path, _)| *path == file.path);
        let count = match reported {
            Some((_, size)) => *size,
            None => 0,
        };
        estimated_bytes.push((file.path.clone(), count));
        exact_bytes += count;
    }
    let mut over_count = exact_count > scope.budget.max_active_exact_files;
    let mut over_bytes = exact_bytes > scope.budget.max_exact_bytes;
    let mut evicted = Vec::new();
    let mut scope_after = scope.clone();
    if !over_count && !over_bytes {
        return Ok((scope_after, evicted));
    }
    let tiers: [&[SourceFileRef]; 3] = [
        &candidate_exact,
        &exact_files
            .iter()
            .filter(|file| !working_paths.contains(&file.path.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
        &exact_files
            .iter()
            .filter(|file| working_paths.contains(&file.path.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
    ];
    for tier in tiers {
        for file in tier {
            if !over_count && !over_bytes {
                break;
            }
            let size = estimated_bytes
                .iter()
                .find(|(path, _)| path == &file.path)
                .map(|(_, size)| *size)
                .unwrap_or(0);
            scope_after =
                set_file_view(&scope_after, &file.path, SourceView::Summary)?;
            exact_count -= 1;
            exact_bytes = exact_bytes.saturating_sub(size);
            evicted.push(EvictionRecord {
                path: file.path.clone(),
                dropped_view: SourceView::Exact,
                retained_view: SourceView::Summary,
                reason: if working_paths.contains(&file.path.as_str()) {
                    "over budget; exact source demoted to summary with revision/evidence retained"
                        .to_owned()
                } else {
                    "over budget; low-value exact source demoted to summary with revision/evidence retained"
                        .to_owned()
                },
            });
            over_count =
                exact_count > scope_after.budget.max_active_exact_files;
            over_bytes = exact_bytes > scope_after.budget.max_exact_bytes;
        }
        if !over_count && !over_bytes {
            break;
        }
    }
    Ok((scope_after, evicted))
}
