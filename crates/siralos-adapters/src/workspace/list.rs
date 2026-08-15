//! Deterministic bounded workspace listing (R4, reference
//! `workspace.list`).
//!
//! Entries are enumerated incrementally with a hard cap so a hostile
//! directory can never be materialized; the cap counts excluded and
//! hidden entries too. Exclusions fold case on case-insensitive
//! platforms. Names are sorted at the authoritative boundary so
//! filesystem enumeration order can never become semantic ordering.

use crate::workspace::fs::{
    DEFAULT_EXCLUDED_DIRECTORIES, MUTATION_TEMP_PREFIX, fold_path_component,
    is_case_insensitive_platform,
};
use crate::workspace::resolve::resolve_workspace_path;

use siralos_core::workspace::bounds::WorkspaceLimits;

use std::path::Path;

/// Kind of one listed entry (lstat classification).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Regular file with its exact byte size.
    File {
        /// Exact byte size.
        size: u64,
    },
    /// Directory.
    Directory,
    /// Symbolic link (never followed).
    Symlink,
    /// Any other special file.
    Other,
}

/// One listed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    /// Entry name (exact bytes as enumerated).
    pub name: String,
    /// Workspace-relative entry path (`/` separators).
    pub path: String,
    /// lstat classification.
    pub kind: EntryKind,
}

/// Outcome of one bounded listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListOutcome {
    /// The requested path was rejected (denied).
    Denied {
        /// Stable rejection message.
        message: String,
    },
    /// The target cannot be inspected or listed (failed).
    Failed {
        /// Stable failure message.
        message: String,
    },
    /// Successful bounded listing.
    Success {
        /// Canonical workspace-relative directory path.
        path: String,
        /// Sorted bounded entries.
        entries: Vec<ListEntry>,
        /// True when entries were truncated to the listing bound.
        truncated: bool,
    },
}
/// List one directory within the canonical workspace root with the
/// reference bounds and semantics.
pub fn list_directory(
    root: &Path,
    requested: &str,
    limits: &WorkspaceLimits,
) -> ListOutcome {
    let resolved = match resolve_workspace_path(root, requested) {
        Ok(resolved) => resolved,
        Err(rejection) => {
            return ListOutcome::Denied { message: rejection.to_string() };
        }
    };
    if let Some(component) = excluded_component(
        &resolved.workspace_relative_path,
        &DEFAULT_EXCLUDED_DIRECTORIES,
    ) {
        return ListOutcome::Denied {
            message: format!(
                "Path is inside the excluded directory {component}."
            ),
        };
    }
    let metadata = match std::fs::metadata(&resolved.absolute_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return ListOutcome::Failed {
                message: format!("Cannot inspect directory: {error}"),
            };
        }
    };
    if !metadata.is_dir() {
        return ListOutcome::Failed {
            message: "Target is not a directory.".to_owned(),
        };
    }
    let fold = is_case_insensitive_platform();
    let mut names: Vec<String> = Vec::new();
    let mut truncated = match enumerate_bounded(
        &resolved.absolute_path,
        limits.max_directory_entries + 1,
        &mut |name| {
            let folded = fold_path_component(&name, fold);
            let excluded =
                DEFAULT_EXCLUDED_DIRECTORIES.iter().any(|candidate| {
                    fold_path_component(candidate, fold) == folded
                });
            if !excluded && !name.starts_with(MUTATION_TEMP_PREFIX) {
                names.push(name);
            }
        },
    ) {
        Ok(capped) => capped,
        Err(error) => {
            return ListOutcome::Failed {
                message: format!("Cannot list directory: {error}"),
            };
        }
    };
    names.sort();
    truncated = truncated || names.len() > limits.max_directory_entries;
    names.truncate(limits.max_directory_entries);
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let entry_path = if resolved.workspace_relative_path == "." {
            name.clone()
        } else {
            format!("{}/{}", resolved.workspace_relative_path, name)
        };
        let kind = match std::fs::symlink_metadata(
            resolved.absolute_path.join(&name),
        ) {
            Ok(metadata) => {
                let file_type = metadata.file_type();
                if file_type.is_symlink() {
                    EntryKind::Symlink
                } else if metadata.is_dir() {
                    EntryKind::Directory
                } else if metadata.is_file() {
                    EntryKind::File { size: metadata.len() }
                } else {
                    EntryKind::Other
                }
            }
            Err(error) => {
                return ListOutcome::Failed {
                    message: format!("Cannot inspect entry: {error}"),
                };
            }
        };
        entries.push(ListEntry { name, path: entry_path, kind });
    }
    ListOutcome::Success {
        path: resolved.workspace_relative_path,
        entries,
        truncated,
    }
}
/// The first excluded directory component of a workspace-relative
/// path under the platform folding policy, if any.
pub fn excluded_component(
    workspace_relative_path: &str,
    excluded: &[&str],
) -> Option<String> {
    let fold = is_case_insensitive_platform();
    let components = workspace_relative_path
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".");
    for component in components {
        let folded = fold_path_component(component, fold);
        if excluded
            .iter()
            .any(|name| fold_path_component(name, fold) == folded)
        {
            return Some(component.to_owned());
        }
    }
    None
}

/// Incremental bounded directory enumeration. Returns `Ok(true)` when
/// the entry cap was reached (truncated); a missing directory yields
/// an empty, untruncated result like the reference.
fn enumerate_bounded(
    directory: &Path,
    max_entries: usize,
    on_entry: &mut dyn FnMut(String),
) -> std::io::Result<bool> {
    let mut handle = match std::fs::read_dir(directory) {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let mut index = 0usize;
    loop {
        if index >= max_entries {
            return Ok(true);
        }
        let Some(entry) = handle.next().transpose()? else {
            return Ok(false);
        };
        on_entry(entry.file_name().to_string_lossy().into_owned());
        index += 1;
    }
}
#[cfg(test)]
mod tests {
    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    use super::{EntryKind, ListOutcome, list_directory};
    use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

    #[test]
    fn lists_sorted_bounded_entries_with_types() {
        let base = std::env::temp_dir().join(format!(
            "siralos-list-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("b.txt"), b"x").unwrap();
        std::fs::write(base.join("a.txt"), b"y").unwrap();
        std::fs::create_dir_all(base.join("node_modules")).unwrap();
        std::fs::write(base.join("node_modules/pkg.js"), b"z").unwrap();
        std::fs::write(base.join(".siralos-mutation-x.tmp"), b"t").unwrap();
        let outcome = list_directory(&base, ".", &WORKSPACE_LIMITS);
        let ListOutcome::Success { entries, truncated, path } = outcome else {
            panic!("listing failed: {outcome:?}");
        };
        assert_eq!(path, ".");
        assert!(!truncated);
        let names: Vec<&str> =
            entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
        assert_eq!(entries[0].kind, EntryKind::File { size: 1 },);
        assert_eq!(entries[2].kind, EntryKind::Directory);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn denies_excluded_and_escaping_paths() {
        let base = std::env::temp_dir().join(format!(
            "siralos-list-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(matches!(
            list_directory(&base, "node_modules", &WORKSPACE_LIMITS),
            ListOutcome::Denied { .. },
        ));
        assert!(matches!(
            list_directory(&base, "../x", &WORKSPACE_LIMITS),
            ListOutcome::Denied { .. },
        ));
        let _ = std::fs::remove_dir_all(&base);
    }
}
