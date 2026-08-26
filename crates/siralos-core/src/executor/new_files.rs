//! New-file discipline and scope evaluation (harness context
//! optimization, ADR 0023; Stage 3R R13.4).
//!
//! Proliferation signals are DETERMINISTIC REVIEW SIGNALS, not hard
//! rules: they flag suspicious expansion and feed existing quality and
//! review findings. They never block legitimate discovery — expansion
//! with evidence is recorded and continues.

/// Recorded rationale for a new production file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFileRationale {
    /// Workspace-relative path.
    pub path: String,
    /// Why a new file (and not an existing owner) is justified.
    pub reason: String,
    /// Existing owner modules inspected before creating the file.
    pub existing_owners_inspected: Vec<String>,
}

/// One proliferation signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProliferationSignal {
    /// Stable signal id.
    pub id: String,
    /// Exact signal message.
    pub message: String,
}

/// Scope-diff classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeDiffClassification {
    /// Matched a planned path.
    Expected,
    /// Recorded rationale present.
    JustifiedExpansion,
    /// Neither planned nor rationalized.
    UnexplainedExpansion,
}

impl ScopeDiffClassification {
    /// Exact reference spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ScopeDiffClassification::Expected => "expected",
            ScopeDiffClassification::JustifiedExpansion => {
                "justified expansion"
            }
            ScopeDiffClassification::UnexplainedExpansion => {
                "unexplained expansion"
            }
        }
    }
}

/// One scope-diff entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDiffEntry {
    /// Changed path.
    pub path: String,
    /// Classification.
    pub classification: ScopeDiffClassification,
    /// Present for justified expansions: the recorded rationale.
    pub rationale: Option<String>,
}

/// Full scope-diff report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDiffReport {
    /// Per-path classifications in input order.
    pub entries: Vec<ScopeDiffEntry>,
    /// Unexplained paths in input order.
    pub unexplained: Vec<String>,
}

/// Host-owned hard bounds.
pub struct NewFileDisciplineLimits;

impl NewFileDisciplineLimits {
    /// Maximum rationales.
    pub const MAX_RATIONALES: usize = 64;
    /// Maximum path bytes.
    pub const MAX_PATH_BYTES: usize = 1024;
    /// Maximum reason bytes.
    pub const MAX_REASON_BYTES: usize = 1024;
    /// Maximum owners per rationale.
    pub const MAX_OWNERS: usize = 8;
    /// Maximum owner-name bytes.
    pub const MAX_OWNER_BYTES: usize = 256;
    /// Maximum signals.
    pub const MAX_SIGNALS: usize = 32;
    /// Maximum signal-message bytes.
    pub const MAX_SIGNAL_MESSAGE_BYTES: usize = 512;
    /// Maximum changed paths per diff evaluation.
    pub const MAX_DIFF_ENTRIES: usize = 256;
}

/// Deterministic proliferation thresholds.
pub struct ProliferationHeuristics;

impl ProliferationHeuristics {
    /// More than this many new production files is suspicious.
    pub const MAX_NEW_PRODUCTION_FILES: usize = 5;
    /// More than this many tiny helper files is suspicious.
    pub const MAX_TINY_HELPER_FILES: usize = 2;
    /// Files below this size count as tiny helpers.
    pub const TINY_FILE_BYTES: usize = 256;
    /// More than this many changed files outside scope is suspicious.
    pub const MAX_CHANGED_OUTSIDE_SCOPE: usize = 3;
}

fn validate_path(path: &str) -> Result<String, String> {
    let text = path.trim();
    if text.is_empty() {
        return Err("A file path must not be empty.".to_owned());
    }
    if text.len() > NewFileDisciplineLimits::MAX_PATH_BYTES {
        return Err(format!(
            "A file path exceeds {} UTF-8 bytes.",
            NewFileDisciplineLimits::MAX_PATH_BYTES
        ));
    }
    Ok(text.to_owned())
}

fn validate_reason(reason: &str) -> Result<String, String> {
    let text = reason.trim();
    if text.is_empty() {
        return Err("A new-file rationale requires a reason.".to_owned());
    }
    if text.len() > NewFileDisciplineLimits::MAX_REASON_BYTES {
        return Err(format!(
            "A new-file rationale reason exceeds {} UTF-8 bytes.",
            NewFileDisciplineLimits::MAX_REASON_BYTES
        ));
    }
    Ok(text.to_owned())
}

