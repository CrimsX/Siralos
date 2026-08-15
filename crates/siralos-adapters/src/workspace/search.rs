//! Deterministic bounded workspace search (R4, reference
//! `workspace.search`).
//!
//! A direct bounded traversal with the reference budgets, exclusions,
//! and stable result ordering. Filesystem enumeration order is never
//! semantic ordering: names are sorted per directory and matches are
//! sorted by (path, line, column) at the authoritative boundary. No
//! index, cache, or language intelligence is introduced.

use crate::workspace::fs::{
    BoundedFileRead, DEFAULT_EXCLUDED_DIRECTORIES, decode_utf8,
    fold_path_component, looks_binary, read_complete_file_bounded,
    split_into_lines, utf16_index_of, utf16_slice,
};
use crate::workspace::resolve::resolve_workspace_path;

use siralos_core::workspace::bounds::WorkspaceLimits;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Why a search stopped before exhausting the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationReason {
    /// The directory-visit budget was exceeded.
    DirectoryBudget,
    /// The entry-examined budget was exceeded.
    EntryBudget,
    /// The files-considered (lstat) budget was exceeded.
    FileBudget,
    /// The files-scanned budget was exceeded.
    ScanBudget,
    /// The input-byte budget was exceeded.
    InputBudget,
    /// The output-byte budget was exceeded.
    OutputBudget,
    /// The wall-clock search budget was exceeded.
    TimeBudget,
    /// The match-count limit was reached.
    MatchLimit,
    /// The maximum directory depth was exceeded.
    DepthBudget,
}

impl TruncationReason {
    /// The canonical protocol string for this reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectoryBudget => "directory_budget",
            Self::EntryBudget => "entry_budget",
            Self::FileBudget => "file_budget",
            Self::ScanBudget => "scan_budget",
            Self::InputBudget => "input_budget",
            Self::OutputBudget => "output_budget",
            Self::TimeBudget => "time_budget",
            Self::MatchLimit => "match_limit",
            Self::DepthBudget => "depth_budget",
        }
    }
}

/// One search match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Workspace-relative file path.
    pub path: String,
    /// One-based line number.
    pub line: u64,
    /// One-based UTF-16 code-unit column of the first occurrence.
    pub column: u64,
    /// The matched line (bounded to the line-length budget).
    pub text: String,
}

/// Outcome of one bounded search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOutcome {
    /// The input was invalid.
    InvalidInput {
        /// Stable validation message.
        message: String,
    },
    /// The requested path was rejected (denied).
    Denied {
        /// Stable rejection message.
        message: String,
    },
    /// The operation was cancelled.
    Cancelled,
    /// Successful bounded search.
    Success {
        /// The literal query.
        query: String,
        /// Canonical workspace-relative search root.
        path: String,
        /// Sorted bounded matches.
        matches: Vec<SearchMatch>,
        /// Files whose bytes were scanned.
        scanned_files: u64,
        /// Files skipped (links, special, oversized, binary, unreadable).
        skipped_files: u64,
        /// True when the traversal stopped early.
        truncated: bool,
        /// Why the traversal stopped early, when truncated.
        truncation_reason: Option<TruncationReason>,
    },
}

/// Parsed search request (mirrors the reference input schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchInput {
    /// Literal case-sensitive query.
    pub query: String,
    /// Search root (default `.`).
    pub path: String,
    /// Maximum matches (capped at the reference match bound).
    pub max_results: usize,
}

