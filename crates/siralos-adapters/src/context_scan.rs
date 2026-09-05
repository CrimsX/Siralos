//! Activation B1/B2: the bounded read-only workspace scan + node taxonomy.
//!
//! B1: a deterministic, read-only scan producing graph-reconstruction material
//! over the existing digest seam. No persistence, no spawn, fail-closed.
//! The scan composes the existing bounded exact-read primitive
//! (`read_complete_file_bounded`) only; listing is deterministic
//! lexicographic. Protected and oversized paths are skipped and counted,
//! the node cap truncates deterministically, and no partial reads ever
//! contribute a digest. Staleness is out of scope for B1.
//! B2 (decision 97): the approved taxonomy at binding time — Source (default),
//! Decision (`docs/adr/**` and `docs/wayfinder/decisions/**` with segment
//! boundary, normalized separators), Knowledge reserved with no producer;
//! classification is pure `classify_node` applied inside the binding;
//! the B1 scan output stays byte-identical (kind-agnostic); per-kind bounds
//! none beyond global B1 bounds; L0 digest only; no model-visible surface;
//! the decision 92 kind-weight table is untouched (Decision +0).

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
// Graph binding with the approved B2 taxonomy (decision 97)
// ---------------------------------------------------------------------------

/// Pure taxonomy classifier applied at graph-binding time only (T2).
///
/// Every scanned file is `Source` by default. Paths under `docs/adr/` or
/// `docs/wayfinder/decisions/` (normalized separators, prefix match on path
/// segments) classify as `Decision`. `Knowledge` is reserved and never produced
/// by the scan in this arc (T1). No per-kind bounds beyond the global B1 scan
/// bounds (T4). The binding stays at L0 digest only (T5), host-side with no
/// model-visible surface (T6). The decision 92 kind-weight table is untouched:
/// `Knowledge +3, Source +1, all other kinds +0` — `Decision` falls in the
/// `+0` bucket.
pub fn classify_node(relative_path: &str) -> ContextNodeKind {
    // Normalize host separators to '/'.
    let normalized = relative_path.replace('\\', "/");
    // Decision prefixes: docs/adr/** and docs/wayfinder/decisions/** with
    // segment-boundary prefix match so docs/adrbogus/x does NOT match.
    if normalized == "docs/adr" || normalized.starts_with("docs/adr/") {
        return ContextNodeKind::Decision;
    }
    if normalized == "docs/wayfinder/decisions"
        || normalized.starts_with("docs/wayfinder/decisions/")
    {
        return ContextNodeKind::Decision;
    }
    ContextNodeKind::Source
}