fn validate_owners(owners: &[String]) -> Result<Vec<String>, String> {
    if owners.len() > NewFileDisciplineLimits::MAX_OWNERS {
        return Err(format!(
            "A new-file rationale names at most {} owners.",
            NewFileDisciplineLimits::MAX_OWNERS
        ));
    }
    owners
        .iter()
        .map(|owner| {
            let text = owner.trim();
            if text.is_empty() {
                return Err(
                    "An inspected-owner name must not be empty.".to_owned()
                );
            }
            if text.len() > NewFileDisciplineLimits::MAX_OWNER_BYTES {
                return Err(format!(
                    "An inspected-owner name exceeds {} UTF-8 bytes.",
                    NewFileDisciplineLimits::MAX_OWNER_BYTES
                ));
            }
            Ok(text.to_owned())
        })
        .collect()
}

/// Record the host-visible rationale for a new production file.
///
/// # Errors
///
/// Exact reference messages.
pub fn create_new_file_rationale(
    path: &str,
    reason: &str,
    existing_owners_inspected: &[String],
) -> Result<NewFileRationale, String> {
    Ok(NewFileRationale {
        path: validate_path(path)?,
        reason: validate_reason(reason)?,
        existing_owners_inspected: validate_owners(existing_owners_inspected)?,
    })
}

fn directory_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(index) => &path[..index],
        None => ".",
    }
}

/// Host-observed facts about one new production file.
pub struct NewProductionFile {
    /// New-file path.
    pub path: String,
    /// Observed size in bytes.
    pub size_bytes: usize,
}

/// Proliferation-signal input.
pub struct DetectProliferationSignalsInput<'a> {
    /// New production files with sizes.
    pub new_production_files: &'a [NewProductionFile],
    /// Planned/expected workspace-relative paths (globs allowed).
    pub planned_paths: &'a [String],
    /// Directories known to already exist.
    pub known_directories: &'a [String],
}

/// Deterministic proliferation signals over host-observed facts.
///
/// # Errors
///
/// Path validation failures.
pub fn detect_proliferation_signals(
    input: &DetectProliferationSignalsInput<'_>,
) -> Result<Vec<ProliferationSignal>, String> {
    let mut signals: Vec<ProliferationSignal> = Vec::new();
    let files = input.new_production_files;
    if files.len() > ProliferationHeuristics::MAX_NEW_PRODUCTION_FILES {
        signals.push(ProliferationSignal {
            id: "PROLIF.MANY_NEW_FILES".to_owned(),
            message: format!(
                "{} new production files in one task exceeds the {}-file review signal; confirm each has a recorded rationale.",
                files.len(),
                ProliferationHeuristics::MAX_NEW_PRODUCTION_FILES
            ),
        });
    }
    let tiny_count = files
        .iter()
        .filter(|file| {
            file.size_bytes < ProliferationHeuristics::TINY_FILE_BYTES
        })
        .count();
    if tiny_count > ProliferationHeuristics::MAX_TINY_HELPER_FILES {
        signals.push(ProliferationSignal {
            id: "PROLIF.TINY_HELPERS".to_owned(),
            message: format!(
                "{tiny_count} tiny helper files (under {} bytes); consider extending an existing owner.",
                ProliferationHeuristics::TINY_FILE_BYTES
            ),
        });
    }
    let mut new_directories: Vec<String> = Vec::new();
    for file in files {
        let directory = directory_of(&file.path).to_owned();
        if directory != "."
            && !input.known_directories.contains(&directory)
            && !new_directories.contains(&directory)
        {
            new_directories.push(directory);
        }
    }
    if !new_directories.is_empty() {
        signals.push(ProliferationSignal {
            id: "PROLIF.NEW_DIRECTORY".to_owned(),
            message: format!(
                "new directories created for this change: {}; verify a distinct responsibility boundary exists.",
                new_directories.join(", ")
            ),
        });
    }
    let outside_scope_count = files
        .iter()
        .filter(|file| {
            !input
                .planned_paths
                .iter()
                .any(|planned| path_matches_pattern(&file.path, planned))
        })
        .count();
    if outside_scope_count > ProliferationHeuristics::MAX_CHANGED_OUTSIDE_SCOPE
    {
        signals.push(ProliferationSignal {
            id: "PROLIF.OUTSIDE_SCOPE".to_owned(),
            message: format!(
                "{outside_scope_count} new files do not match any planned path; record evidence and promote through the scope before treating them as expected."
            ),
        });
    }
    signals.truncate(NewFileDisciplineLimits::MAX_SIGNALS);
    Ok(signals
        .into_iter()
        .map(|signal| {
            let message = if signal.message.len()
                > NewFileDisciplineLimits::MAX_SIGNAL_MESSAGE_BYTES
            {
                format!(
                    "{}\u{2026}",
                    &signal.message[..240.min(signal.message.len())]
                )
            } else {
                signal.message
            };
            ProliferationSignal { id: signal.id, message }
        })
        .collect())
}