/// Parse and validate a search request.
pub fn parse_search_input(
    input: &serde_json::Value,
    limits: &WorkspaceLimits,
) -> Result<SearchInput, String> {
    let object = match input {
        serde_json::Value::Object(object) => object,
        _ => return Err("Tool input must be a JSON object.".to_owned()),
    };
    let query = match object.get("query") {
        Some(serde_json::Value::String(value)) if !value.is_empty() => {
            value.clone()
        }
        Some(serde_json::Value::String(_)) => {
            return Err("\"query\" is required.".to_owned());
        }
        Some(_) => return Err("\"query\" must be a string.".to_owned()),
        None => return Err("\"query\" is required.".to_owned()),
    };
    let path = match object.get("path") {
        None => ".".to_owned(),
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(_) => return Err("\"path\" must be a string.".to_owned()),
    };
    let requested = match object.get("maxResults") {
        None => limits.max_search_matches,
        Some(serde_json::Value::Number(number)) => {
            let value =
                number.as_u64().and_then(|value| usize::try_from(value).ok());
            match value.filter(|value| *value >= 1) {
                Some(value) => value.min(limits.max_search_matches),
                None => {
                    return Err("\"maxResults\" must be a positive integer."
                        .to_owned());
                }
            }
        }
        Some(_) => {
            return Err(
                "\"maxResults\" must be a positive integer.".to_owned()
            );
        }
    };
    Ok(SearchInput { query, path, max_results: requested })
}
/// Search the workspace with the reference budgets and semantics.
pub fn search(
    root: &Path,
    input: &SearchInput,
    limits: &WorkspaceLimits,
    cancelled: bool,
) -> SearchOutcome {
    let resolved = match resolve_workspace_path(root, &input.path) {
        Ok(resolved) => resolved,
        Err(rejection) => {
            return SearchOutcome::Denied { message: rejection.to_string() };
        }
    };
    if let Some(component) = crate::workspace::list::excluded_component(
        &resolved.workspace_relative_path,
        &DEFAULT_EXCLUDED_DIRECTORIES,
    ) {
        return SearchOutcome::Denied {
            message: format!(
                "Path is inside the excluded directory {component}."
            ),
        };
    }
    if cancelled {
        return SearchOutcome::Cancelled;
    }
    let fold = fold_component_platform();
    let mut matches: Vec<SearchMatch> = Vec::new();
    let mut scanned_files: u64 = 0;
    let mut skipped_files: u64 = 0;
    let mut directories_visited: u64 = 0;
    let mut entries_examined: u64 = 0;
    let mut files_considered: u64 = 0;
    let mut input_bytes: u64 = 0;
    let mut output_bytes: u64 = 0;
    let mut truncated = false;
    let mut truncation_reason: Option<TruncationReason> = None;
    let deadline =
        Instant::now() + Duration::from_millis(limits.max_search_duration_ms);
    macro_rules! stop_search {
        ($reason:expr) => {
            truncated = true;
            truncation_reason = Some($reason);
            matches.sort_by(compare_matches);
            return SearchOutcome::Success {
                query: input.query.clone(),
                path: resolved.workspace_relative_path.clone(),
                matches,
                scanned_files,
                skipped_files,
                truncated,
                truncation_reason,
            };
        };
    }
    let mut pending: Vec<PendingDirectory> = vec![PendingDirectory {
        absolute: resolved.absolute_path.clone(),
        relative: resolved.workspace_relative_path.clone(),
        depth: 0,
    }];
    while let Some(directory) = pending.pop() {
        if cancelled {
            return SearchOutcome::Cancelled;
        }
        if Instant::now() >= deadline {
            stop_search!(TruncationReason::TimeBudget);
        }
        directories_visited += 1;
        if directories_visited > limits.max_search_directories as u64 {
            stop_search!(TruncationReason::DirectoryBudget);
        }
        if directory.depth > limits.max_search_depth {
            stop_search!(TruncationReason::DepthBudget);
        }
        let mut names: Vec<String> = Vec::new();
        let entries = match std::fs::read_dir(&directory.absolute) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            if cancelled {
                return SearchOutcome::Cancelled;
            }
            entries_examined += 1;
            if entries_examined > limits.max_search_entries as u64 {
                stop_search!(TruncationReason::EntryBudget);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let folded = fold_path_component(&name, fold);
            if DEFAULT_EXCLUDED_DIRECTORIES
                .iter()
                .any(|excluded| fold_path_component(excluded, fold) == folded)
            {
                continue;
            }
            names.push(name);
        }
        names.sort();
        for name in names {
            if cancelled {
                return SearchOutcome::Cancelled;
            }
            if Instant::now() >= deadline {
                stop_search!(TruncationReason::TimeBudget);
            }
            let absolute = directory.absolute.join(&name);
            let stats = match std::fs::symlink_metadata(&absolute) {
                Ok(stats) => stats,
                Err(_) => {
                    skipped_files += 1;
                    continue;
                }
            };
            let file_type = stats.file_type();
            if file_type.is_symlink() {
                skipped_files += 1;
                continue;
            }
            if stats.is_dir() {
                pending.push(PendingDirectory {
                    absolute,
                    relative: child_relative_path(&directory.relative, &name),
                    depth: directory.depth + 1,
                });
                continue;
            }
            if !stats.is_file() {
                skipped_files += 1;
                continue;
            }
            files_considered += 1;
            if files_considered > limits.max_search_files_considered as u64 {
                stop_search!(TruncationReason::FileBudget);
            }
            if stats.len() > limits.max_search_file_size_bytes as u64 {
                skipped_files += 1;
                continue;
            }
            if scanned_files >= limits.max_search_files as u64 {
                stop_search!(TruncationReason::ScanBudget);
            }
            scanned_files += 1;
            if cancelled {
                return SearchOutcome::Cancelled;
            }
            let bytes = match read_complete_file_bounded(
                &absolute,
                limits.max_search_file_size_bytes,
            ) {
                BoundedFileRead::Complete(bytes) => bytes,
                _ => {
                    skipped_files += 1;
                    continue;
                }
            };
            input_bytes += bytes.len() as u64;
            if input_bytes > limits.max_search_input_bytes as u64 {
                stop_search!(TruncationReason::InputBudget);
            }
            if looks_binary(&bytes) {
                skipped_files += 1;
                continue;
            }
            let text = match decode_utf8(&bytes) {
                Some(text) => text,
                None => {
                    skipped_files += 1;
                    continue;
                }
            };
            let relative_path =
                child_relative_path(&directory.relative, &name);
            let lines = split_into_lines(&text);
            for (line_index, line) in lines.iter().enumerate() {
                if (line_index & 63) == 0 {
                    if cancelled {
                        return SearchOutcome::Cancelled;
                    }
                    if Instant::now() >= deadline {
                        stop_search!(TruncationReason::TimeBudget);
                    }
                }
                if let Some(column) = utf16_index_of(line, &input.query) {
                    let match_text =
                        utf16_slice(line, limits.max_search_line_length_chars);
                    matches.push(SearchMatch {
                        path: relative_path.clone(),
                        line: line_index as u64 + 1,
                        column: column as u64 + 1,
                        text: match_text.to_owned(),
                    });
                    output_bytes += match_text.len() as u64;
                    if output_bytes > limits.max_search_output_bytes as u64 {
                        stop_search!(TruncationReason::OutputBudget);
                    }
                    if matches.len() >= input.max_results {
                        stop_search!(TruncationReason::MatchLimit);
                    }
                }
            }
        }
    }
    matches.sort_by(compare_matches);
    SearchOutcome::Success {
        query: input.query.clone(),
        path: resolved.workspace_relative_path,
        matches,
        scanned_files,
        skipped_files,
        truncated,
        truncation_reason,
    }
}
/// One pending directory in the bounded traversal.
struct PendingDirectory {
    absolute: PathBuf,
    relative: String,
    depth: usize,
}

