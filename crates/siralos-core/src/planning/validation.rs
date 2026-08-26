//! Host-owned plan candidate validation boundary (Stage 3 milestone 7;
//! Stage 3R R13.4).
//!
//! Planner output is untrusted data: it becomes a `TaskPlan` only after
//! this deterministic validation passes. Malformed output is REJECTED
//! with exact reference reasons — never silently treated as plan prose.
//! Validation covers structure, bounds, path containment, evidence and
//! revision identity of verified touchpoints, acceptance-criteria
//! linkage, secret-shaped content, and policy-shaped capability claims.
//! The reference's regular expressions are ported as faithful
//! hand-rolled deterministic matchers (core has no regex engine).

use serde_json::Value;

use super::model::{
    PlanConstraint, PlanRisk, PlanRiskSeverity, PlanScope, PlanStep,
    PlanTouchpoint, PlanValidationStrategy, PlanningDepth, PlanningLimits,
    TaskPlanContent, is_valid_plan_element_id, is_valid_revision_handle,
};
use crate::task::TaskContract;

/// Validation context: the exact contract the plan binds to plus the
/// host-routed depth.
pub struct PlanCandidateContext<'a> {
    /// The exact TaskContract the plan will bind to.
    pub contract: &'a TaskContract,
    /// The host-routed depth; the candidate must match it.
    pub depth: PlanningDepth,
}

/// Outcome of validating one untrusted candidate.
#[derive(Debug)]
pub enum PlanCandidateResult {
    /// The candidate passed; content was reconstructed to the exact shape.
    Ok(Box<TaskPlanContent>),
    /// The candidate failed with up to eight exact reasons.
    Rejected(Vec<String>),
}

fn is_plain_object(value: Option<&Value>) -> bool {
    value.is_some_and(Value::is_object)
}

fn non_empty_string(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| !text.trim().is_empty())
}

fn is_string_array(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .is_some_and(|entries| entries.iter().all(|entry| entry.is_string()))
}

/// Conservative deterministic check for secret-shaped content.
fn reject_secret_content(text: &str) -> Option<&'static str> {
    if secret_assignment_pattern(text) || secret_key_prefix_pattern(text) {
        return Some(
            "The plan contains secret-shaped content and was rejected.",
        );
    }
    None
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

/// /\b(api[_-]?key|secret|password|passwd|token|bearer|private[_-]?key|
/// credential|access[_-]?key)\b\s*[:=]\s*[A-Za-z0-9_\-./+]{8,}/i
fn secret_assignment_pattern(text: &str) -> bool {
    const WORDS: [&[u8]; 10] = [
        b"api_key",
        b"api-key",
        b"apikey",
        b"secret",
        b"password",
        b"passwd",
        b"token",
        b"bearer",
        b"credential",
        b"access-key",
    ];
    // Additional composite spellings handled below: private[_-]?key and
    // access[_-]?key (the hyphen/underscore variants are generated).
    let lower_bytes = text.to_ascii_lowercase();
    let bytes = lower_bytes.as_bytes();
    let mut words: Vec<Vec<u8>> =
        WORDS.iter().map(|word| word.to_vec()).collect();
    words.push(b"private_key".to_vec());
    words.push(b"private-key".to_vec());
    words.push(b"privatekey".to_vec());
    words.push(b"access_key".to_vec());
    words.push(b"accesskey".to_vec());
    'outer: for start in 0..bytes.len() {
        for word in &words {
            if !bytes[start..].starts_with(word) {
                continue;
            }
            let mut end = start + word.len();
            if !(end == bytes.len() || !is_word_byte(bytes[end])) {
                continue;
            }
            if !boundary_before(bytes, start) {
                continue;
            }
            // \s*[:=]\s*
            while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end >= bytes.len() || (bytes[end] != b':' && bytes[end] != b'=')
            {
                continue;
            }
            end += 1;
            while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            // [A-Za-z0-9_\-./+]{8,}
            let run = bytes[end..]
                .iter()
                .take_while(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(**byte, b'_' | b'-' | b'.' | b'/' | b'+')
                })
                .count();
            if run >= 8 {
                return true;
            }
            continue 'outer;
        }
    }
    false
}

/// /\b(sk|pk|ghp|gho|ghu)[A-Za-z0-9]{16,}\b/i plus the case-sensitive
/// /\bAKIA[0-9A-Z]{16,}\b/ variant.
fn secret_key_prefix_pattern(text: &str) -> bool {
    const PREFIXES: [&[u8]; 5] = [b"sk", b"pk", b"ghp", b"gho", b"ghu"];
    let lower_bytes = text.to_ascii_lowercase();
    let lower_bytes = lower_bytes.as_bytes();
    let original_bytes = text.as_bytes();
    'outer_lower: for start in 0..lower_bytes.len() {
        for prefix in PREFIXES {
            if !lower_bytes[start..].starts_with(prefix) {
                continue;
            }
            if !boundary_before(lower_bytes, start) {
                continue;
            }
            let run_len = original_bytes
                .get(start + prefix.len()..)
                .map(|rest| {
                    rest.iter()
                        .take_while(|byte| byte.is_ascii_alphanumeric())
                        .count()
                })
                .unwrap_or(0);
            if run_len < 16 {
                continue;
            }
            let end = start + prefix.len() + run_len;
            if boundary_after(original_bytes, end) {
                return true;
            }
            continue 'outer_lower;
        }
    }
    // Case-sensitive AWS prefix over uppercase alphanumerics.
    'outer_akia: for start in 0..original_bytes.len() {
        if !original_bytes[start..].starts_with(b"AKIA")
            || !boundary_before(original_bytes, start)
        {
            continue;
        }
        let run_len = original_bytes[start + 4..]
            .iter()
            .take_while(|byte| {
                byte.is_ascii_digit() || byte.is_ascii_uppercase()
            })
            .count();
        if run_len < 16 {
            continue;
        }
        let end = start + 4 + run_len;
        if boundary_after(original_bytes, end) {
            return true;
        }
        continue 'outer_akia;
    }
    false
}