/// Minimal deterministic glob support for workspace paths: `*` matches
/// within one path segment, `**` matches across segments; a pattern with
/// no wildcards matches the exact path only. This mirrors the reference's
/// generated-regex semantics including its `**` edge behavior.
pub fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return path == pattern;
    }
    #[derive(Debug, Clone)]
    enum Token {
        Lit(char),
        StarNoSlash,
        DoubleStar,
    }
    let mut tokens: Vec<Token> = Vec::new();
    for (index, segment) in pattern.split('/').enumerate() {
        if index > 0 {
            tokens.push(Token::Lit('/'));
        }
        if segment == "**" {
            tokens.push(Token::DoubleStar);
            continue;
        }
        for character in segment.chars() {
            if character == '*' {
                tokens.push(Token::StarNoSlash);
            } else {
                tokens.push(Token::Lit(character));
            }
        }
    }
    let path_chars: Vec<char> = path.chars().collect();
    // DoubleStar validity: ((?:[^/]+/)*)[^/]* — slashes allowed only as
    // chunk terminators of non-empty chunks.
    fn double_star_valid(chars: &[char]) -> bool {
        if chars.is_empty() {
            return true;
        }
        if chars[0] == '/' {
            return false;
        }
        let bytes_like = chars;
        for window in bytes_like.windows(2) {
            if window[0] == '/' && window[1] == '/' {
                return false;
            }
        }
        true
    }
    fn match_from(
        tokens: &[Token],
        token_index: usize,
        chars: &[char],
        char_index: usize,
    ) -> bool {
        if token_index == tokens.len() {
            return char_index == chars.len();
        }
        match &tokens[token_index] {
            Token::Lit(expected) => {
                chars.get(char_index) == Some(expected)
                    && match_from(
                        tokens,
                        token_index + 1,
                        chars,
                        char_index + 1,
                    )
            }
            Token::StarNoSlash => {
                let mut length = chars.len() - char_index;
                loop {
                    if chars[char_index..char_index + length]
                        .iter()
                        .all(|character| *character != '/')
                        && match_from(
                            tokens,
                            token_index + 1,
                            chars,
                            char_index + length,
                        )
                    {
                        return true;
                    }
                    if length == 0 {
                        return false;
                    }
                    length -= 1;
                }
            }
            Token::DoubleStar => {
                let mut end = char_index;
                loop {
                    if double_star_valid(&chars[char_index..end])
                        && match_from(tokens, token_index + 1, chars, end)
                    {
                        return true;
                    }
                    if end >= chars.len() {
                        return false;
                    }
                    end += 1;
                }
            }
        }
    }
    match_from(&tokens, 0, &path_chars, 0)
}

/// Scope-diff evaluation input.
pub struct EvaluateScopeDiffInput<'a> {
    /// Planned/expected workspace-relative paths (touchpoints, globs).
    pub planned_paths: &'a [String],
    /// Actual changed files.
    pub changed_paths: &'a [String],
    /// Rationales recorded during execution.
    pub rationales: &'a [NewFileRationale],
}

/// Compare the planned scope with actual changed files.
///
/// # Errors
///
/// Too many changed paths or invalid path text.
pub fn evaluate_scope_diff(
    input: &EvaluateScopeDiffInput<'_>,
) -> Result<ScopeDiffReport, String> {
    if input.changed_paths.len() > NewFileDisciplineLimits::MAX_DIFF_ENTRIES {
        return Err(format!(
            "Scope evaluation accepts at most {} changed paths.",
            NewFileDisciplineLimits::MAX_DIFF_ENTRIES
        ));
    }
    let mut entries = Vec::new();
    let mut unexplained = Vec::new();
    for path in input.changed_paths {
        let normalized = validate_path(path)?;
        let planned = input
            .planned_paths
            .iter()
            .any(|pattern| path_matches_pattern(&normalized, pattern));
        if planned {
            entries.push(ScopeDiffEntry {
                path: normalized,
                classification: ScopeDiffClassification::Expected,
                rationale: None,
            });
        } else {
            let rationale = input
                .rationales
                .iter()
                .find(|rationale| rationale.path == normalized)
                .map(|rationale| rationale.reason.clone());
            if let Some(rationale) = rationale {
                entries.push(ScopeDiffEntry {
                    path: normalized,
                    classification:
                        ScopeDiffClassification::JustifiedExpansion,
                    rationale: Some(rationale),
                });
            } else {
                entries.push(ScopeDiffEntry {
                    path: normalized.clone(),
                    classification:
                        ScopeDiffClassification::UnexplainedExpansion,
                    rationale: None,
                });
                unexplained.push(normalized);
            }
        }
    }
    Ok(ScopeDiffReport { entries, unexplained })
}
