//! Activation B1: the bounded read-only workspace scan.
//!
//! A deterministic, read-only scan producing graph-reconstruction material
//! over the existing digest seam. No persistence, no spawn, fail-closed.
//! The scan composes the existing bounded exact-read primitive
//! (`read_complete_file_bounded`) only; listing is deterministic
//! lexicographic. Protected and oversized paths are skipped and counted,
//! the node cap truncates deterministically, and no partial reads ever
//! contribute a digest. Staleness is out of scope for B1.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use siralos_core::context_graph::{
    ContextGraph, ContextGraphError, ContextNode, ContextNodeKind,
};

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// Pinned scan bounds for B1 (record constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBounds {
    /// Maximum number of nodes admitted.
    pub max_nodes: usize,
    /// Maximum file size in bytes (no partial reads).
    pub max_file_bytes: usize,
}

/// Default B1 bounds: 256 nodes, 65536 bytes/file.
pub const DEFAULT_SCAN_BOUNDS: ScanBounds =
    ScanBounds { max_nodes: 256, max_file_bytes: 65536 };

// ---------------------------------------------------------------------------
// Scan primitives
// ---------------------------------------------------------------------------

/// One scan node carrying the existing identity digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanNode {
    /// Workspace-relative path (`/` separators, lexicographic order).
    pub relative_path: String,
    /// 64 lowercase hex content digest over the exact bytes.
    pub content_digest: String,
    /// Exact byte length.
    pub byte_len: usize,
}

/// Result of the bounded scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedScan {
    /// Admitted nodes in lexicographic relative_path order.
    pub nodes: Vec<ScanNode>,
    /// Protected paths skipped (no node, counted).
    pub protected_skipped: usize,
    /// Oversized files skipped (no partial read, no digest).
    pub oversized_skipped: usize,
    /// Candidate files not admitted due to node-cap truncation.
    pub files_not_scanned: usize,
    /// True when truncation occurred.
    pub truncated: bool,
}

/// Typed unavailability — never panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    /// Workspace unavailable (fail-closed).
    Unavailable {
        /// Human-readable reason (never panics).
        message: String,
    },
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ScanError {}

// ---------------------------------------------------------------------------
// Protected predicate
// ---------------------------------------------------------------------------