/// Platform folding for this search run.
fn fold_component_platform() -> bool {
    crate::workspace::fs::is_case_insensitive_platform()
}

/// Child relative path with `/` separators (`"."` root handling).
fn child_relative_path(directory_path: &str, name: &str) -> String {
    if directory_path == "." {
        name.to_owned()
    } else {
        format!("{directory_path}/{name}")
    }
}

/// Deterministic match ordering: path, then line, then column.
fn compare_matches(a: &SearchMatch, b: &SearchMatch) -> std::cmp::Ordering {
    a.path.cmp(&b.path).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column))
}

#[cfg(test)]
mod tests {
    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    use super::{
        SearchInput, SearchOutcome, TruncationReason, parse_search_input,
        search,
    };
    use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

    fn workspace() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "siralos-search-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src")).unwrap();
        std::fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
        std::fs::write(base.join("src/main.txt"), b"needle here\nno match\n")
            .unwrap();
        std::fs::write(
            base.join("README.md"),
            b"NEEDLE upper\nneedle lower\n",
        )
        .unwrap();
        std::fs::write(base.join("node_modules/pkg/x.js"), b"needle hidden\n")
            .unwrap();
        std::fs::write(base.join("src/bin.dat"), [0u8, 1, 2]).unwrap();
        base
    }

    #[test]
    fn finds_sorted_case_sensitive_matches_with_exclusions() {
        let base = workspace();
        let input = SearchInput {
            query: "needle".to_owned(),
            path: ".".to_owned(),
            max_results: WORKSPACE_LIMITS.max_search_matches,
        };
        let SearchOutcome::Success { matches, truncated, .. } =
            search(&base, &input, &WORKSPACE_LIMITS, false)
        else {
            panic!("search failed");
        };
        assert!(!truncated);
        assert_eq!(
            matches.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(),
            vec!["README.md", "src/main.txt"],
        );
        let main = &matches[1];
        assert_eq!(main.line, 1);
        assert_eq!(main.column, 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn match_limit_and_scan_budget_are_deterministic() {
        let base = workspace();
        let limited = SearchInput {
            query: "needle".to_owned(),
            path: ".".to_owned(),
            max_results: 1,
        };
        let SearchOutcome::Success {
            truncated,
            truncation_reason,
            matches,
            ..
        } = search(&base, &limited, &WORKSPACE_LIMITS, false)
        else {
            panic!("search failed");
        };
        assert!(truncated);
        assert_eq!(truncation_reason, Some(TruncationReason::MatchLimit));
        assert_eq!(matches.len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn denies_excluded_roots_and_invalid_inputs() {
        let base = workspace();
        let excluded = SearchInput {
            query: "needle".to_owned(),
            path: "node_modules".to_owned(),
            max_results: 10,
        };
        assert!(matches!(
            search(&base, &excluded, &WORKSPACE_LIMITS, false),
            SearchOutcome::Denied { .. },
        ));
        assert!(
            parse_search_input(&serde_json::json!({}), &WORKSPACE_LIMITS,)
                .is_err()
        );
        assert!(
            parse_search_input(
                &serde_json::json!({ "query": "x", "maxResults": 0 }),
                &WORKSPACE_LIMITS,
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