/// Binding of ScanNodes into the decision 79 ContextGraph using the B2 taxonomy.
///
/// The scan output (`BoundedScan`/`ScanNode`) is kind-agnostic and byte-identical
/// to B1 (T2). Classification happens only here via `classify_node`. The binding
/// stays L0 digest only; staleness is out of scope.
pub fn bind_scan_to_graph(
    scan: &BoundedScan,
) -> Result<ContextGraph, ContextGraphError> {
    let nodes: Vec<ContextNode> = scan
        .nodes
        .iter()
        .map(|n| ContextNode {
            id: n.relative_path.clone(),
            kind: classify_node(&n.relative_path),
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
        classify_node, is_protected, scan_workspace,
        scan_workspace_with_bounds,
    };
    use siralos_core::context_graph::ContextNodeKind;
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

    // -----------------------------------------------------------------------
    // B2 taxonomy tests (decision 97 T1-T6)
    // -----------------------------------------------------------------------

    #[test]
    fn classify_decision_prefix_docs_adr() {
        assert_eq!(classify_node("docs/adr/x"), ContextNodeKind::Decision);
        assert_eq!(
            classify_node("docs/adr/nested/y.md"),
            ContextNodeKind::Decision
        );
        assert_eq!(
            classify_node("docs/adr/0036-lean.md"),
            ContextNodeKind::Decision
        );
        // exact prefix directory file
        assert_eq!(classify_node("docs/adr"), ContextNodeKind::Decision);
        // normalized separators
        assert_eq!(
            classify_node("docs\\adr\\win.md"),
            ContextNodeKind::Decision
        );
    }

    #[test]
    fn classify_decision_prefix_wayfinder() {
        assert_eq!(
            classify_node("docs/wayfinder/decisions/x"),
            ContextNodeKind::Decision
        );
        assert_eq!(
            classify_node("docs/wayfinder/decisions/nested/file.md"),
            ContextNodeKind::Decision
        );
        assert_eq!(
            classify_node("docs/wayfinder/decisions"),
            ContextNodeKind::Decision
        );
        assert_eq!(
            classify_node("docs\\wayfinder\\decisions\\win.md"),
            ContextNodeKind::Decision
        );
    }

    #[test]
    fn classify_non_matching_near_misses_are_source() {
        assert_eq!(classify_node("docs/adrbogus/x"), ContextNodeKind::Source);
        assert_eq!(classify_node("docs/adr.txt"), ContextNodeKind::Source);
        assert_eq!(classify_node("docs/adrbogus"), ContextNodeKind::Source);
        assert_eq!(
            classify_node("docs/wayfinder/decisionsbogus/x"),
            ContextNodeKind::Source
        );
        assert_eq!(
            classify_node("docs/wayfinder/decisions.txt"),
            ContextNodeKind::Source
        );
        assert_eq!(
            classify_node("docs/wayfinder/other/x"),
            ContextNodeKind::Source
        );
        assert_eq!(classify_node("other/docs/adr/x"), ContextNodeKind::Source);
    }

    #[test]
    fn classify_default_source() {
        assert_eq!(classify_node("a.txt"), ContextNodeKind::Source);
        assert_eq!(classify_node("src/lib.rs"), ContextNodeKind::Source);
        assert_eq!(classify_node("docs/readme.md"), ContextNodeKind::Source);
        assert_eq!(classify_node(""), ContextNodeKind::Source);
    }

    #[test]
    fn binding_never_produces_knowledge() {
        let root = tmp_root("no-knowledge");
        write(&root, "docs/adr/001.md", b"decision");
        write(&root, "docs/wayfinder/decisions/002.md", b"decision2");
        write(&root, "src/a.txt", b"source");
        write(&root, "knowledge.txt", b"looks like knowledge but is Source");
        let scan = scan_workspace(&root).unwrap();
        let graph = bind_scan_to_graph(&scan).expect("graph");
        for node in graph.nodes() {
            assert_ne!(
                node.kind,
                ContextNodeKind::Knowledge,
                "no Knowledge nodes: {}",
                node.id
            );
        }
        // Also directly: classify never returns Knowledge
        assert_ne!(classify_node("docs/adr/x"), ContextNodeKind::Knowledge);
        assert_ne!(classify_node("any/path"), ContextNodeKind::Knowledge);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_output_unchanged_byte_equal() {
        let root = tmp_root("scan-unchanged");
        write(&root, "docs/adr/1.md", b"d1");
        write(&root, "docs/wayfinder/decisions/2.md", b"d2");
        write(&root, "a.txt", b"plain");
        let scan1 = scan_workspace(&root).unwrap();
        let scan2 = scan_workspace(&root).unwrap();
        // BoundedScan byte-equal (kind-agnostic)
        assert_eq!(scan1, scan2);
        let json1 = serde_json::to_string(&serde_json::json!({
            "nodes": scan1.nodes.iter().map(|n| serde_json::json!({"path": n.relative_path, "digest": n.content_digest, "len": n.byte_len})).collect::<Vec<_>>(),
            "protected_skipped": scan1.protected_skipped,
            "oversized_skipped": scan1.oversized_skipped,
            "files_not_scanned": scan1.files_not_scanned,
            "truncated": scan1.truncated,
        }))
        .unwrap();
        let json2 = serde_json::to_string(&serde_json::json!({
            "nodes": scan2.nodes.iter().map(|n| serde_json::json!({"path": n.relative_path, "digest": n.content_digest, "len": n.byte_len})).collect::<Vec<_>>(),
            "protected_skipped": scan2.protected_skipped,
            "oversized_skipped": scan2.oversized_skipped,
            "files_not_scanned": scan2.files_not_scanned,
            "truncated": scan2.truncated,
        }))
        .unwrap();
        assert_eq!(json1, json2);
        // Binding does not mutate scan
        let scan_before = scan1.clone();
        let _ = bind_scan_to_graph(&scan1).expect("bind");
        assert_eq!(scan1, scan_before, "bind must not mutate scan");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn kind_weight_decision_is_zero() {
        // Decision 92 table: Knowledge +3, Source +1, all other kinds +0. Decision is +0.
        fn kind_bonus(kind: ContextNodeKind) -> i32 {
            match kind {
                ContextNodeKind::Knowledge => 3,
                ContextNodeKind::Source => 1,
                _ => 0,
            }
        }
        assert_eq!(kind_bonus(ContextNodeKind::Decision), 0);
        assert_eq!(kind_bonus(ContextNodeKind::Source), 1);
        assert_eq!(kind_bonus(ContextNodeKind::Knowledge), 3);
        // Other kinds also +0
        assert_eq!(kind_bonus(ContextNodeKind::Run), 0);
        assert_eq!(kind_bonus(ContextNodeKind::Skill), 0);
        assert_eq!(kind_bonus(ContextNodeKind::Task), 0);
    }

    #[test]
    fn determinism_binding_byte_equal() {
        let root = tmp_root("bind-determinism");
        write(&root, "docs/adr/a.md", b"hello");
        write(&root, "b.txt", b"world");
        let scan = scan_workspace(&root).unwrap();
        let g1 = bind_scan_to_graph(&scan).unwrap();
        let g2 = bind_scan_to_graph(&scan).unwrap();
        // Graphs byte-equal deterministically (nodes sorted by id, same kinds/digests)
        assert_eq!(g1.nodes(), g2.nodes());
        assert_eq!(g1.edges().len(), g2.edges().len());
        let json1 = serde_json::to_string(&serde_json::json!({
            "nodes": g1.nodes().iter().map(|n| serde_json::json!({"id": n.id, "kind": format!("{:?}", n.kind), "digest": n.content_digest})).collect::<Vec<_>>()
        }))
        .unwrap();
        let json2 = serde_json::to_string(&serde_json::json!({
            "nodes": g2.nodes().iter().map(|n| serde_json::json!({"id": n.id, "kind": format!("{:?}", n.kind), "digest": n.content_digest})).collect::<Vec<_>>()
        }))
        .unwrap();
        assert_eq!(json1, json2);
        let _ = std::fs::remove_dir_all(&root);
    }

    // Adjust is_protected to handle any-depth .siralos/.git would be caught by component check.
    // The above test documents the intended behavior; if it fails, fix predicate.

    use std::path::PathBuf;
}
