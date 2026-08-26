//! Deterministic host-owned planning-depth selection (Stage 3 milestone
//! 7, ADR 0020; Stage 3R R13.4).
//!
//! The HOST decides whether planning is needed and at what depth — never
//! the model. The policy is a pure deterministic function of host-visible
//! task facts; ambiguous signals resolve conservatively toward `light`
//! over `none`. The marker checks are faithful deterministic ports of the
//! reference's regular expressions (no regex engine in core).

use super::model::PlanningDepth;

/// Deterministic machine-readable reason for a planning decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningDecisionReason {
    /// Explicit plan request.
    ExplicitPlanRequest,
    /// Inspection-only or no expected mutation.
    InspectionOrNoMutation,
    /// Protected behavioral configuration involved.
    ProtectedConfig,
    /// Multiple subsystems.
    MultiSubsystem,
    /// Research required.
    ResearchRequired,
    /// Capability uncertainty.
    CapabilityUncertainty,
    /// Godot scene/resource relationships.
    SceneResourceRelationships,
    /// Mixed script/native surface.
    MixedSurfaceRelationships,
    /// Narrow repair on a known surface.
    NarrowRepairKnownSurface,
    /// Unknown surface, conservatively bounded.
    UnknownSurfaceBounded,
    /// Broad surface or many criteria.
    BroadSurfaceOrManyCriteria,
    /// Bounded but non-trivial.
    BoundedNonTrivial,
}

impl PlanningDecisionReason {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            PlanningDecisionReason::ExplicitPlanRequest => {
                "explicit-plan-request"
            }
            PlanningDecisionReason::InspectionOrNoMutation => {
                "inspection-or-no-mutation"
            }
            PlanningDecisionReason::ProtectedConfig => "protected-config",
            PlanningDecisionReason::MultiSubsystem => "multi-subsystem",
            PlanningDecisionReason::ResearchRequired => "research-required",
            PlanningDecisionReason::CapabilityUncertainty => {
                "capability-uncertainty"
            }
            PlanningDecisionReason::SceneResourceRelationships => {
                "scene-resource-relationships"
            }
            PlanningDecisionReason::MixedSurfaceRelationships => {
                "mixed-surface-relationships"
            }
            PlanningDecisionReason::NarrowRepairKnownSurface => {
                "narrow-repair-known-surface"
            }
            PlanningDecisionReason::UnknownSurfaceBounded => {
                "unknown-surface-bounded"
            }
            PlanningDecisionReason::BroadSurfaceOrManyCriteria => {
                "broad-surface-or-many-criteria"
            }
            PlanningDecisionReason::BoundedNonTrivial => "bounded-non-trivial",
        }
    }
}

/// A host depth decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningDecision {
    /// Selected depth.
    pub depth: PlanningDepth,
    /// Deterministic reason.
    pub reason: PlanningDecisionReason,
}

/// Host-visible development-surface classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    /// Script files only.
    ScriptOnly,
    /// Native resources only.
    NativeOnly,
    /// Mixed script and native surface.
    Mixed,
    /// No classified surface.
    None,
}

impl SurfaceKind {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SurfaceKind::ScriptOnly => "script_only",
            SurfaceKind::NativeOnly => "native_only",
            SurfaceKind::Mixed => "mixed",
            SurfaceKind::None => "none",
        }
    }
}

/// Explicitly requested plan depth for an explicit plan request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedDepth {
    /// Light.
    Light,
    /// Full.
    Full,
}