fn is_protected(relative: &str) -> bool {
    // Any-depth AGENTS.md (exact file name match on last component).
    if let Some(last) = relative.rsplit('/').next() {
        if last == "AGENTS.md" {
            return true;
        }
    }
    // .siralos/** and .git/** at any depth: any path component equals the directory name.
    for component in relative.split('/') {
        if component == ".siralos" || component == ".git" {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Deterministic walk (bounded, lexicographic)
// ---------------------------------------------------------------------------

fn collect_all_files(root: &Path) -> Result<Vec<String>, ScanError> {
    // Depth-first stack of directories to visit (relative).
    let mut dirs: Vec<PathBuf> = vec![PathBuf::new()];
    let mut files: Vec<String> = Vec::new();

    while let Some(rel_dir) = dirs.pop() {
        let abs_dir = if rel_dir.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&rel_dir)
        };
        let read = match std::fs::read_dir(&abs_dir) {
            Ok(handle) => handle,
            Err(_) => continue,
        };
        // Collect entries then sort lexicographically by name.
        let mut entries: BTreeMap<String, PathBuf> = BTreeMap::new();
        for entry in read {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            // Stable: keep first insertion for duplicate names (should not happen).
            entries.entry(name).or_insert(entry.path());
        }
        // Sorted by BTreeMap already.
        for (name, abs_path) in entries {
            let rel_path = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                rel_dir.join(&name)
            };
            let rel_str = rel_path.to_string_lossy().replace('\\', "/");
            let metadata = match std::fs::symlink_metadata(&abs_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                dirs.push(rel_path);
            } else if file_type.is_file() {
                files.push(rel_str);
            }
        }
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// Public scan
// ---------------------------------------------------------------------------

/// Bounded scan with explicit bounds (test hook; pinned defaults are the record constants).
pub fn scan_workspace_with_bounds(
    root: &Path,
    bounds: ScanBounds,
) -> Result<BoundedScan, ScanError> {
    // Fail-closed: root must be an accessible directory.
    let meta = std::fs::symlink_metadata(root).map_err(|error| {
        ScanError::Unavailable {
            message: format!("workspace unavailable: {error}"),
        }
    })?;
    if !meta.is_dir() {
        return Err(ScanError::Unavailable {
            message: "workspace unavailable: not a directory".to_owned(),
        });
    }
    let mut files = collect_all_files(root)?;
    files.sort();

    let mut nodes: Vec<ScanNode> = Vec::new();
    let mut protected_skipped: usize = 0;
    let mut oversized_skipped: usize = 0;
    let mut files_not_scanned: usize = 0;
    let mut truncated = false;

    for relative in files {
        if is_protected(&relative) {
            protected_skipped += 1;
            continue;
        }
        let abs = root.join(&relative);
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        if size > bounds.max_file_bytes as u64 {
            oversized_skipped += 1;
            continue;
        }
        if nodes.len() >= bounds.max_nodes {
            files_not_scanned += 1;
            truncated = true;
            continue;
        }
        match crate::workspace::fs::read_complete_file_bounded(
            &abs,
            bounds.max_file_bytes,
        ) {
            crate::workspace::fs::BoundedFileRead::Complete(bytes) => {
                let digest = siralos_core::identity::sha256_hex(&bytes);
                let len = bytes.len();
                nodes.push(ScanNode {
                    relative_path: relative,
                    content_digest: digest,
                    byte_len: len,
                });
            }
            crate::workspace::fs::BoundedFileRead::TooLarge => {
                oversized_skipped += 1;
            }
            crate::workspace::fs::BoundedFileRead::NotReadable
            | crate::workspace::fs::BoundedFileRead::IoError(_) => {
                // Not a readable regular file — no node, no counters beyond truncation.
                continue;
            }
        }
    }

    Ok(BoundedScan {
        nodes,
        protected_skipped,
        oversized_skipped,
        files_not_scanned,
        truncated,
    })
}

/// Bounded scan with the pinned B1 defaults (256 nodes, 65536 bytes/file).
pub fn scan_workspace(root: &Path) -> Result<BoundedScan, ScanError> {
    scan_workspace_with_bounds(root, DEFAULT_SCAN_BOUNDS)
}

// ---------------------------------------------------------------------------
// Provisional graph binding (B2 revisits kinds and per-kind bounds)
// ---------------------------------------------------------------------------

/// Pure provisional binding of ScanNodes into the decision 79 ContextGraph.
///
/// Uses `ContextNodeKind::Source` for every node via the graph's existing
/// validated constructor (`ContextGraph::build`). The binding is provisional
/// until the B2 taxonomy review; staleness marking is out of scope for B1.
pub fn bind_scan_to_graph(
    scan: &BoundedScan,
) -> Result<ContextGraph, ContextGraphError> {
    let nodes: Vec<ContextNode> = scan
        .nodes
        .iter()
        .map(|n| ContextNode {
            id: n.relative_path.clone(),
            kind: ContextNodeKind::Source,
            content_digest: n.content_digest.clone(),
            summary: String::new(),
            source_bindings: Vec::new(),
            token_estimate: 0,
        })
        .collect();
    ContextGraph::build(nodes, Vec::new())
}

// ---------------------------------------------------------------------------
// Tests (~8 as specified)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SCAN_BOUNDS, ScanBounds, ScanError, bind_scan_to_graph,
        is_protected, scan_workspace, scan_workspace_with_bounds,
    };
    use std::path::Path;

    fn tmp_root(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "siralos-scan-test-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn write(root: &Path, rel: &str, content: &[u8]) {
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }

    #[test]
    fn determinism_run_twice_byte_equal() {
        let root = tmp_root("determinism");
        write(&root, "b.txt", b"hello");
        write(&root, "a.txt", b"world");
        write(&root, "sub/c.txt", b"!");
        let first = scan_workspace(&root).unwrap();
        let second = scan_workspace(&root).unwrap();
        assert_eq!(first, second);
        // Also byte-equal after JSON canonicalization (simulated by debug equality).
        let first_json = serde_json::to_string(&serde_json::json!({
            "nodes": first.nodes.iter().map(|n| serde_json::json!({"path": n.relative_path, "digest": n.content_digest, "len": n.byte_len})).collect::<Vec<_>>(),
            "protected_skipped": first.protected_skipped,
            "oversized_skipped": first.oversized_skipped,
            "files_not_scanned": first.files_not_scanned,
            "truncated": first.truncated,
        }))
        .unwrap();
        let second_json = serde_json::to_string(&serde_json::json!({
            "nodes": second.nodes.iter().map(|n| serde_json::json!({"path": n.relative_path, "digest": n.content_digest, "len": n.byte_len})).collect::<Vec<_>>(),
            "protected_skipped": second.protected_skipped,
            "oversized_skipped": second.oversized_skipped,
            "files_not_scanned": second.files_not_scanned,
            "truncated": second.truncated,
        }))
        .unwrap();
        assert_eq!(first_json, second_json);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lexicographic_order() {
        let root = tmp_root("lex");
        write(&root, "z.txt", b"z");
        write(&root, "a.txt", b"a");
        write(&root, "m/n.txt", b"m");
        write(&root, "a/b.txt", b"ab");
        let scan = scan_workspace(&root).unwrap();
        let paths: Vec<&str> =
            scan.nodes.iter().map(|n| n.relative_path.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "nodes must be lexicographic");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn node_cap_truncation_at_256_with_counted_remainder() {
        let root = tmp_root("cap");
        for i in 0..300 {
            write(&root, &format!("f{i:03}.txt"), b"x");
        }
        let scan = scan_workspace(&root).unwrap();
        assert_eq!(scan.nodes.len(), 256);
        assert!(scan.truncated);
        assert_eq!(scan.files_not_scanned, 44);
        assert_eq!(scan.protected_skipped, 0);
        assert_eq!(scan.oversized_skipped, 0);

        // Small bounds override via the bounds-taking entry point.
        let small = ScanBounds { max_nodes: 2, max_file_bytes: 65536 };
        let small_scan = scan_workspace_with_bounds(&root, small).unwrap();
        assert_eq!(small_scan.nodes.len(), 2);
        assert!(small_scan.truncated);
        assert_eq!(small_scan.files_not_scanned, 298);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_skip_no_partial_digest() {
        let root = tmp_root("oversized");
        write(&root, "small.txt", b"ok");
        // Create a file one byte over the limit.
        let big = vec![b'x'; DEFAULT_SCAN_BOUNDS.max_file_bytes + 1];
        write(&root, "big.txt", &big);
        let scan = scan_workspace(&root).unwrap();
        assert_eq!(scan.oversized_skipped, 1);
        assert_eq!(scan.nodes.len(), 1);
        assert_eq!(scan.nodes[0].relative_path, "small.txt");
        // Ensure no node for big.txt.
        assert!(!scan.nodes.iter().any(|n| n.relative_path == "big.txt"));
        // No partial reads: if we had read the prefix, the digest would not match full bytes.
        // Here we simply ensure the big file did not contribute a node.
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_skip_agents_and_dot_dirs() {
        let root = tmp_root("protected");
        write(&root, "keep.txt", b"keep");
        write(&root, "AGENTS.md", b"protected root");
        write(&root, "sub/AGENTS.md", b"protected deep");
        write(&root, ".siralos/secret.txt", b"protected siralos");
        write(&root, ".siralos/nested/inner.txt", b"also protected");
        write(&root, ".git/config", b"protected git");
        write(&root, ".git/objects/obj.txt", b"protected git deep");
        let scan = scan_workspace(&root).unwrap();
        assert_eq!(scan.nodes.len(), 1);
        assert_eq!(scan.nodes[0].relative_path, "keep.txt");
        assert_eq!(scan.protected_skipped, 6);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn digest_correctness_over_exact_bytes() {
        let root = tmp_root("digest");
        let content = b"hello world\nsecond line\n";
        write(&root, "a.txt", content);
        let scan = scan_workspace(&root).unwrap();
        assert_eq!(scan.nodes.len(), 1);
        let expected = siralos_core::identity::sha256_hex(content);
        assert_eq!(scan.nodes[0].content_digest, expected);
        assert_eq!(scan.nodes[0].byte_len, content.len());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn typed_unavailability_workspace_missing() {
        let missing =
            Path::new("/tmp/siralos-scan-missing-workspace-zzz-404-not-exist");
        let _ = std::fs::remove_dir_all(missing);
        let err = scan_workspace(missing).unwrap_err();
        assert!(matches!(err, ScanError::Unavailable { .. }));
        // Never panic.
    }

    #[test]
    fn read_only_no_mutation_regression() {
        let root = tmp_root("readonly");
        write(&root, "a.txt", b"content");
        let before_entries: Vec<String> = {
            let mut v = Vec::new();
            for entry in walkdir(&root) {
                v.push(entry);
            }
            v.sort();
            v
        };
        let _ = scan_workspace(&root).unwrap();
        let after_entries: Vec<String> = {
            let mut v = Vec::new();
            for entry in walkdir(&root) {
                v.push(entry);
            }
            v.sort();
            v
        };
        assert_eq!(
            before_entries, after_entries,
            "scan must not mutate workspace"
        );
        // Also no files created/deleted: count same.
        let _ = std::fs::remove_dir_all(&root);
    }

    fn walkdir(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![PathBuf::new()];
        while let Some(rel) = stack.pop() {
            let abs = if rel.as_os_str().is_empty() {
                root.to_path_buf()
            } else {
                root.join(&rel)
            };
            let Ok(read) = std::fs::read_dir(&abs) else { continue };
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel_path = if rel.as_os_str().is_empty() {
                    PathBuf::from(&name)
                } else {
                    rel.join(&name)
                };
                let rel_str = rel_path.to_string_lossy().replace('\\', "/");
                let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                    continue;
                };
                if meta.is_dir() && !meta.file_type().is_symlink() {
                    stack.push(rel_path);
                    out.push(rel_str);
                } else if meta.is_file() {
                    out.push(rel_str);
                }
            }
        }
        out
    }

    #[test]
    fn graph_binding_builds_expected_node_set() {
        let root = tmp_root("graph");
        write(&root, "b.txt", b"bb");
        write(&root, "a.txt", b"aa");
        let scan = scan_workspace(&root).unwrap();
        assert_eq!(scan.nodes.len(), 2);
        let graph = bind_scan_to_graph(&scan).expect("graph build");
        assert_eq!(graph.nodes().len(), 2);
        // Nodes in graph are canonically sorted by id.
        let ids: Vec<&str> =
            graph.nodes().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a.txt", "b.txt"]);
        for node in graph.nodes() {
            assert_eq!(
                node.kind,
                siralos_core::context_graph::ContextNodeKind::Source
            );
            assert_eq!(node.summary, "");
            assert!(node.source_bindings.is_empty());
            assert_eq!(node.content_digest.len(), 64);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn protected_predicate_exactness() {
        assert!(is_protected("AGENTS.md"));
        assert!(is_protected("a/b/AGENTS.md"));
        assert!(!is_protected("AGENTS.md.bak"));
        assert!(!is_protected("myAGENTS.md"));
        assert!(is_protected(".siralos/x"));
        assert!(is_protected(".git/y/z"));
        assert!(!is_protected(".siralos2/x"));
        assert!(is_protected("a/.siralos/x"));
        assert!(is_protected("a/b/.git/c.txt"));
    }

    // Adjust is_protected to handle any-depth .siralos/.git would be caught by component check.
    // The above test documents the intended behavior; if it fails, fix predicate.

    use std::path::PathBuf;
}