/// Conservative deterministic policy-claim patterns (mirrors knowledge's
/// rejection posture; plans can never claim authority). Operates on a
/// normalized form: lowercased, whitespace collapsed to single spaces.
pub fn reject_plan_policy_claims(text: &str) -> Option<&'static str> {
    let lowered = text.to_lowercase();
    let normalized = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    let bytes = normalized.as_bytes();
    if policy_claim_one(bytes)
        || policy_claim_two(bytes)
        || policy_claim_three(bytes)
        || policy_claim_four(bytes)
        || policy_claim_five(bytes)
    {
        return Some(POLICY_CLAIM_MESSAGE);
    }
    None
}

const POLICY_CLAIM_MESSAGE: &str = "The plan contains a policy-shaped capability claim; plans are descriptive and can never grant or disable capability, sandbox, approval, or execution policy.";

fn find_subslice(
    haystack: &[u8],
    from: usize,
    needle: &[u8],
) -> Option<usize> {
    if needle.is_empty() || haystack.len() < from + needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

/// Gap of up to `max` characters containing none of the excluded bytes,
/// starting exactly at `from`; returns the exclusive end of the maximal gap.
fn gap_end(bytes: &[u8], from: usize, max: usize, excluded: u8) -> usize {
    let mut end = from;
    while end < bytes.len() && bytes[end] != excluded && end - from < max {
        end += 1;
    }
    end
}

/// Pattern 1:
/// \b(enable|allow|grant|permit|approve)\b[^.\n]{0,60}\b(unrestricted|
/// full)?\s*(network|internet|shell|write|execution|commands?|sandbox|
/// approval|mutation)\b
fn policy_claim_one(bytes: &[u8]) -> bool {
    const VERBS: [&[u8]; 5] =
        [b"enable", b"allow", b"grant", b"permit", b"approve"];
    const NOUNS: [&[u8]; 10] = [
        b"network",
        b"internet",
        b"shell",
        b"write",
        b"execution",
        b"commands",
        b"command",
        b"sandbox",
        b"approval",
        b"mutation",
    ];
    for verb in VERBS {
        let mut search_from = 0usize;
        while let Some(verb_end_rel) = find_subslice(bytes, search_from, verb)
        {
            let verb_start = verb_end_rel;
            let verb_end = verb_start + verb.len();
            search_from = verb_start + 1;
            if !boundary_before(bytes, verb_start)
                || !boundary_after(bytes, verb_end)
            {
                continue;
            }
            let limit = gap_end(bytes, verb_end, 60, b'.');
            // Try every split point inside the gap for the optional
            // ("unrestricted"|"full") group followed by \s* and a noun.
            let mut cursor = verb_end;
            loop {
                for qualifier in
                    [Some(&b"unrestricted"[..]), Some(&b"full"[..]), None]
                {
                    if let Some(word) = qualifier {
                        if bytes[cursor..].starts_with(word)
                            && boundary_before(bytes, cursor)
                        {
                            let after = cursor + word.len();
                            if after > limit {
                                continue;
                            }
                            let mut noun_start = after;
                            while noun_start < bytes.len()
                                && bytes[noun_start] == b' '
                            {
                                noun_start += 1;
                            }
                            if noun_starts_noun_match(
                                bytes, noun_start, &NOUNS,
                            ) {
                                return true;
                            }
                        }
                    } else if noun_starts_noun_match(bytes, cursor, &NOUNS) {
                        return true;
                    }
                }
                if cursor >= limit {
                    break;
                }
                cursor += 1;
            }
        }
    }
    false
}

/// Whole-word noun check shared by pattern 1 branches: the noun must
/// begin exactly at `cursor` with word boundaries on both sides.
fn noun_starts_noun_match(
    bytes: &[u8],
    cursor: usize,
    nouns: &[&[u8]],
) -> bool {
    if cursor > bytes.len()
        || (cursor < bytes.len() && !boundary_before(bytes, cursor))
    {
        return false;
    }
    for noun in nouns {
        if bytes[cursor..].starts_with(noun) {
            let end = cursor + noun.len();
            if boundary_after(bytes, end) {
                return true;
            }
        }
    }
    false
}

/// Pattern 2:
/// \b(disable|bypass|turn\s*off|turn\s*down|ignore|override)\b[^.\n]{0,60}
/// \b(sandbox|approval|checkpoint|security|policy|restriction|limit|
/// permission)\b
fn policy_claim_two(bytes: &[u8]) -> bool {
    const LEADS: [&[u8]; 6] = [
        b"disable",
        b"bypass",
        b"turn off",
        b"turn down",
        b"ignore",
        b"override",
    ];
    const TARGETS: [&[u8]; 8] = [
        b"sandbox",
        b"approval",
        b"checkpoint",
        b"security",
        b"policy",
        b"restriction",
        b"limit",
        b"permission",
    ];
    for lead in LEADS {
        let mut search_from = 0usize;
        while let Some(start) = find_subslice(bytes, search_from, lead) {
            let lead_end = start + lead.len();
            search_from = start + 1;
            let lead_boundary_ok = boundary_before(bytes, start)
                && boundary_after(bytes, lead_end);
            if !lead_boundary_ok {
                continue;
            }
            let limit = gap_end(bytes, lead_end, 60, b'.');
            for target in TARGETS {
                let mut cursor = lead_end;
                while let Some(position) = find_subslice(bytes, cursor, target)
                {
                    if position > limit {
                        break;
                    }
                    let end = position + target.len();
                    if boundary_before(bytes, position)
                        && boundary_after(bytes, end)
                    {
                        return true;
                    }
                    cursor = position + 1;
                }
            }
        }
    }
    false
}

/// Pattern 3:
/// \bno\s+(approval|permission|checkpoint|review|sandbox)\b[^.\n]{0,40}\b
/// (needed|required|necessary)\b
fn policy_claim_three(bytes: &[u8]) -> bool {
    const KINDS: [&[u8]; 5] =
        [b"approval", b"permission", b"checkpoint", b"review", b"sandbox"];
    const TAILS: [&[u8]; 3] = [b"needed", b"required", b"necessary"];
    for kind in KINDS {
        let pattern: Vec<u8> = [b"no ".as_slice(), kind].concat();
        let mut search_from = 0usize;
        while let Some(start) = find_subslice(bytes, search_from, &pattern) {
            let end = start + pattern.len();
            search_from = start + 1;
            if !boundary_before(bytes, start) || !boundary_after(bytes, end) {
                continue;
            }
            let limit = gap_end(bytes, end, 40, b'.');
            for tail in TAILS {
                let mut cursor = end;
                while let Some(position) = find_subslice(bytes, cursor, tail) {
                    if position > limit {
                        break;
                    }
                    let tail_end = position + tail.len();
                    if boundary_before(bytes, position)
                        && boundary_after(bytes, tail_end)
                    {
                        return true;
                    }
                    cursor = position + 1;
                }
            }
        }
    }
    false
}

/// Pattern 4:
/// \b(commands?|scripts?|shell|execution|mutations?|writes?|edits?)\b
/// [^.\n]{0,40}\b(without|no)\s+(approval|permission|checkpoint|review)\b
fn policy_claim_four(bytes: &[u8]) -> bool {
    const SUBJECTS: [&[u8]; 12] = [
        b"commands",
        b"command",
        b"scripts",
        b"script",
        b"shell",
        b"execution",
        b"mutations",
        b"mutation",
        b"writes",
        b"write",
        b"edits",
        b"edit",
    ];
    const CONNECTORS: [&[u8]; 2] = [b"without", b"no"];
    const OBJECTS: [&[u8]; 4] =
        [b"approval", b"permission", b"checkpoint", b"review"];
    for subject in SUBJECTS {
        let mut search_from = 0usize;
        while let Some(start) = find_subslice(bytes, search_from, subject) {
            let subject_end = start + subject.len();
            search_from = start + 1;
            if !boundary_before(bytes, start)
                || !boundary_after(bytes, subject_end)
            {
                continue;
            }
            let limit = gap_end(bytes, subject_end, 40, b'.');
            for connector in CONNECTORS {
                let mut cursor = subject_end;
                let mut found_connector = false;
                while let Some(position) =
                    find_subslice(bytes, cursor, connector)
                {
                    if position > limit {
                        break;
                    }
                    let connector_end = position + connector.len();
                    if boundary_before(bytes, position)
                        && boundary_after(bytes, connector_end)
                    {
                        found_connector = true;
                        // \s+ then object word.
                        let mut word_start = connector_end;
                        while word_start < bytes.len()
                            && bytes[word_start] == b' '
                        {
                            word_start += 1;
                        }
                        if word_start == connector_end {
                            cursor = position + 1;
                            continue;
                        }
                        for object in OBJECTS {
                            if bytes[word_start..].starts_with(object) {
                                let object_end = word_start + object.len();
                                if boundary_after(bytes, object_end) {
                                    return true;
                                }
                            }
                        }
                    }
                    cursor = position + 1;
                }
                if found_connector {
                    break;
                }
            }
        }
    }
    false
}

/// Pattern 5: ^[^.\n]{0,40}\b(unrestricted|full)\s+(network|shell|write|
/// access)\b
fn policy_claim_five(bytes: &[u8]) -> bool {
    const QUALIFIERS: [&[u8]; 2] = [b"unrestricted", b"full"];
    const NOUNS: [&[u8]; 4] = [b"network", b"shell", b"write", b"access"];
    let limit = gap_end(bytes, 0, 40, b'.');
    for qualifier in QUALIFIERS {
        let mut cursor = 0usize;
        while let Some(position) = find_subslice(bytes, cursor, qualifier) {
            if position > limit {
                break;
            }
            let qualifier_end = position + qualifier.len();
            if boundary_before(bytes, position)
                && boundary_after(bytes, qualifier_end)
            {
                let mut noun_start = qualifier_end;
                while noun_start < bytes.len() && bytes[noun_start] == b' ' {
                    noun_start += 1;
                }
                if noun_start > qualifier_end {
                    for noun in NOUNS {
                        if bytes[noun_start..].starts_with(noun) {
                            let noun_end = noun_start + noun.len();
                            if boundary_after(bytes, noun_end) {
                                return true;
                            }
                        }
                    }
                }
            }
            cursor = position + 1;
        }
    }
    false
}

/// Workspace-relative path containment: relative, forward slashes, no
/// null bytes, no workspace escape, never a reference/research namespace
/// path. Candidate touchpoints may carry glob wildcards.
pub fn is_safe_plan_path(path: &str, allow_glob: bool) -> bool {
    if path.is_empty() || path.len() > PlanningLimits::MAX_PATH_BYTES {
        return false;
    }
    if path.contains('\0') || path.contains('\\') || path.starts_with('/') {
        return false;
    }
    let drive_prefix =
        path.as_bytes().first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && path.as_bytes().get(1) == Some(&b':');
    if drive_prefix {
        return false;
    }
    if path.starts_with("@reference/") || path.starts_with("@research/") {
        return false;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| {
        *segment == ".." || *segment == "." || segment.is_empty()
    }) {
        return false;
    }
    if !allow_glob && (path.contains('*') || path.contains('?')) {
        return false;
    }
    if allow_glob {
        let without_doublestar = path.replace("**", "");
        let without_star = without_doublestar.replace('*', "");
        if without_star.contains('*') || without_star.contains('?') {
            return false;
        }
    }
    true
}

const EVIDENCE_KINDS: [&str; 8] = [
    "read",
    "api",
    "reference",
    "research",
    "knowledge",
    "instruction",
    "scene",
    "resource",
];

fn is_evidence_reference(value: &str) -> bool {
    let Some(separator) = value.find(':') else {
        return false;
    };
    if separator == 0 {
        return false;
    }
    let kind = &value[..separator];
    let reference = &value[separator + 1..];
    if !EVIDENCE_KINDS.contains(&kind) {
        return false;
    }
    !reference.is_empty()
        && reference.len() <= 256
        && !reference.contains('\0')
}

struct ReasonSink(Vec<String>);

impl ReasonSink {
    fn push(&mut self, reason: impl Into<String>) {
        self.0.push(reason.into());
    }
}

fn bounded(value: &str, max_bytes: usize) -> bool {
    value.len() <= max_bytes
}

/// Validate one untrusted planner candidate expressed as JSON.
#[allow(clippy::too_many_lines)]
pub fn validate_plan_candidate(
    raw: &Value,
    context: &PlanCandidateContext<'_>,
) -> PlanCandidateResult {
    let depth_label = context.depth.as_str();
    if !is_plain_object(Some(raw)) {
        return PlanCandidateResult::Rejected(vec![
            "The plan candidate must be a JSON object.".to_owned(),
        ]);
    }
    let object = raw.as_object().expect("checked plain object");
    if !matches!(context.depth, PlanningDepth::Light | PlanningDepth::Full) {
        return PlanCandidateResult::Rejected(vec![
            "A plan cannot be created at depth none.".to_owned(),
        ]);
    }
    let mut reasons = ReasonSink(Vec::new());

    if let Some(declared_depth) = object.get("depth").and_then(Value::as_str) {
        if declared_depth != depth_label {
            reasons.push(format!(
                "The plan depth {} does not match the host-routed depth {}.",
                crate::identity::json_escape(declared_depth),
                depth_label
            ));
        }
    }

    let objective_value = object.get("objective");
    let mut objective: Option<String> = None;
    if !non_empty_string(objective_value) {
        reasons.push("A plan requires a non-empty objective.".to_owned());
    } else {
        let text = objective_value.and_then(Value::as_str).unwrap_or_default();
        if !bounded(text, PlanningLimits::MAX_OBJECTIVE_BYTES) {
            reasons.push(format!(
                "The objective exceeds the {}-byte bound.",
                PlanningLimits::MAX_OBJECTIVE_BYTES
            ));
        } else {
            if let Some(claim) = reject_plan_policy_claims(text) {
                reasons.push(claim.to_owned());
            }
            if let Some(secret) = reject_secret_content(text) {
                reasons.push(secret.to_owned());
            }
            objective = Some(text.to_owned());
        }
    }

    let max_steps = if context.depth == PlanningDepth::Light {
        PlanningLimits::MAX_STEPS_LIGHT
    } else {
        PlanningLimits::MAX_STEPS
    };

    let scope_raw = object.get("scope");
    let mut scope: Option<PlanScope> = None;
    if scope_raw.is_none() && context.depth == PlanningDepth::Light {
        // Light plans are compact: scope is not forced onto them.
    } else if !is_plain_object(scope_raw) {
        reasons.push("A plan requires a scope object.".to_owned());
    } else {
        let scope_object = scope_raw.expect("checked object");
        let in_scope = scope_object.get("inScope");
        let out_of_scope = scope_object.get("outOfScope");
        if !is_string_array(in_scope)
            || in_scope.and_then(Value::as_array).is_some_and(|entries| {
                entries.len() > PlanningLimits::MAX_SCOPE_ENTRIES
            })
        {
            reasons.push(
                "A plan requires an inScope string array within bounds."
                    .to_owned(),
            );
        } else {
            let entries = in_scope
                .and_then(Value::as_array)
                .map(|entries| entries.as_slice())
                .unwrap_or(&[]);
            if entries.iter().any(|entry| {
                !bounded(
                    entry.as_str().unwrap_or_default(),
                    PlanningLimits::MAX_STATEMENT_BYTES,
                )
            }) {
                reasons.push(
                    "A scope entry exceeds the statement byte bound."
                        .to_owned(),
                );
            }
        }
        if !is_string_array(out_of_scope)
            || out_of_scope.and_then(Value::as_array).is_some_and(|entries| {
                entries.len() > PlanningLimits::MAX_SCOPE_ENTRIES
            })
        {
            reasons.push(
                "A plan requires an outOfScope string array within bounds."
                    .to_owned(),
            );
        } else {
            let entries = out_of_scope
                .and_then(Value::as_array)
                .map(|entries| entries.as_slice())
                .unwrap_or(&[]);
            if entries.iter().any(|entry| {
                !bounded(
                    entry.as_str().unwrap_or_default(),
                    PlanningLimits::MAX_STATEMENT_BYTES,
                )
            }) {
                reasons.push(
                    "A scope entry exceeds the statement byte bound."
                        .to_owned(),
                );
            }
        }
        if scope.is_none() {
            scope = Some(PlanScope {
                in_scope: string_list(in_scope),
                out_of_scope: string_list(out_of_scope),
            });
        }
    }

    let non_goals_raw = object.get("nonGoals");
    let mut non_goals: Vec<String> = Vec::new();
    if non_goals_raw.is_none() && context.depth == PlanningDepth::Light {
        // Compact light plans may omit non-goals.
    } else if !is_string_array(non_goals_raw)
        || non_goals_raw.and_then(Value::as_array).is_some_and(|entries| {
            entries.len() > PlanningLimits::MAX_NON_GOALS
        })
    {
        reasons.push("nonGoals must be a bounded string array.".to_owned());
    } else {
        let entries = non_goals_raw
            .and_then(Value::as_array)
            .map(|entries| entries.as_slice())
            .unwrap_or(&[]);
        if entries.iter().any(|entry| {
            !bounded(
                entry.as_str().unwrap_or_default(),
                PlanningLimits::MAX_STATEMENT_BYTES,
            )
        }) {
            reasons.push(
                "A non-goal exceeds the statement byte bound.".to_owned(),
            );
        } else {
            non_goals = string_list(non_goals_raw);
        }
    }

    let touchpoints_raw = object.get("touchpoints");
    let mut touchpoints: Vec<PlanTouchpoint> = Vec::new();
    let mut touchpoint_ids: Vec<String> = Vec::new();
    let touchpoints_array = touchpoints_raw.and_then(Value::as_array);
    if touchpoints_array.is_none()
        || touchpoints_array.is_some_and(|entries| {
            entries.len() > PlanningLimits::MAX_TOUCHPOINTS
        })
    {
        reasons.push("touchpoints must be a bounded array.".to_owned());
    } else if let Some(entries) = touchpoints_array {
        for entry in entries {
            if !is_plain_object(Some(entry)) {
                reasons.push("Each touchpoint must be an object.".to_owned());
                continue;
            }
            let touchpoint = entry.as_object().expect("checked object");
            let id = touchpoint.get("id").and_then(Value::as_str);
            let path = touchpoint.get("path").and_then(Value::as_str);
            let confidence =
                touchpoint.get("confidence").and_then(Value::as_str);
            if !non_empty_string(touchpoint.get("id"))
                || !is_valid_plan_element_id(id.unwrap_or_default())
            {
                reasons
                    .push("Each touchpoint requires a valid id.".to_owned());
                continue;
            }
            let id_text = id.expect("checked id").to_owned();
            if touchpoint_ids.contains(&id_text) {
                reasons.push(format!("Duplicate touchpoint id: {id_text}"));
                continue;
            }
            touchpoint_ids.push(id_text.clone());
            if !non_empty_string(touchpoint.get("path"))
                || !bounded(
                    path.unwrap_or_default(),
                    PlanningLimits::MAX_PATH_BYTES,
                )
            {
                reasons.push(format!(
                    "Touchpoint {id_text} requires a bounded path."
                ));
                continue;
            }
            let path_text = path.expect("checked path").to_owned();
            let confidence_kind = match confidence {
                Some("verified") => Some(TouchpointConfidenceKind::Verified),
                Some("candidate") => Some(TouchpointConfidenceKind::Candidate),
                _ => None,
            };
            let Some(confidence_kind) = confidence_kind else {
                reasons.push(format!(
                    "Touchpoint {id_text} requires confidence verified or candidate."
                ));
                continue;
            };
            let allow_glob =
                confidence_kind == TouchpointConfidenceKind::Candidate;
            if !is_safe_plan_path(&path_text, allow_glob) {
                reasons.push(format!(
                    "Touchpoint {id_text} path {} is not a safe workspace-relative path.",
                    crate::identity::json_escape(&path_text)
                ));
            }
            let revision = touchpoint.get("revision");
            let evidence = touchpoint.get("evidence");
            let note = touchpoint.get("note");
            let mut revision_text: Option<String> = None;
            if confidence_kind == TouchpointConfidenceKind::Verified {
                if !non_empty_string(revision)
                    || !is_valid_revision_handle(
                        revision.and_then(Value::as_str).unwrap_or_default(),
                    )
                {
                    reasons.push(format!(
                        "Verified touchpoint {id_text} requires the exact inspected workspace revision handle (rev_ + 32 hex)."
                    ));
                } else if !bounded(
                    revision.and_then(Value::as_str).unwrap_or_default(),
                    PlanningLimits::MAX_REVISION_BYTES,
                ) {
                    reasons.push(format!(
                        "Touchpoint {id_text} revision exceeds the byte bound."
                    ));
                } else {
                    revision_text = Some(
                        revision
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    );
                }
            } else if let Some(revision_value) = revision {
                if !non_empty_string(Some(revision_value)) {
                    reasons.push(format!(
                        "Touchpoint {id_text} revision must be a string when present."
                    ));
                } else {
                    revision_text = revision_value.as_str().map(str::to_owned);
                }
            }
            let mut evidence_text: Option<String> = None;
            if let Some(evidence_value) = evidence {
                if !non_empty_string(Some(evidence_value))
                    || !is_evidence_reference(
                        evidence_value.as_str().unwrap_or_default(),
                    )
                {
                    reasons.push(format!(
                        "Touchpoint {id_text} evidence must be a bounded kind:ref reference (read|api|reference|research|knowledge|instruction)."
                    ));
                } else {
                    evidence_text = evidence_value.as_str().map(str::to_owned);
                }
            }
            let mut note_text: Option<String> = None;
            if let Some(note_value) = note {
                if !non_empty_string(Some(note_value))
                    || !bounded(
                        note_value.as_str().unwrap_or_default(),
                        PlanningLimits::MAX_NOTE_BYTES,
                    )
                {
                    reasons.push(format!(
                        "Touchpoint {id_text} note exceeds the byte bound."
                    ));
                } else {
                    note_text = note_value.as_str().map(str::to_owned);
                }
            }
            touchpoints.push(PlanTouchpoint {
                id: id_text,
                path: path_text,
                confidence: match confidence_kind {
                    TouchpointConfidenceKind::Verified => {
                        super::model::TouchpointConfidence::Verified
                    }
                    TouchpointConfidenceKind::Candidate => {
                        super::model::TouchpointConfidence::Candidate
                    }
                },
                revision: revision_text,
                evidence: evidence_text,
                note: note_text,
            });
        }
    }

    let constraints_raw = object.get("constraints");
    let mut constraints: Vec<PlanConstraint> = Vec::new();
    let mut constraint_ids: Vec<String> = Vec::new();
    if let Some(constraints_value) = constraints_raw {
        let array = constraints_value.as_array();
        if array.is_none()
            || array.is_some_and(|entries| {
                entries.len() > PlanningLimits::MAX_CONSTRAINTS
            })
        {
            reasons.push("constraints must be a bounded array.".to_owned());
        } else if let Some(entries) = array {
            for entry in entries {
                if !is_plain_object(Some(entry)) {
                    reasons
                        .push("Each constraint must be an object.".to_owned());
                    continue;
                }
                let constraint = entry.as_object().expect("checked object");
                let id = constraint.get("id").and_then(Value::as_str);
                let description =
                    constraint.get("description").and_then(Value::as_str);
                if !non_empty_string(constraint.get("id"))
                    || !is_valid_plan_element_id(id.unwrap_or_default())
                {
                    reasons.push(
                        "Each constraint requires a valid id.".to_owned(),
                    );
                    continue;
                }
                let id_text = id.expect("checked id").to_owned();
                if constraint_ids.contains(&id_text) {
                    reasons
                        .push(format!("Duplicate constraint id: {id_text}"));
                    continue;
                }
                constraint_ids.push(id_text.clone());
                if !non_empty_string(constraint.get("description"))
                    || !bounded(
                        description.unwrap_or_default(),
                        PlanningLimits::MAX_STATEMENT_BYTES,
                    )
                {
                    reasons.push(format!(
                        "Constraint {id_text} requires a bounded description."
                    ));
                } else if let Some(claim) =
                    reject_plan_policy_claims(description.expect("checked"))
                {
                    reasons.push(claim.to_owned());
                } else {
                    constraints.push(PlanConstraint {
                        id: id_text,
                        description: description.expect("checked").to_owned(),
                    });
                }
            }
        }
    }

    let risks_raw = object.get("risks");
    let mut risks: Vec<PlanRisk> = Vec::new();
    let mut risk_ids: Vec<String> = Vec::new();
    if let Some(risks_value) = risks_raw {
        let array = risks_value.as_array();
        if array.is_none()
            || array.is_some_and(|entries| {
                entries.len() > PlanningLimits::MAX_RISKS
            })
        {
            reasons.push("risks must be a bounded array.".to_owned());
        } else if let Some(entries) = array {
            for entry in entries {
                if !is_plain_object(Some(entry)) {
                    reasons.push("Each risk must be an object.".to_owned());
                    continue;
                }
                let risk = entry.as_object().expect("checked object");
                let id = risk.get("id").and_then(Value::as_str);
                let severity = risk.get("severity").and_then(Value::as_str);
                let description =
                    risk.get("description").and_then(Value::as_str);
                if !non_empty_string(risk.get("id"))
                    || !is_valid_plan_element_id(id.unwrap_or_default())
                {
                    reasons.push("Each risk requires a valid id.".to_owned());
                    continue;
                }
                let id_text = id.expect("checked id").to_owned();
                if risk_ids.contains(&id_text) {
                    reasons.push(format!("Duplicate risk id: {id_text}"));
                    continue;
                }
                risk_ids.push(id_text.clone());
                let severity_kind = match severity {
                    Some("low") => PlanRiskSeverity::Low,
                    Some("medium") => PlanRiskSeverity::Medium,
                    Some("high") => PlanRiskSeverity::High,
                    _ => {
                        reasons.push(format!(
                            "Risk {id_text} requires severity low, medium, or high."
                        ));
                        PlanRiskSeverity::Low
                    }
                };
                if !non_empty_string(risk.get("description"))
                    || !bounded(
                        description.unwrap_or_default(),
                        PlanningLimits::MAX_STATEMENT_BYTES,
                    )
                {
                    reasons.push(format!(
                        "Risk {id_text} requires a bounded description."
                    ));
                } else {
                    risks.push(PlanRisk {
                        id: id_text,
                        severity: severity_kind,
                        description: description.expect("checked").to_owned(),
                    });
                }
            }
        }
    }

    let steps_raw = object.get("steps");
    let mut steps: Vec<PlanStep> = Vec::new();
    let mut step_ids: Vec<String> = Vec::new();
    let steps_array = steps_raw.and_then(Value::as_array);
    if steps_array.is_none_or(|entries| entries.is_empty())
        || steps_array.is_some_and(|entries| entries.len() > max_steps)
    {
        reasons.push(format!(
            "steps must be a non-empty bounded array (at most {max_steps} for {depth_label} plans)."
        ));
    } else {
        for entry in steps_array.expect("checked array") {
            if !is_plain_object(Some(entry)) {
                reasons.push("Each step must be an object.".to_owned());
                continue;
            }
            let step = entry.as_object().expect("checked object");
            let id = step.get("id").and_then(Value::as_str);
            let title = step.get("title").and_then(Value::as_str);
            if !non_empty_string(step.get("id"))
                || !is_valid_plan_element_id(id.unwrap_or_default())
            {
                reasons.push("Each step requires a valid id.".to_owned());
                continue;
            }
            let id_text = id.expect("checked id").to_owned();
            if step_ids.contains(&id_text) {
                reasons.push(format!("Duplicate step id: {id_text}"));
                continue;
            }
            step_ids.push(id_text.clone());
            let mut title_text: Option<String> = None;
            if !non_empty_string(step.get("title"))
                || !bounded(
                    title.unwrap_or_default(),
                    PlanningLimits::MAX_STEP_TITLE_BYTES,
                )
            {
                reasons
                    .push(format!("Step {id_text} requires a bounded title."));
            } else if let Some(claim) =
                reject_plan_policy_claims(title.expect("checked"))
            {
                reasons.push(claim.to_owned());
            } else {
                title_text = Some(title.expect("checked").to_owned());
            }
            let mut description_text: Option<String> = None;
            if let Some(description) = step.get("description") {
                if !non_empty_string(Some(description))
                    || !bounded(
                        description.as_str().unwrap_or_default(),
                        PlanningLimits::MAX_STEP_DESCRIPTION_BYTES,
                    )
                {
                    reasons.push(format!(
                        "Step {id_text} description exceeds the byte bound."
                    ));
                } else {
                    description_text = description.as_str().map(str::to_owned);
                }
            }
            let expected = step.get("expectedTouchpoints");
            let mut expected_touchpoints: Vec<String> = Vec::new();
            if expected.and_then(Value::as_array).is_none_or(|entries| {
                entries.len()
                    > PlanningLimits::MAX_EXPECTED_TOUCHPOINTS_PER_STEP
                    || entries.iter().any(|item| !item.is_string())
            }) {
                reasons.push(format!(
                    "Step {id_text} expectedTouchpoints must be a bounded id array."
                ));
            } else {
                let entries =
                    expected.and_then(Value::as_array).expect("checked");
                let unknown: Vec<&str> = entries
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|reference| {
                        !touchpoint_ids.iter().any(|known| known == reference)
                    })
                    .collect();
                for reference in unknown {
                    reasons.push(format!(
                        "Step {id_text} references unknown touchpoint {}.",
                        crate::identity::json_escape(reference)
                    ));
                }
                expected_touchpoints = entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
            }
            let verification = step.get("verification");
            let mut verification_refs: Option<Vec<String>> = None;
            if let Some(verification_value) = verification {
                let entries = verification_value.as_array();
                if entries.is_none_or(|items| {
                    items.len()
                        > PlanningLimits::MAX_VERIFICATION_REFS_PER_STEP
                        || items.iter().any(|item| !item.is_string())
                }) {
                    reasons.push(format!(
                        "Step {id_text} verification must be a bounded criterion-id array."
                    ));
                } else {
                    let items = entries.expect("checked array");
                    for reference in items.iter().filter_map(Value::as_str) {
                        if !context
                            .contract
                            .acceptance_criteria()
                            .iter()
                            .any(|criterion| criterion.id() == reference)
                        {
                            reasons.push(format!(
                                "Step {id_text} references unknown acceptance criterion {}.",
                                crate::identity::json_escape(reference)
                            ));
                        }
                    }
                    verification_refs = Some(
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                    );
                }
            }
            if let Some(title_text) = title_text {
                steps.push(PlanStep {
                    id: id_text,
                    title: title_text,
                    description: description_text,
                    expected_touchpoints,
                    verification: verification_refs,
                });
            }
        }
    }

    let validation_raw = object.get("validation");
    let mut validation: Option<PlanValidationStrategy> = None;
    if !is_plain_object(validation_raw) {
        reasons.push("A plan requires a validation object.".to_owned());
    } else {
        let validation_object =
            validation_raw.and_then(Value::as_object).expect("checked");
        let checks = validation_object.get("checks");
        if !is_string_array(checks)
            || checks
                .and_then(Value::as_array)
                .is_none_or(|entries| entries.is_empty())
            || checks.and_then(Value::as_array).is_some_and(|entries| {
                entries.len() > PlanningLimits::MAX_VALIDATION_CHECKS
            })
        {
            reasons.push(
                "validation.checks must be a non-empty bounded string array."
                    .to_owned(),
            );
        } else {
            let entries = checks.and_then(Value::as_array).expect("checked");
            if entries.iter().any(|entry| {
                !bounded(
                    entry.as_str().unwrap_or_default(),
                    PlanningLimits::MAX_STATEMENT_BYTES,
                )
            }) {
                reasons.push(
                    "A validation check exceeds the statement byte bound."
                        .to_owned(),
                );
            }
        }
        let requirements = validation_object.get("requirements");
        let mut requirements_list: Option<Vec<String>> = None;
        if let Some(requirements_value) = requirements {
            if !is_string_array(Some(requirements_value))
                || requirements_value.as_array().is_some_and(|entries| {
                    entries.len() > PlanningLimits::MAX_VALIDATION_REQUIREMENTS
                })
            {
                reasons.push(
                    "validation.requirements must be a bounded string array."
                        .to_owned(),
                );
            } else {
                let entries = requirements_value.as_array().expect("checked");
                let mut oversized = false;
                for requirement in entries.iter().filter_map(Value::as_str) {
                    if !bounded(
                        requirement,
                        PlanningLimits::MAX_STATEMENT_BYTES,
                    ) {
                        reasons.push(
                            "A validation requirement exceeds the statement byte bound."
                                .to_owned(),
                        );
                        oversized = true;
                        break;
                    }
                    if let Some(claim) = reject_plan_policy_claims(requirement)
                    {
                        reasons.push(claim.to_owned());
                    }
                }
                if !oversized {
                    requirements_list = Some(
                        entries
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                    );
                }
            }
        }
        if validation.is_none() {
            validation = Some(PlanValidationStrategy {
                checks: string_list(checks),
                requirements: requirements_list,
            });
        }
    }

    let rollback_raw = object.get("rollback");
    let mut rollback: Option<super::model::PlanRollbackStrategy> = None;
    if let Some(rollback_value) = rollback_raw {
        if !is_plain_object(Some(rollback_value)) {
            reasons.push("rollback requires an object.".to_owned());
        } else {
            let description =
                rollback_value.get("description").and_then(Value::as_str);
            if !non_empty_string(rollback_value.get("description"))
                || !bounded(
                    description.unwrap_or_default(),
                    PlanningLimits::MAX_ROLLBACK_BYTES,
                )
            {
                reasons.push(
                    "rollback requires a bounded description.".to_owned(),
                );
            } else {
                rollback = Some(super::model::PlanRollbackStrategy {
                    description: description.expect("checked").to_owned(),
                });
            }
        }
    }

    let rationale_raw = object.get("rationale");
    let mut rationale: Option<String> = None;
    if let Some(rationale_value) = rationale_raw {
        if !non_empty_string(Some(rationale_value))
            || !bounded(
                rationale_value.as_str().unwrap_or_default(),
                PlanningLimits::MAX_RATIONALE_BYTES,
            )
        {
            reasons.push("rationale must be a bounded string.".to_owned());
        } else {
            rationale = rationale_value.as_str().map(str::to_owned);
        }
    }

    // Aggregate content byte bound: an enormous plan is rejected, never
    // injected into context.
    if reasons.0.is_empty() {
        let serialized = aggregate_candidate_bytes(object);
        if serialized > PlanningLimits::MAX_PLAN_CONTENT_BYTES {
            reasons.push(format!(
                "The plan exceeds the {}-byte content bound.",
                PlanningLimits::MAX_PLAN_CONTENT_BYTES
            ));
        }
    }

    if !reasons.0.is_empty() {
        reasons.0.truncate(8);
        return PlanCandidateResult::Rejected(reasons.0);
    }

    let Some(objective) = objective else {
        return PlanCandidateResult::Rejected(vec![
            "A plan requires a non-empty objective.".to_owned(),
        ]);
    };
    let Some(scope) = scope else {
        return PlanCandidateResult::Rejected(vec![
            "A plan requires a scope object.".to_owned(),
        ]);
    };
    let Some(validation) = validation else {
        return PlanCandidateResult::Rejected(vec![
            "A plan requires a validation object.".to_owned(),
        ]);
    };
    PlanCandidateResult::Ok(Box::new(TaskPlanContent {
        objective,
        scope,
        non_goals,
        touchpoints,
        constraints,
        risks,
        steps,
        validation,
        rollback,
        rationale,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchpointConfidenceKind {
    Verified,
    Candidate,
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Byte length of the aggregate serialized candidate the reference
/// measures (`JSON.stringify` over the fixed field set).
fn aggregate_candidate_bytes(
    object: &serde_json::Map<String, Value>,
) -> usize {
    let mut aggregate = serde_json::Map::new();
    for key in [
        "objective",
        "scope",
        "nonGoals",
        "touchpoints",
        "constraints",
        "risks",
        "steps",
        "validation",
        "rollback",
        "rationale",
    ] {
        aggregate.insert(
            key.to_owned(),
            object.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    crate::identity::canonical_json_value(&Value::Object(aggregate)).len()
}

/// Extract a JSON object from planner text (tolerant of one code fence).
pub fn extract_plan_candidate_json(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    let fenced = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
        .map(|inner| {
            inner
                .trim()
                .trim_start_matches("json")
                .trim_start()
                .trim_end()
                .to_owned()
        });
    let candidate = fenced.as_deref().unwrap_or(trimmed);
    match serde_json::from_str::<Value>(candidate) {
        Ok(parsed) if parsed.is_object() => Some(parsed),
        _ => None,
    }
}