/// Host-visible signals the depth policy may use.
pub struct PlanningDecisionInput<'a> {
    /// Task request text (protected-config detection).
    pub request: &'a str,
    /// The user explicitly asked for a plan.
    pub explicit_plan_request: bool,
    /// Depth the user explicitly requested.
    pub requested_depth: Option<RequestedDepth>,
    /// The task only inspects/reviews.
    pub inspection_only: bool,
    /// Source mutation is expected.
    pub expected_mutation: bool,
    /// Number of acceptance criteria.
    pub acceptance_criterion_count: usize,
    /// Protected behavioral configuration is involved.
    pub protected_config_involved: bool,
    /// The task spans multiple subsystems.
    pub spans_multiple_subsystems: bool,
    /// Project/reference research is required.
    pub research_required: bool,
    /// Runtime capability uncertainty remains.
    pub capability_uncertainty: bool,
    /// Narrow repair of an identified issue.
    pub narrow_repair: bool,
    /// Likely touched files already known (0 = unknown).
    pub known_touchpoints: usize,
    /// Explicit Godot scene/resource relationship work.
    pub involves_godot_scene_or_resource: Option<bool>,
    /// Host-derived development-surface classification.
    pub surface: Option<SurfaceKind>,
}

/// Host-owned planning-depth policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanningPolicy;

impl PlanningPolicy {
    /// Decide the planning depth from host-visible facts.
    pub fn decide(
        &self,
        input: &PlanningDecisionInput<'_>,
    ) -> PlanningDecision {
        if input.explicit_plan_request {
            let depth = match input.requested_depth {
                Some(RequestedDepth::Light) => PlanningDepth::Light,
                Some(RequestedDepth::Full) | None => PlanningDepth::Full,
            };
            return PlanningDecision {
                depth,
                reason: PlanningDecisionReason::ExplicitPlanRequest,
            };
        }
        if input.inspection_only || !input.expected_mutation {
            return PlanningDecision {
                depth: PlanningDepth::None,
                reason: PlanningDecisionReason::InspectionOrNoMutation,
            };
        }
        if input.protected_config_involved {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::ProtectedConfig,
            };
        }
        if input.spans_multiple_subsystems {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::MultiSubsystem,
            };
        }
        if input.research_required {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::ResearchRequired,
            };
        }
        if input.capability_uncertainty {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::CapabilityUncertainty,
            };
        }
        if input.involves_godot_scene_or_resource == Some(true)
            && (input.known_touchpoints > 2
                || input.acceptance_criterion_count >= 3)
        {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::SceneResourceRelationships,
            };
        }
        if input.surface == Some(SurfaceKind::Mixed) {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::MixedSurfaceRelationships,
            };
        }
        if input.narrow_repair
            && input.known_touchpoints > 0
            && input.known_touchpoints <= 2
        {
            return PlanningDecision {
                depth: PlanningDepth::None,
                reason: PlanningDecisionReason::NarrowRepairKnownSurface,
            };
        }
        if input.known_touchpoints == 0 {
            return PlanningDecision {
                depth: PlanningDepth::Light,
                reason: PlanningDecisionReason::UnknownSurfaceBounded,
            };
        }
        if input.acceptance_criterion_count >= 4 || input.known_touchpoints > 4
        {
            return PlanningDecision {
                depth: PlanningDepth::Full,
                reason: PlanningDecisionReason::BroadSurfaceOrManyCriteria,
            };
        }
        PlanningDecision {
            depth: PlanningDepth::Light,
            reason: PlanningDecisionReason::BoundedNonTrivial,
        }
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn boundary_before(bytes: &[u8], start: usize) -> bool {
    start == 0 || !is_word_byte(bytes[start - 1])
}

fn boundary_after(bytes: &[u8], end: usize) -> bool {
    end == bytes.len() || !is_word_byte(bytes[end])
}

/// Find `needle` in `haystack` at every index, invoking `on_match` with
/// the [start, end) byte range; stops early when `on_match` returns true.
fn for_each_occurrence(
    haystack: &[u8],
    needle: &[u8],
    on_match: &mut dyn FnMut(usize, usize) -> bool,
) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    'outer: for start in 0..=(haystack.len() - needle.len()) {
        for (offset, byte) in needle.iter().enumerate() {
            if haystack[start + offset] != *byte {
                continue 'outer;
            }
        }
        if on_match(start, start + needle.len()) {
            return true;
        }
    }
    false
}

/// Deterministic marker check for protected behavioral-config references:
/// `AGENTS.md`, `.siralos/`, or the phrase "behavioural config".
pub fn contains_protected_config_reference(text: &str) -> bool {
    let normalized = text.replace('\\', "/");
    let bytes = normalized.to_ascii_lowercase();
    let bytes = bytes.as_bytes();
    let agents_hit =
        for_each_occurrence(bytes, b"agents.md", &mut |start, end| {
            // (^|[\s/"]) before and ([\s/"]|$) after.
            let before_ok = start == 0
                || matches!(bytes[start - 1], b'/' | b'"')
                || bytes[start - 1].is_ascii_whitespace();
            let after_ok = boundary_whitespace_quote_slash_end(bytes, end);
            before_ok && after_ok
        });
    if agents_hit {
        return true;
    }
    let siralos_hit =
        for_each_occurrence(bytes, b".siralos", &mut |_start, end| {
            // (^|[\s/"])(\.siralos)(\/|[\s"]|$)/i — the leading dot needs no
            // left boundary; the tail must be a slash, space, quote, or end.
            boundary_whitespace_quote_slash_end(bytes, end)
        });
    if siralos_hit {
        return true;
    }
    normalized.to_ascii_lowercase().contains("behavioural config")
        || normalized.to_ascii_lowercase().contains("behavioral config")
}

fn byte_is_space_or_quote_or_slash(byte: u8) -> bool {
    byte == b'/' || byte == b'"' || byte.is_ascii_whitespace()
}

fn boundary_whitespace_quote_slash_end(bytes: &[u8], end: usize) -> bool {
    match bytes.get(end) {
        None => true,
        Some(byte) => byte_is_space_or_quote_or_slash(*byte),
    }
}

/// Deterministic marker check for explicit Godot scene/resource
/// references: `.tscn`/`.tres` paths, scene/resource tree phrasing, or
/// inherited/instanced-scene phrasing. Plain prose mentioning "scene" or
/// "resource" alone does not match.
pub fn contains_godot_scene_or_resource_reference(text: &str) -> bool {
    let normalized = text.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    // /\.(tscn|tres)\b/i
    let extension_hit =
        for_each_occurrence(bytes, b".tscn", &mut |_start, end| {
            boundary_after(bytes, end)
        }) || for_each_occurrence(bytes, b".tres", &mut |_start, end| {
            boundary_after(bytes, end)
        });
    if extension_hit {
        return true;
    }
    // \b(?:scene|resource)\s+(?:file|tree|inherits?|instance|instanced|
    // signal|connection)s?\b/i
    const NOUNS: [&[u8]; 8] = [
        b"file",
        b"tree",
        b"inherits",
        b"inherit",
        b"instance",
        b"instanced",
        b"signal",
        b"connection",
    ];
    for lead in [b"scene" as &[u8], b"resource" as &[u8]] {
        let hit = for_each_occurrence(bytes, lead, &mut |start, end| {
            if !boundary_before(bytes, start) {
                return false;
            }
            // \s+ — consume the full whitespace run.
            let mut cursor = end;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor == end {
                return false;
            }
            for noun in NOUNS {
                if bytes[cursor..].starts_with(noun) {
                    let noun_end = cursor + noun.len();
                    // Optional plural `s?` — only when not already part of
                    // the matched word (e.g. "inherits" already ends in s).
                    let final_end = if bytes.get(noun_end) == Some(&b's')
                        && !ends_with_s(noun)
                    {
                        noun_end + 1
                    } else {
                        noun_end
                    };
                    if boundary_after(bytes, final_end) {
                        return true;
                    }
                }
            }
            false
        });
        if hit {
            return true;
        }
    }
    // \b(?:inherited|instanced)\s+scene\b/i
    for lead in [b"inherited" as &[u8], b"instanced" as &[u8]] {
        let hit = for_each_occurrence(bytes, lead, &mut |start, end| {
            if !boundary_before(bytes, start) {
                return false;
            }
            let mut cursor = end;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor == end {
                return false;
            }
            if bytes[cursor..].starts_with(b"scene") {
                return boundary_after(bytes, cursor + b"scene".len());
            }
            false
        });
        if hit {
            return true;
        }
    }
    false
}

fn ends_with_s(word: &[u8]) -> bool {
    word.last() == Some(&b's')
}
