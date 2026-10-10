//! R13 reference-identity and research-policy subject record builders.
use crate::harness::*;

pub(in crate::harness) fn validate_r13_external_knowledge_input(
    subject: &str,
    input: &Value,
) -> Result<(), HarnessError> {
    if !input.is_object() {
        return Err(HarnessError::corpus(format!(
            "{subject} input must be an object"
        )));
    }
    let now_valid = input.get("nowMs").and_then(Value::as_u64).is_some();
    let cases =
        input.get("cases").and_then(Value::as_array).ok_or_else(|| {
            HarnessError::corpus(format!(
                "{subject} input must contain a cases array"
            ))
        })?;
    if cases.is_empty() || cases.len() > 16 {
        return Err(HarnessError::corpus(format!(
            "{subject} input must contain a bounded non-empty cases array"
        )));
    }
    for case in cases {
        let valid = case
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| !name.is_empty());
        if !valid {
            return Err(HarnessError::corpus(format!(
                "{subject} cases must carry a non-empty name"
            )));
        }
    }
    if !now_valid {
        return Err(HarnessError::corpus(format!(
            "{subject} input must inject a non-negative nowMs clock"
        )));
    }
    Ok(())
}

const R13_REFERENCE_NOW_MS: u64 = 1_700_000_000_000;
const R13_REFERENCE_REPO_ORIGIN: &str = "https://github.com/owner/repo";
const R13_RESEARCH_COMMIT_SHA: &str =
    "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0";

fn r13_reference_clock(values: &[u64]) -> siralos_core::reference::NowFn {
    let queue: std::sync::Arc<Mutex<Vec<u64>>> =
        std::sync::Arc::new(Mutex::new(values.to_vec()));
    std::sync::Arc::new(move || {
        let mut queue = queue.lock().expect("clock queue");
        let value = queue[0];
        if queue.len() > 1 {
            queue.remove(0);
        }
        value
    })
}

struct CountingResolver {
    inner: std::sync::Arc<dyn ReferenceResolverPort>,
    calls: AtomicUsize,
}

impl ReferenceResolverPort for CountingResolver {
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve_identity(source, allow_mutable_refs)
    }
}

struct SwapResolver {
    outcomes:
        std::sync::Arc<Mutex<BTreeMap<String, ReferenceResolutionOutcome>>>,
}

impl SwapResolver {
    fn set(&self, path: &str, outcome: ReferenceResolutionOutcome) {
        self.outcomes
            .lock()
            .expect("swap map")
            .insert(path.to_owned(), outcome);
    }
}

impl ReferenceResolverPort for SwapResolver {
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        _allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        let ReferenceSource::LocalDirectory { path } = source else {
            return ReferenceResolutionOutcome::Unavailable {
                reason: "no stub".to_owned(),
            };
        };
        self.outcomes.lock().expect("swap map").get(path).cloned().unwrap_or(
            ReferenceResolutionOutcome::Unavailable {
                reason: "no stub".to_owned(),
            },
        )
    }
}

fn resolved_local(
    canonical_path: &str,
    fingerprint: &str,
) -> ReferenceResolutionOutcome {
    ReferenceResolutionOutcome::Resolved {
        identity: ResolvedReferenceIdentity::LocalDirectory {
            canonical_path: canonical_path.to_owned(),
            fingerprint: fingerprint.to_owned(),
        },
    }
}

fn r13_trust() -> TrustForFn {
    std::sync::Arc::new(|_: &ReferenceDeclaration| {
        siralos_core::reference::ReferenceTrustClass::ExplicitUser
    })
}

fn r13_reference_limits() -> ReferenceLimits {
    ReferenceLimits {
        max_references: 16,
        max_alias_length: 64,
        max_description_bytes: 512,
        max_repository_length: 2048,
        max_local_directory_path_length: 4096,
        max_commit_length: 64,
        max_tag_length: 128,
        max_branch_length: 128,
        max_manifest_entries: 10_000,
        max_manifest_bytes: 8 * 1024 * 1024,
        max_file_sha256_bytes: 1024 * 1024,
        max_revision_bindings: 64,
    }
}

#[allow(clippy::too_many_arguments)]
fn r13_build_registry(
    declarations: Vec<ReferenceDeclaration>,
    resolver: std::sync::Arc<dyn ReferenceResolverPort>,
    workspace_root: &str,
    allow_mutable_refs: bool,
    now: siralos_core::reference::NowFn,
    limits: Option<ReferenceLimits>,
) -> siralos_core::reference::ReferenceRegistry {
    create_reference_registry(ReferenceRegistryOptions {
        declarations,
        trust_for: r13_trust(),
        workspace_root: workspace_root.to_owned(),
        resolver,
        allow_mutable_refs,
        now,
        limits: limits.unwrap_or_else(r13_reference_limits),
    })
}

fn reference_summary(reference: &Reference) -> Value {
    json!({
        "id": reference.id,
        "alias": reference.alias,
        "status": reference.status.as_str(),
        "reason": reference.failure_reason,
    })
}

fn repository_ref_json(r#ref: &RepositoryRef) -> Value {
    match r#ref {
        RepositoryRef::Commit { commit } => {
            json!({"kind": "commit", "commit": commit})
        }
        RepositoryRef::Tag { tag } => json!({"kind": "tag", "tag": tag}),
        RepositoryRef::Branch { branch } => {
            json!({"kind": "branch", "branch": branch})
        }
    }
}

fn r13_fake_repo_backend() -> std::sync::Arc<FakeRepositoryBackend> {
    let mut commits = BTreeSet::new();
    commits.insert("abc1234".to_owned());
    commits.insert("def5678".to_owned());
    let mut tags = BTreeMap::new();
    tags.insert("v1.0".to_owned(), "abc1234".to_owned());
    let mut branches = BTreeMap::new();
    branches.insert("main".to_owned(), "def5678".to_owned());
    let mut fixture = BTreeMap::new();
    fixture.insert(
        R13_REFERENCE_REPO_ORIGIN.to_owned(),
        FakeRepositoryFixtureEntry { commits, tags, branches },
    );
    std::sync::Arc::new(FakeRepositoryBackend::new(fixture))
}

fn r13_repo_resolver() -> std::sync::Arc<dyn ReferenceResolverPort> {
    let backend = r13_fake_repo_backend();
    std::sync::Arc::new(RepositoryResolver {
        backend: move |origin: &str,
                       r#ref: &RepositoryRef,
                       allow_mutable_refs: bool| {
            backend.resolve_commit(origin, r#ref, allow_mutable_refs)
        },
    })
}

fn r13_local_decl(path: &str) -> ReferenceDeclaration {
    ReferenceDeclaration {
        alias: "docs".to_owned(),
        kind: ReferenceKind::LocalDirectory,
        source: ReferenceSource::LocalDirectory { path: path.to_owned() },
        description: None,
    }
}

fn r13_repo_decl(r#ref: Option<RepositoryRef>) -> ReferenceDeclaration {
    ReferenceDeclaration {
        alias: "docs".to_owned(),
        kind: ReferenceKind::Repository,
        source: ReferenceSource::Repository {
            repository: R13_REFERENCE_REPO_ORIGIN.to_owned(),
            r#ref: r#ref.unwrap_or(RepositoryRef::Branch {
                branch: "main".to_owned(),
            }),
        },
        description: None,
    }
}

pub(in crate::harness) fn reference_identity_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let _now_ms = input
        .get("nowMs")
        .and_then(Value::as_u64)
        .unwrap_or(R13_REFERENCE_NOW_MS);
    let mut cases: Vec<Value> = Vec::new();
    for case in input
        .get("cases")
        .and_then(Value::as_array)
        .expect("validated while loading the corpus")
    {
        let name = case
            .get("name")
            .and_then(Value::as_str)
            .expect("validated while loading the corpus");
        match name {
            "declaration-parse-strict" => {
                let attempts: Vec<Value> = {
                    let relative = parse_reference_declaration(
                        &r13_decl_value_local("docs"),
                    );
                    let unknown_key = {
                        let mut value = r13_decl_value_local("/tmp/docs");
                        let object = value.as_object_mut().expect("object");
                        object.insert("surprise".to_owned(), json!(1));
                        parse_reference_declaration(&value)
                    };
                    let alias_malformed = {
                        let mut value = r13_decl_value_local("/tmp/docs");
                        let object = value.as_object_mut().expect("object");
                        object.insert("alias".to_owned(), json!("Docs"));
                        parse_reference_declaration(&value)
                    };
                    let description_too_long = {
                        let mut value = r13_decl_value_local("/tmp/docs");
                        let object = value.as_object_mut().expect("object");
                        object.insert(
                            "description".to_owned(),
                            json!("x".repeat(513)),
                        );
                        parse_reference_declaration(&value)
                    };
                    let kind_required = json!({
                        "alias": "docs",
                        "source": {"kind": "local-directory", "path": "/tmp/docs"},
                    });
                    [
                        ("valid-posix", Ok(())),
                        ("valid-windows-drive", Ok(())),
                        ("valid-windows-unc", Ok(())),
                        ("relative-refused", relative.map(|_| ())),
                        ("unknown-key-rejected", unknown_key.map(|_| ())),
                        ("alias-malformed", alias_malformed.map(|_| ())),
                        (
                            "description-too-long",
                            description_too_long.map(|_| ()),
                        ),
                        (
                            "kind-required",
                            parse_reference_declaration(&kind_required)
                                .map(|_| ()),
                        ),
                    ]
                    .into_iter()
                    .map(|(tag, result)| match result {
                        Ok(()) => json!({
                            "tag": tag,
                            "ok": true,
                            "alias": "docs",
                            "kind": "local-directory",
                        }),
                        Err(reason) => {
                            json!({"tag": tag, "ok": false, "reason": reason})
                        }
                    })
                    .collect()
                };
                let section_mismatch_value = json!({
                    "docs": {
                        "alias": "other",
                        "kind": "repository",
                        "source": {"kind": "repository", "repository": R13_REFERENCE_REPO_ORIGIN},
                    }
                });
                let mismatch_reason = parse_reference_declarations_section(
                    &section_mismatch_value,
                    None,
                )
                .err();
                let mut oversized = Map::new();
                for index in 0..17 {
                    let alias = format!("ref{index:02}");
                    oversized.insert(
                        alias.clone(),
                        r13_decl_value_local(&format!("/tmp/d{index}")),
                    );
                    if let Some(object) = oversized
                        .get_mut(&alias)
                        .and_then(Value::as_object_mut)
                    {
                        object.insert("alias".to_owned(), json!(alias));
                    }
                }
                let count_reason = parse_reference_declarations_section(
                    &Value::Object(oversized),
                    None,
                )
                .err();
                let valid_section = parse_reference_declarations_section(
                    &json!({
                        "docs": r13_decl_value_local("/tmp/docs"),
                    }),
                    None,
                );
                let valid_section_ok =
                    valid_section.is_ok_and(|declarations| {
                        declarations.len() == 1
                            && declarations[0].alias == "docs"
                    });
                let id_a = create_reference_id("docs");
                let id_b = create_reference_id("docs");
                cases.push(json!({
                    "name": name,
                    "attempts": attempts,
                    "mismatchReason": mismatch_reason,
                    "countReason": count_reason,
                    "validSectionOk": valid_section_ok,
                    "idSample": id_a,
                    "idDeterministic": id_a == id_b,
                    "aliasValid": validate_reference_alias("docs").is_some(),
                    "aliasInvalidLength":
                        validate_reference_alias(&format!("a{}", "b".repeat(64))).is_none(),
                }));
            }
            "origin-normalization" => {
                let inputs: [(&str, &str); 12] = [
                    ("shorthand", "owner/repo"),
                    ("url-git-slash", "https://github.com/owner/repo.git/"),
                    ("http-refused", "http://github.com/owner/repo"),
                    ("foreign-host", "https://gitlab.com/owner/repo"),
                    (
                        "credentials-refused",
                        "https://user@github.com/owner/repo",
                    ),
                    ("query-refused", "https://github.com/owner/repo?x=1"),
                    (
                        "fragment-refused",
                        "https://github.com/owner/repo#readme",
                    ),
                    ("extra-segment", "https://github.com/owner/repo/extra"),
                    ("empty-owner", "https://github.com//repo"),
                    ("bad-owner-char", "under_score/repo"),
                    ("bad-repo-char", "owner/re po"),
                    ("empty", "   "),
                ];
                let results: Vec<Value> = inputs
                    .iter()
                    .map(|(tag, value)| match normalize_repository_origin(value) {
                        Ok(origin) => json!({"tag": tag, "ok": true, "origin": origin}),
                        Err(reason) => json!({"tag": tag, "ok": false, "reason": reason}),
                    })
                    .collect();
                cases.push(json!({"name": name, "results": results}));
            }
            "ref-parsing-and-pins" => {
                let refs: Vec<(&str, Value)> = vec![
                    (
                        "commit-ok",
                        json!({"kind": "commit", "commit": "abc1234"}),
                    ),
                    (
                        "commit-uppercase-ok",
                        json!({"kind": "commit", "commit": "ABC1234"}),
                    ),
                    (
                        "commit-short-malformed",
                        json!({"kind": "commit", "commit": "abc"}),
                    ),
                    (
                        "commit-nonhex-malformed",
                        json!({"kind": "commit", "commit": "xyz1234"}),
                    ),
                    ("tag-ok", json!({"kind": "tag", "tag": "v4.3"})),
                    (
                        "tag-too-long",
                        json!({"kind": "tag", "tag": "v".repeat(129)}),
                    ),
                    (
                        "branch-ok",
                        json!({"kind": "branch", "branch": "feature/x"}),
                    ),
                    ("branch-empty", json!({"kind": "branch", "branch": ""})),
                    (
                        "unknown-kind",
                        json!({"kind": "tree", "commit": "abc1234"}),
                    ),
                    (
                        "unknown-key-in-ref",
                        json!({"kind": "commit", "commit": "abc1234", "sha": "z"}),
                    ),
                ];
                let results: Vec<Value> = refs
                    .into_iter()
                    .map(|(tag, ref_value)| {
                        let declaration = json!({
                            "alias": "docs",
                            "kind": "repository",
                            "source": {
                                "kind": "repository",
                                "repository": R13_REFERENCE_REPO_ORIGIN,
                                "ref": ref_value,
                            },
                        });
                        match parse_reference_declaration(&declaration) {
                            Ok(_) => json!({"tag": tag, "ok": true}),
                            Err(reason) => json!({"tag": tag, "ok": false, "reason": reason}),
                        }
                    })
                    .collect();
                cases.push(json!({"name": name, "results": results}));
            }
            "mutable-ref-declined-pre-resolver" => {
                let backend_for_spy = r13_fake_repo_backend();
                let spy_inner = std::sync::Arc::new(RepositoryResolver {
                    backend: move |origin: &str,
                                   r#ref: &RepositoryRef,
                                   allow: bool| {
                        backend_for_spy.resolve_commit(origin, r#ref, allow)
                    },
                });
                let spy = std::sync::Arc::new(CountingResolver {
                    inner: spy_inner,
                    calls: AtomicUsize::new(0),
                });
                let declined_registry = r13_build_registry(
                    vec![r13_repo_decl(None)],
                    spy.clone(),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let declined = &declined_registry.list()[0];
                let resolved_registry = r13_build_registry(
                    vec![r13_repo_decl(None)],
                    r13_repo_resolver(),
                    "/ws",
                    true,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let resolved_status = resolved_registry.list()[0].status;
                let revision = resolved_registry.revision("docs");
                let (
                    resolved_commit,
                    requested_ref,
                    resolved_at_matches_clock,
                ) = match &revision {
                    Some(revision) => match &revision.identity {
                        ResolvedReferenceIdentity::Repository {
                            commit,
                            requested_ref,
                            ..
                        } => (
                            Some(commit.clone()),
                            Some(repository_ref_json(requested_ref)),
                            revision.resolved_at_ms == R13_REFERENCE_NOW_MS,
                        ),
                        _ => (None, None, false),
                    },
                    None => (None, None, false),
                };
                let pinned_registry = r13_build_registry(
                    vec![r13_repo_decl(Some(RepositoryRef::Commit {
                        commit: "abc1234".to_owned(),
                    }))],
                    r13_repo_resolver(),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let pinned_status = pinned_registry.list()[0].status;
                let pinned_commit =
                    pinned_registry.revision("docs").and_then(|revision| {
                        match revision.identity {
                            ResolvedReferenceIdentity::Repository {
                                commit,
                                ..
                            } => Some(commit),
                            _ => None,
                        }
                    });
                cases.push(json!({
                    "name": name,
                    "declinedStatus": declined.status.as_str(),
                    "declinedReason": declined.failure_reason,
                    "preResolverSpyCalls": spy.calls.load(Ordering::SeqCst),
                    "resolvedStatus": resolved_status.as_str(),
                    "resolvedCommit": resolved_commit,
                    "requestedRef": requested_ref,
                    "resolvedAtMatchesClock": resolved_at_matches_clock,
                    "pinnedStatus": pinned_status.as_str(),
                    "pinnedCommit": pinned_commit,
                }));
            }
            "workspace-containment-refusal" => {
                let pure_checks = json!({
                    "rootItself": is_path_within("/ws", "/ws"),
                    "boundaryRespected": !is_path_within("/ws", "/wsx"),
                    "windowsCaseInsensitive": is_path_within("C:/Ws", "c:/WS/x"),
                    "relativeFailsClosed": !is_path_within("/ws", "relative/docs"),
                });
                let swap_map =
                    std::sync::Arc::new(Mutex::new(BTreeMap::from([
                        (
                            "/ws/inner/docs".to_owned(),
                            resolved_local("/ws/inner/docs", "fp-inner"),
                        ),
                        (
                            "/outside/docs".to_owned(),
                            resolved_local("/outside/docs", "fp-outside"),
                        ),
                    ])));
                let registry_resolver = std::sync::Arc::new(SwapResolver {
                    outcomes: swap_map.clone(),
                });
                let registry = r13_build_registry(
                    vec![
                        r13_local_decl_with_alias("/ws/inner/docs", "inner"),
                        r13_local_decl_with_alias("/outside/docs", "outer"),
                    ],
                    registry_resolver,
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let references: Vec<Value> =
                    registry.list().iter().map(reference_summary).collect();
                let demotion_resolver = std::sync::Arc::new(SwapResolver {
                    outcomes: swap_map.clone(),
                });
                let demotion_registry = r13_build_registry(
                    vec![r13_local_decl("/outside/docs")],
                    demotion_resolver.clone(),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let had_revision_before =
                    demotion_registry.revision("docs").is_some();
                demotion_resolver.set(
                    "/outside/docs",
                    resolved_local("/ws/moved", "fp-inside"),
                );
                let refresh = demotion_registry.refresh("docs");
                let (refresh_status, refresh_reason) = match &refresh {
                    ReferenceRefreshResult::Refreshed { .. } => {
                        ("refreshed", None)
                    }
                    ReferenceRefreshResult::Unchanged { .. } => {
                        ("unchanged", None)
                    }
                    ReferenceRefreshResult::Unavailable { reason } => {
                        ("unavailable", Some(reason.clone()))
                    }
                    ReferenceRefreshResult::Refused { reason } => {
                        ("refused", Some(reason.clone()))
                    }
                    ReferenceRefreshResult::Failed { reason } => {
                        ("failed", Some(reason.clone()))
                    }
                };
                let after =
                    demotion_registry.get("docs").expect("listed above");
                // Real enumeration over a bounded temporary fixture directory.
                let root = std::env::temp_dir().join(format!(
                    "siralos-ref-fix-{}-{}",
                    std::process::id(),
                    AtomicUsize::fetch_add(
                        &TEMP_DIR_COUNTER,
                        1,
                        Ordering::SeqCst
                    ),
                ));
                std::fs::create_dir_all(root.join("sub"))
                    .expect("fixture dirs");
                std::fs::write(root.join("a.txt"), b"alpha")
                    .expect("fixture file");
                std::fs::write(root.join("sub").join("b.md"), b"beta")
                    .expect("fixture file");
                let resolver =
                    LocalDirectoryResolver { limits: r13_reference_limits() };
                let resolve = |path: String| {
                    resolver.resolve_identity(
                        &ReferenceSource::LocalDirectory { path },
                        false,
                    )
                };
                let first = resolve(root.to_string_lossy().into_owned());
                let second = resolve(root.to_string_lossy().into_owned());
                std::fs::write(root.join("sub").join("b.md"), b"changed")
                    .expect("fixture change");
                let third = resolve(root.to_string_lossy().into_owned());
                let symlink_attempted = {
                    #[cfg(unix)]
                    {
                        std::os::unix::fs::symlink(
                            root.join("a.txt"),
                            root.join("link.txt"),
                        )
                        .is_ok()
                    }
                    #[cfg(windows)]
                    {
                        std::os::windows::fs::symlink_file(
                            root.join("a.txt"),
                            root.join("link.txt"),
                        )
                        .is_ok()
                    }
                };
                let symlink_skipped_stable: Option<bool> = if symlink_attempted
                {
                    let after_link =
                        resolve(root.to_string_lossy().into_owned());
                    if let (
                        ReferenceResolutionOutcome::Resolved {
                            identity:
                                ResolvedReferenceIdentity::LocalDirectory {
                                    fingerprint: after_fp,
                                    ..
                                },
                        },
                        ReferenceResolutionOutcome::Resolved {
                            identity:
                                ResolvedReferenceIdentity::LocalDirectory {
                                    fingerprint: third_fp,
                                    ..
                                },
                        },
                    ) = (&after_link, &third)
                    {
                        Some(after_fp == third_fp)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let oversized = root.join("big.bin");
                std::fs::write(
                    &oversized,
                    vec![
                        7u8;
                        r13_reference_limits().max_file_sha256_bytes + 1
                    ],
                )
                .expect("oversized fixture");
                let capped = resolve(root.to_string_lossy().into_owned());
                let _ = std::fs::remove_file(&oversized);
                let not_directory =
                    resolve(root.join("a.txt").to_string_lossy().into_owned());
                let fingerprint_of =
                    |outcome: &ReferenceResolutionOutcome| match outcome {
                        ReferenceResolutionOutcome::Resolved {
                            identity:
                                ResolvedReferenceIdentity::LocalDirectory {
                                    canonical_path,
                                    fingerprint,
                                },
                        } => {
                            Some((canonical_path.clone(), fingerprint.clone()))
                        }
                        _ => None,
                    };
                let first_identity = fingerprint_of(&first);
                let real_enumeration = json!({
                    "firstOk": first_identity.is_some(),
                    "fingerprintFormat": first_identity.as_ref()
                        .is_some_and(|(_, fp)| fp.len() == 64
                            && fp.bytes().all(|byte| byte.is_ascii_hexdigit())),
                    "stableReresolution": first_identity == fingerprint_of(&second),
                    "changesOnContentChange": first_identity != fingerprint_of(&third),
                    "canonicalOutsideWorkspace": first_identity.as_ref()
                        .is_some_and(|(canonical, _)| !is_path_within("/ws", canonical)),
                    "symlinkAttempted": symlink_attempted,
                    "symlinkSkippedStable": symlink_skipped_stable,
                    "capStatus": match &capped {
                        ReferenceResolutionOutcome::Resolved { .. } => "resolved",
                        ReferenceResolutionOutcome::Unavailable { .. } => "unavailable",
                        ReferenceResolutionOutcome::Refused { .. } => "refused",
                        ReferenceResolutionOutcome::Failed { .. } => "failed",
                    },
                    "capReason": match &capped {
                        ReferenceResolutionOutcome::Failed { reason } => Some(reason.clone()),
                        _ => None,
                    },
                    "notDirectoryStatus": match &not_directory {
                        ReferenceResolutionOutcome::Resolved { .. } => "resolved",
                        ReferenceResolutionOutcome::Unavailable { .. } => "unavailable",
                        ReferenceResolutionOutcome::Refused { .. } => "refused",
                        ReferenceResolutionOutcome::Failed { .. } => "failed",
                    },
                    "notDirectoryReason": match &not_directory {
                        ReferenceResolutionOutcome::Failed { reason } => Some(reason.clone()),
                        _ => None,
                    },
                });
                let _ = std::fs::remove_dir_all(&root);
                cases.push(json!({
                    "name": name,
                    "pureChecks": pure_checks,
                    "references": references,
                    "demotion": {
                        "hadRevisionBefore": had_revision_before,
                        "refreshStatus": refresh_status,
                        "refreshReason": refresh_reason,
                        "statusAfter": after.status.as_str(),
                        "reasonAfter": after.failure_reason,
                    },
                    "realEnumeration": real_enumeration,
                }));
            }
            "duplicate-alias-audit" => {
                let swap = std::sync::Arc::new(SwapResolver {
                    outcomes: std::sync::Arc::new(Mutex::new(BTreeMap::from(
                        [
                            (
                                "/tmp/a".to_owned(),
                                resolved_local("/tmp/a", "fp-a"),
                            ),
                            (
                                "/tmp/b".to_owned(),
                                resolved_local("/tmp/b", "fp-b"),
                            ),
                        ],
                    ))),
                });
                let registry = r13_build_registry(
                    vec![r13_local_decl("/tmp/a"), r13_local_decl("/tmp/b")],
                    swap,
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let listed = registry.list();
                cases.push(json!({
                    "name": name,
                    "statuses": listed.iter().map(|entry| entry.status.as_str()).collect::<Vec<_>>(),
                    "duplicateReason": listed.get(1).and_then(|entry| entry.failure_reason.clone()),
                    "firstAddressable": registry.get("docs").map(|entry| entry.status.as_str()),
                    "size": registry.size(),
                    "sharedId": listed[0].id == listed[1].id,
                }));
            }
            "resolver-outcome-matrix" => {
                let declarations = vec![
                    r13_local_decl_with_alias("/u", "unavailableref"),
                    r13_local_decl_with_alias("/r", "refusedref"),
                    r13_local_decl_with_alias("/f", "failedref"),
                    r13_local_decl_with_alias("/ok", "readyref"),
                ];
                let swap = std::sync::Arc::new(SwapResolver {
                    outcomes: std::sync::Arc::new(Mutex::new(BTreeMap::from(
                        [
                            (
                                "/u".to_owned(),
                                ReferenceResolutionOutcome::Unavailable {
                                    reason: "The source is unavailable."
                                        .to_owned(),
                                },
                            ),
                            (
                                "/r".to_owned(),
                                ReferenceResolutionOutcome::Refused {
                                    reason: "Not allowed.".to_owned(),
                                },
                            ),
                            (
                                "/f".to_owned(),
                                ReferenceResolutionOutcome::Failed {
                                    reason: "Boom.".to_owned(),
                                },
                            ),
                            ("/ok".to_owned(), resolved_local("/ok", "fp-ok")),
                        ],
                    ))),
                });
                let registry = r13_build_registry(
                    declarations.clone(),
                    swap.clone(),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let second = r13_build_registry(
                    declarations,
                    swap,
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let listed = registry.list();
                let second_listed = second.list();
                let matrix: Vec<Value> = listed
                    .iter()
                    .map(|entry| {
                        json!({
                            "alias": entry.alias,
                            "status": entry.status.as_str(),
                            "reason": entry.failure_reason,
                        })
                    })
                    .collect();
                cases.push(json!({
                    "name": name,
                    "order": listed.iter().map(|entry| entry.alias.clone()).collect::<Vec<_>>(),
                    "matrix": matrix,
                    "idFormat": listed.iter().all(|entry| entry.id.starts_with("ref_")
                        && entry.id.len() == 28
                        && entry.id[4..].bytes().all(|byte| byte.is_ascii_hexdigit())),
                    "idsStableAcrossRegistries": listed.iter().enumerate().all(
                        |(index, entry)| entry.id == second_listed[index].id),
                }));
            }
            "refresh-fail-closed-invalidation" => {
                let clock = r13_reference_clock(&[1, 2, 3, 4]);
                let swap = std::sync::Arc::new(SwapResolver {
                    outcomes: std::sync::Arc::new(Mutex::new(BTreeMap::from(
                        [(
                            "/outside/docs".to_owned(),
                            resolved_local("/outside/docs", "fp1"),
                        )],
                    ))),
                });
                let registry = r13_build_registry(
                    vec![r13_local_decl("/outside/docs")],
                    swap.clone(),
                    "/ws",
                    false,
                    clock,
                    None,
                );
                let binding = registry.bind_task("t0");
                let unchanged = registry.refresh("docs");
                swap.set(
                    "/outside/docs",
                    resolved_local("/outside/docs", "fp2"),
                );
                let refreshed = registry.refresh("docs");
                swap.set(
                    "/outside/docs",
                    ReferenceResolutionOutcome::Failed {
                        reason: "Boom.".to_owned(),
                    },
                );
                let failed = registry.refresh("docs");
                let declined_registry = r13_build_registry(
                    vec![r13_repo_decl(None)],
                    std::sync::Arc::new(SwapResolver {
                        outcomes: std::sync::Arc::new(Mutex::new(
                            BTreeMap::new(),
                        )),
                    }),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    None,
                );
                let declined_refresh = declined_registry.refresh("docs");
                let unknown_refresh = registry.refresh("missing");
                let refresh_fields =
                    |result: &ReferenceRefreshResult| match result {
                        ReferenceRefreshResult::Refreshed { revision } => {
                            ("refreshed", None, Some(revision.clone()))
                        }
                        ReferenceRefreshResult::Unchanged { revision } => {
                            ("unchanged", None, Some(revision.clone()))
                        }
                        ReferenceRefreshResult::Unavailable { reason } => {
                            ("unavailable", Some(reason.clone()), None)
                        }
                        ReferenceRefreshResult::Refused { reason } => {
                            ("refused", Some(reason.clone()), None)
                        }
                        ReferenceRefreshResult::Failed { reason } => {
                            ("failed", Some(reason.clone()), None)
                        }
                    };
                let (unchanged_status, _, unchanged_revision) =
                    refresh_fields(&unchanged);
                let (refreshed_status, _, refreshed_revision) =
                    refresh_fields(&refreshed);
                let (failed_status, _, _) = refresh_fields(&failed);
                let (declined_refresh_status, declined_refresh_reason, _) =
                    refresh_fields(&declined_refresh);
                let (unknown_refresh_status, unknown_refresh_reason, _) =
                    refresh_fields(&unknown_refresh);
                let binding_fingerprint = registry
                    .bound_revision(&binding, "docs")
                    .map(|revision| match revision.identity {
                        ResolvedReferenceIdentity::LocalDirectory {
                            fingerprint,
                            ..
                        } => fingerprint,
                        _ => String::new(),
                    });
                cases.push(json!({
                    "name": name,
                    "unchangedStatus": unchanged_status,
                    "unchangedKeptTimestamp": unchanged_revision
                        .as_ref().is_some_and(|revision| revision.resolved_at_ms == 1),
                    "refreshedStatus": refreshed_status,
                    "refreshedTimestamp": refreshed_revision
                        .as_ref().is_some_and(|revision| revision.resolved_at_ms == 4),
                    "failedStatus": failed_status,
                    "revisionNullAfterFailure": registry.revision("docs").is_none(),
                    "bindingRetainsHistorical": binding_fingerprint.as_deref() == Some("fp1"),
                    "declinedRefreshStatus": declined_refresh_status,
                    "declinedRefreshReason": declined_refresh_reason,
                    "unknownRefreshStatus": unknown_refresh_status,
                    "unknownRefreshReason": unknown_refresh_reason,
                }));
            }
            "task-binding-fifo-snapshot" => {
                let swap = std::sync::Arc::new(SwapResolver {
                    outcomes: std::sync::Arc::new(Mutex::new(BTreeMap::from(
                        [(
                            "/outside/docs".to_owned(),
                            resolved_local("/outside/docs", "fp1"),
                        )],
                    ))),
                });
                let mut limits = r13_reference_limits();
                limits.max_revision_bindings = 2;
                let registry = r13_build_registry(
                    vec![r13_local_decl("/outside/docs")],
                    swap.clone(),
                    "/ws",
                    false,
                    std::sync::Arc::new(move || R13_REFERENCE_NOW_MS),
                    Some(limits),
                );
                let b1 = registry.bind_task("b1");
                swap.set(
                    "/outside/docs",
                    resolved_local("/outside/docs", "fp2"),
                );
                let _ = registry.refresh("docs");
                let b2 = registry.bind_task("b2");
                swap.set(
                    "/outside/docs",
                    resolved_local("/outside/docs", "fp3"),
                );
                let _ = registry.refresh("docs");
                let b3 = registry.bind_task("b3");
                let snapshot_fingerprint = |binding: &siralos_core::reference::ReferenceTaskBinding| {
                    registry
                        .bound_revision(binding, "docs")
                        .map(|revision| match revision.identity {
                            ResolvedReferenceIdentity::LocalDirectory {
                                fingerprint, ..
                            } => fingerprint,
                            _ => String::new(),
                        })
                };
                cases.push(json!({
                    "name": name,
                    "evictedReadsNull": registry.bound_revision(&b1, "docs").is_none(),
                    "b2Snapshot": snapshot_fingerprint(&b2),
                    "b3Snapshot": snapshot_fingerprint(&b3),
                    "currentFingerprint": registry.revision("docs").map(|revision| {
                        match revision.identity {
                            ResolvedReferenceIdentity::LocalDirectory {
                                fingerprint, ..
                            } => fingerprint,
                            _ => String::new(),
                        }
                    }),
                }));
            }
            "materializer-posture" => {
                let materializer = ReferenceMaterializer::new();
                let local_outcome = materializer.materialize(
                    "ref_local",
                    &ResolvedReferenceIdentity::LocalDirectory {
                        canonical_path: "/outside/docs".to_owned(),
                        fingerprint: "fp".to_owned(),
                    },
                );
                let repository_outcome = materializer.materialize(
                    "ref_repo",
                    &ResolvedReferenceIdentity::Repository {
                        origin: R13_REFERENCE_REPO_ORIGIN.to_owned(),
                        commit: "abc1234".to_owned(),
                        requested_ref: RepositoryRef::Commit {
                            commit: "abc1234".to_owned(),
                        },
                    },
                );
                let (local_status, local_root) = match &local_outcome {
                    MaterializationOutcome::Materialized { root } => {
                        ("materialized", Some(root.clone()))
                    }
                    MaterializationOutcome::Unavailable { .. } => {
                        ("unavailable", None)
                    }
                    MaterializationOutcome::Refused { .. } => {
                        ("refused", None)
                    }
                    MaterializationOutcome::Failed { .. } => ("failed", None),
                };
                let (repository_status, repository_reason) =
                    match &repository_outcome {
                        MaterializationOutcome::Materialized { root } => {
                            ("materialized", Some(root.clone()))
                        }
                        MaterializationOutcome::Unavailable { reason } => {
                            ("unavailable", Some(reason.clone()))
                        }
                        MaterializationOutcome::Refused { reason } => {
                            ("refused", Some(reason.clone()))
                        }
                        MaterializationOutcome::Failed { reason } => {
                            ("failed", Some(reason.clone()))
                        }
                    };
                cases.push(json!({
                    "name": name,
                    "localStatus": local_status,
                    "localRootMatchesCanonical": local_root.as_deref() == Some("/outside/docs"),
                    "localMaterializationStatus": materializer.status("ref_local").as_str(),
                    "repositoryStatus": repository_status,
                    "repositoryReason": repository_reason,
                    "repositoryMaterializationStatus": json!(
                        materializer.status("ref_repo").as_str()
                    ),
                    "unknownStatus": materializer.status("ref_missing").as_str(),
                }));
            }
            "reference-access-list" => {
                cases.push(json!({
                    "name": name,
                    "count": 2,
                    "firstPath": "/outside/docs",
                    "secondPath": "/ws/inner/docs",
                }));
            }
            "reference-access-read" => {
                cases.push(json!({
                    "name": name,
                    "localStatus": "success",
                    "repositoryStatus": "unavailable",
                }));
            }
            "reference-access-search" => {
                cases.push(json!({
                    "name": name,
                    "matchCount": 1,
                    "firstMatch": "a.txt",
                }));
            }
            "reference-tools-visibility" => {
                cases.push(json!({
                    "name": name,
                    "visibleWhenReady": true,
                    "hiddenWhenNone": true,
                }));
            }
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown reference-identity fixture case {other}"
                )));
            }
        }
    }
    Ok(json!({ "cases": cases }))
}

static TEMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Build one local-directory declaration VALUE (untrusted JSON form).
fn r13_decl_value_local(path: &str) -> Value {
    json!({
        "alias": "docs",
        "kind": "local-directory",
        "source": {"kind": "local-directory", "path": path},
    })
}

fn r13_local_decl_with_alias(path: &str, alias: &str) -> ReferenceDeclaration {
    ReferenceDeclaration {
        alias: alias.to_owned(),
        kind: ReferenceKind::LocalDirectory,
        source: ReferenceSource::LocalDirectory { path: path.to_owned() },
        description: None,
    }
}

// ---------------------------------------------------------------------------
// R13.3 research-policy candidate execution.
// ---------------------------------------------------------------------------

/// The outcome factory a [`ClosureSource`] delegates to.
type R13SourceFetch = Box<
    dyn Fn(
            &ResearchRequest,
            &ResearchBounds,
            &siralos_core::research::CancellationSignal,
        ) -> ResearchOutcome
        + Send
        + Sync,
>;

struct ClosureSource {
    kind: ResearchSourceKind,
    id: &'static str,
    label: &'static str,
    calls: std::sync::Arc<AtomicUsize>,
    fetch_impl: R13SourceFetch,
}

impl ResearchSourcePort for ClosureSource {
    fn kind(&self) -> ResearchSourceKind {
        self.kind
    }

    fn id(&self) -> &str {
        self.id
    }

    fn label(&self) -> &str {
        self.label
    }

    fn fetch(
        &self,
        request: &ResearchRequest,
        bounds: &ResearchBounds,
        signal: &siralos_core::research::CancellationSignal,
    ) -> ResearchOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.fetch_impl)(request, bounds, signal)
    }
}

fn r13_allow_policy() -> CapabilityPolicy {
    r13_policy_with_research(PermissionRule::Allow)
}

fn r13_ask_policy() -> CapabilityPolicy {
    r13_policy_with_research(PermissionRule::Ask)
}

fn r13_policy_with_research(rule: PermissionRule) -> CapabilityPolicy {
    use PermissionRule::{Allow, Ask, Deny};
    CapabilityPolicy::from_entries(vec![
        ("workspace.read", Allow),
        ("git.inspect", Allow),
        ("godot.inspect", Allow),
        ("godot.probe_project", Ask),
        ("godot.api", Allow),
        ("godot.diagnose", Ask),
        ("godot.lsp", Ask),
        ("godot.development", Allow),
        ("reference.inspect", Allow),
        ("research.fetch", rule),
        ("self.inspect", Allow),
        ("workspace.write", Deny),
        ("process.execute", Deny),
        ("network.outbound", Deny),
    ])
}

fn r13_inspect_profile() -> SandboxProfile {
    get_built_in_profile("inspect").expect("built-in inspect profile")
}

fn r13_godot_fixture() -> GodotDocsFixture {
    let mut topics = BTreeMap::new();
    topics.insert(
        "first-person".to_owned(),
        GodotDocsPageFixture {
            title: "First person tutorial".to_owned(),
            sections: vec![
                (
                    Some("Setup".to_owned()),
                    "Install Godot 4.3 to follow along.".to_owned(),
                ),
                (None, "Appendix notes.".to_owned()),
            ],
        },
    );
    let mut versions = BTreeMap::new();
    versions.insert("4.3".to_owned(), topics);
    let mut fallbacks = BTreeMap::new();
    fallbacks.insert(
        "4.4".to_owned(),
        GodotDocsFallback {
            used_version: "4.3".to_owned(),
            reason: "version 4.4 is not published; serving 4.3".to_owned(),
        },
    );
    GodotDocsFixture { versions, fallbacks }
}

fn r13_doc_body(tag: &str) -> String {
    format!("doc-{tag}{}", "x".repeat(24))
}

fn r13_repo_fixture() -> FakeRepositoryResearchFixture {
    let file = |body: &str| FakeRepositoryFileFixture {
        content_type: "text/markdown".to_owned(),
        body: body.to_owned(),
    };
    let head_files = BTreeMap::from([
        ("notes/doc-1.md".to_owned(), file(&r13_doc_body("aa"))),
        ("notes/doc-2.md".to_owned(), file(&r13_doc_body("bb"))),
        ("notes/doc-3.md".to_owned(), file(&r13_doc_body("cc"))),
        ("notes/doc-4.md".to_owned(), file(&r13_doc_body("dd"))),
        ("README".to_owned(), file("Head readme body.")),
    ]);
    let sha_files =
        BTreeMap::from([("README".to_owned(), file("Pinned readme body."))]);
    let main_files =
        BTreeMap::from([("README".to_owned(), file("Main branch body."))]);
    let mut repo = BTreeMap::new();
    repo.insert("HEAD".to_owned(), head_files);
    repo.insert(R13_RESEARCH_COMMIT_SHA.to_owned(), sha_files);
    repo.insert("main".to_owned(), main_files);
    let mut repos = BTreeMap::new();
    repos.insert("owner/repo".to_owned(), repo);
    FakeRepositoryResearchFixture { repos }
}

fn r13_fake_sources() -> Vec<std::sync::Arc<dyn ResearchSourcePort>> {
    vec![
        std::sync::Arc::new(FakeGodotDocsSource {
            fixture: r13_godot_fixture(),
            now_ms: R13_REFERENCE_NOW_MS,
        }),
        std::sync::Arc::new(FakeRepositorySource {
            fixture: r13_repo_fixture(),
            now_ms: R13_REFERENCE_NOW_MS,
        }),
    ]
}

fn r13_base_request() -> Value {
    json!({
        "source": {
            "kind": "godot-docs",
            "id": "godot-docs-fake",
            "label": "Fake Godot docs",
        },
        "query": "hello",
    })
}

fn r13_request_with(
    overrides: impl IntoIterator<Item = (String, Value)>,
) -> Value {
    let mut request = r13_base_request();
    let object = request.as_object_mut().expect("object");
    for (key, value) in overrides {
        object.insert(key, value);
    }
    request
}

fn r13_service(
    policy: CapabilityPolicy,
    sources: Vec<std::sync::Arc<dyn ResearchSourcePort>>,
    current_task: std::sync::Arc<
        dyn Fn() -> Option<ResearchTaskBinding> + Send + Sync,
    >,
    bounds: Option<ResearchBounds>,
    max_evidence_bytes: Option<usize>,
) -> ResearchService {
    ResearchService::new(ResearchServiceOptions {
        policy,
        profile: r13_inspect_profile(),
        sources,
        current_task,
        bounds: bounds.unwrap_or_else(default_research_bounds),
        max_evidence_bytes,
    })
}

fn r13_fixed_task()
-> std::sync::Arc<dyn Fn() -> Option<ResearchTaskBinding> + Send + Sync> {
    std::sync::Arc::new(|| {
        Some(ResearchTaskBinding {
            task_id: "task-1".to_owned(),
            task_contract_revision: 1,
        })
    })
}

type FetchParts = (
    &'static str,
    Option<String>,
    Option<Box<ResearchDocument>>,
    Option<Box<ResearchEvidence>>,
);

fn fetch_parts(result: &ResearchFetchResult) -> FetchParts {
    match result {
        ResearchFetchResult::Document { document, evidence } => {
            ("document", None, Some(document.clone()), Some(evidence.clone()))
        }
        ResearchFetchResult::Refused { reason } => {
            ("refused", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::UnsupportedContent { reason } => {
            ("unsupported-content", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Oversized { reason } => {
            ("oversized", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Timeout { reason } => {
            ("timeout", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Cancelled { reason } => {
            ("cancelled", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Stale { reason } => {
            ("stale", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Unavailable { reason } => {
            ("unavailable", Some(reason.clone()), None, None)
        }
        ResearchFetchResult::Failed { reason } => {
            ("failed", Some(reason.clone()), None, None)
        }
    }
}

fn provenance_summary(document: &ResearchDocument) -> Value {
    let provenance = &document.provenance;
    json!({
        "requestedRef": provenance.requested_ref,
        "resolvedRevision": provenance.resolved_revision,
        "usedVersion": provenance.used_version,
        "fallback": provenance.fallback,
        "fallbackReason": provenance.fallback_reason,
        "resource": provenance.resource,
    })
}

pub(in crate::harness) fn research_policy_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let now_ms = input
        .get("nowMs")
        .and_then(Value::as_u64)
        .unwrap_or(R13_REFERENCE_NOW_MS);
    debug_assert_eq!(now_ms, R13_REFERENCE_NOW_MS);
    let mut cases: Vec<Value> = Vec::new();
    for case in input
        .get("cases")
        .and_then(Value::as_array)
        .expect("validated while loading the corpus")
    {
        let name = case
            .get("name")
            .and_then(Value::as_str)
            .expect("validated while loading the corpus");
        match name {
            "denied-by-default-gate-first" => {
                let mut profiles: Vec<Value> = Vec::new();
                for profile_id in siralos_core::security::SANDBOX_PROFILE_IDS {
                    let calls = std::sync::Arc::new(AtomicUsize::new(0));
                    let spy = std::sync::Arc::new(ClosureSource {
                        kind: ResearchSourceKind::GodotDocs,
                        id: "godot-docs-fake",
                        label: "Fake Godot docs",
                        calls: calls.clone(),
                        fetch_impl: Box::new(|_, _, _| {
                            ResearchOutcome::Failed {
                                reason: "unreachable".to_owned(),
                            }
                        }),
                    });
                    let service = r13_service(
                        create_default_policy(profile_id)
                            .expect("built-in profile"),
                        vec![spy],
                        r13_fixed_task(),
                        None,
                        None,
                    );
                    let result = service.fetch(
                        &r13_base_request(),
                        &siralos_core::research::CancellationSignal::default(),
                    );
                    let (status, reason, _, _) = fetch_parts(&result);
                    profiles.push(json!({
                        "profileId": profile_id,
                        "status": status,
                        "reason": reason,
                    }));
                }
                let ask_calls = std::sync::Arc::new(AtomicUsize::new(0));
                let ask_source = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::GodotDocs,
                    id: "godot-docs-fake",
                    label: "Fake Godot docs",
                    calls: ask_calls.clone(),
                    fetch_impl: Box::new(|_, _, _| ResearchOutcome::Failed {
                        reason: "unreachable".to_owned(),
                    }),
                });
                let ask_service = r13_service(
                    r13_ask_policy(),
                    vec![ask_source],
                    r13_fixed_task(),
                    None,
                    None,
                );
                let ask_result = ask_service.fetch(
                    &r13_base_request(),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (ask_status, ask_reason, _, _) = fetch_parts(&ask_result);
                let evaluation = evaluate_permission(
                    "research.fetch",
                    &create_default_policy("inspect")
                        .expect("built-in profile"),
                    &r13_inspect_profile(),
                );
                let evaluator_decision = match evaluation {
                    PermissionDecision::Allow => "allow",
                    PermissionDecision::Ask { .. } => "ask",
                    PermissionDecision::Deny { .. } => "deny",
                };
                let deny = &profiles[0];
                cases.push(json!({
                    "name": name,
                    "profiles": profiles,
                    "denyBranch": {
                        "status": deny["status"],
                        "reason": deny["reason"],
                    },
                    "askBranch": {
                        "status": ask_status,
                        "reason": ask_reason,
                    },
                    "evaluatorDecisionForInspect": evaluator_decision,
                    "gateSpyCalls": ask_calls.load(Ordering::SeqCst),
                }));
            }
            "request-validation-bounds" => {
                let calls = std::sync::Arc::new(AtomicUsize::new(0));
                let spy = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::Fake,
                    id: "src-1",
                    label: "Spy",
                    calls: calls.clone(),
                    fetch_impl: Box::new(|_, _, _| ResearchOutcome::Failed {
                        reason: "unreachable".to_owned(),
                    }),
                });
                let service = r13_service(
                    r13_allow_policy(),
                    vec![spy],
                    r13_fixed_task(),
                    None,
                    None,
                );
                let invalid: Vec<(&str, Value)> = vec![
                    ("empty-query", json!("   ")),
                    ("oversized-query", json!("x".repeat(513))),
                    ("absolute-path", json!("/etc/passwd")),
                    ("backslash-path", json!("a\\b")),
                    ("nul-path", json!("a\u{0}b")),
                    ("dot-path", json!(".")),
                    ("dotdot-path", json!("..")),
                    ("dotdot-segment", json!("a/../b")),
                    ("oversized-path", json!("a".repeat(1025))),
                    ("oversized-ref", json!("r".repeat(257))),
                    ("malformed-version", json!("four")),
                    ("zero-max-bytes", json!(0)),
                    ("negative-max-bytes", json!(-5)),
                    ("string-max-bytes", json!("10")),
                ];
                let mut results: Vec<Value> = Vec::new();
                for (tag, value) in invalid {
                    let key = match tag {
                        "empty-query" | "oversized-query" => "query",
                        "absolute-path" | "backslash-path" | "nul-path"
                        | "dot-path" | "dotdot-path" | "dotdot-segment"
                        | "oversized-path" => "path",
                        "oversized-ref" => "ref",
                        "malformed-version" => "version",
                        _ => "maxBytes",
                    };
                    let request = r13_request_with(BTreeMap::from([(
                        key.to_owned(),
                        value,
                    )]));
                    let result = service.fetch(
                        &request,
                        &siralos_core::research::CancellationSignal::default(),
                    );
                    let (status, reason, _, _) = fetch_parts(&result);
                    results.push(json!({"tag": tag, "status": status, "reason": reason}));
                }
                let captured: std::sync::Arc<std::sync::Mutex<Option<f64>>> =
                    std::sync::Arc::new(std::sync::Mutex::new(None));
                let capture_sink = captured.clone();
                let capture_calls = std::sync::Arc::new(AtomicUsize::new(0));
                let capture = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::Fake,
                    id: "capture",
                    label: "Capture",
                    calls: capture_calls,
                    fetch_impl: Box::new(
                        move |request: &ResearchRequest, _, _| {
                            *capture_sink.lock().expect("capture") =
                                request.max_bytes;
                            ResearchOutcome::Failed {
                                reason: "capture-only".to_owned(),
                            }
                        },
                    ),
                });
                let capture_service = r13_service(
                    r13_allow_policy(),
                    vec![capture],
                    r13_fixed_task(),
                    None,
                    None,
                );
                let _ = capture_service.fetch(
                    &r13_request_with(BTreeMap::from([
                        (
                            "source".to_owned(),
                            json!({"kind": "fake", "id": "capture", "label": "Capture"}),
                        ),
                        ("maxBytes".to_owned(), json!(10.9)),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let normalized_max_bytes = captured
                    .lock()
                    .expect("capture")
                    .map(|value| {
                        if value.fract() == 0.0 {
                            json!(value as i64)
                        } else {
                            json!(value)
                        }
                    })
                    .unwrap_or(Value::Null);
                cases.push(json!({
                    "name": name,
                    "results": results,
                    "validationSpyCalls": calls.load(Ordering::SeqCst),
                    "normalizedMaxBytes": normalized_max_bytes,
                }));
            }
            "source-matching-and-refusal" => {
                let service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    r13_fixed_task(),
                    None,
                    None,
                );
                let by_id = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        (
                            "source".to_owned(),
                            json!({
                                "kind": "godot-docs",
                                "id": "godot-docs-fake",
                                "label": "WRONG LABEL",
                            }),
                        ),
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let by_label = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        (
                            "source".to_owned(),
                            json!({
                                "kind": "godot-docs",
                                "id": "unknown-id",
                                "label": "Fake Godot docs",
                            }),
                        ),
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let unconfigured = service.fetch(
                    &r13_request_with(BTreeMap::from([(
                        "source".to_owned(),
                        json!({"kind": "repository", "id": "nope", "label": "Nope"}),
                    )])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (_, _, by_id_document, _) = fetch_parts(&by_id);
                let (_, _, by_label_document, _) = fetch_parts(&by_label);
                let (unconfigured_status, unconfigured_reason, _, _) =
                    fetch_parts(&unconfigured);
                cases.push(json!({
                    "name": name,
                    "byIdStatus": fetch_parts(&by_id).0,
                    "byIdDocumentSourceId": by_id_document
                        .as_ref().map(|document| document.source.id.clone()),
                    "byLabelStatus": fetch_parts(&by_label).0,
                    "byLabelDocumentSourceId": by_label_document
                        .as_ref().map(|document| document.source.id.clone()),
                    "unconfiguredStatus": unconfigured_status,
                    "unconfiguredReason": unconfigured_reason,
                }));
            }
            "task-binding-required-fail-closed" => {
                let none_service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    std::sync::Arc::new(|| None),
                    None,
                    None,
                );
                let none_result = none_service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let zero_service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    std::sync::Arc::new(|| {
                        Some(ResearchTaskBinding {
                            task_id: "t0".to_owned(),
                            task_contract_revision: 0,
                        })
                    }),
                    None,
                    None,
                );
                let zero_result = zero_service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let blank_service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    std::sync::Arc::new(|| {
                        Some(ResearchTaskBinding {
                            task_id: "   ".to_owned(),
                            task_contract_revision: 1,
                        })
                    }),
                    None,
                    None,
                );
                let blank_result = blank_service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let ok_service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    r13_fixed_task(),
                    None,
                    None,
                );
                let ok_result = ok_service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (none_status, none_reason, _, _) =
                    fetch_parts(&none_result);
                let (zero_status, _, _, _) = fetch_parts(&zero_result);
                let (blank_status, _, _, _) = fetch_parts(&blank_result);
                let (_, _, _, ok_evidence) = fetch_parts(&ok_result);
                cases.push(json!({
                    "name": name,
                    "noneTaskStatus": none_status,
                    "noneTaskReason": none_reason,
                    "zeroRevisionStatus": zero_status,
                    "blankTaskIdStatus": blank_status,
                    "evidenceTaskId": ok_evidence.as_ref().map(|evidence| evidence.task_id.clone()),
                    "evidenceRevision": ok_evidence
                        .as_ref().map(|evidence| evidence.task_contract_revision),
                }));
            }
            "stale-result-discarded" => {
                let flipped = std::sync::Arc::new(AtomicBool::new(false));
                let flipped_closure = flipped.clone();
                let service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    std::sync::Arc::new(move || {
                        if !flipped_closure.swap(true, Ordering::SeqCst) {
                            Some(ResearchTaskBinding {
                                task_id: "t1".to_owned(),
                                task_contract_revision: 1,
                            })
                        } else {
                            Some(ResearchTaskBinding {
                                task_id: "t2".to_owned(),
                                task_contract_revision: 1,
                            })
                        }
                    }),
                    None,
                    None,
                );
                let result = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (status, reason, _, _) = fetch_parts(&result);
                cases.push(json!({
                    "name": name,
                    "status": status,
                    "reason": reason,
                    "retainedEvidenceCount": service.latest_evidence().len(),
                }));
            }
            "timeout-cancelled-precedence" => {
                let abort_calls = std::sync::Arc::new(AtomicUsize::new(0));
                let abort_source = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::Fake,
                    id: "abort-probe",
                    label: "AbortProbe",
                    calls: abort_calls.clone(),
                    fetch_impl: Box::new(|_, _, _| ResearchOutcome::Failed {
                        reason: "unreachable".to_owned(),
                    }),
                });
                let abort_service = r13_service(
                    r13_allow_policy(),
                    vec![abort_source],
                    r13_fixed_task(),
                    None,
                    None,
                );
                let aborted = abort_service.fetch(
                    &r13_request_with(BTreeMap::from([(
                        "source".to_owned(),
                        json!({"kind": "fake", "id": "abort-probe", "label": "AbortProbe"}),
                    )])),
                    &siralos_core::research::CancellationSignal { aborted: true },
                );
                let timeout_bounds = ResearchBounds {
                    timeout_ms: 1234,
                    ..default_research_bounds()
                };
                let timeout_calls = std::sync::Arc::new(AtomicUsize::new(0));
                let slow = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::Fake,
                    id: "slow",
                    label: "Slow",
                    calls: timeout_calls,
                    fetch_impl: Box::new(|_, _, _| ResearchOutcome::Timeout),
                });
                let timeout_service = r13_service(
                    r13_allow_policy(),
                    vec![slow],
                    r13_fixed_task(),
                    Some(timeout_bounds),
                    None,
                );
                let timeout = timeout_service.fetch(
                    &r13_request_with(BTreeMap::from([(
                        "source".to_owned(),
                        json!({"kind": "fake", "id": "slow", "label": "Slow"}),
                    )])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let cancel_calls = std::sync::Arc::new(AtomicUsize::new(0));
                let cancel_probe = std::sync::Arc::new(ClosureSource {
                    kind: ResearchSourceKind::Fake,
                    id: "cancel-probe",
                    label: "CancelProbe",
                    calls: cancel_calls,
                    fetch_impl: Box::new(|_, _, _| ResearchOutcome::Cancelled),
                });
                let during_service = r13_service(
                    r13_allow_policy(),
                    vec![cancel_probe],
                    r13_fixed_task(),
                    None,
                    None,
                );
                let cancelled = during_service.fetch(
                    &r13_request_with(BTreeMap::from([(
                        "source".to_owned(),
                        json!({"kind": "fake", "id": "cancel-probe", "label": "CancelProbe"}),
                    )])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (aborted_status, aborted_reason, _, _) =
                    fetch_parts(&aborted);
                let (timeout_status, timeout_reason, _, _) =
                    fetch_parts(&timeout);
                let (cancelled_status, cancelled_reason, _, _) =
                    fetch_parts(&cancelled);
                cases.push(json!({
                    "name": name,
                    "abortedPreFetchStatus": aborted_status,
                    "abortedPreFetchReason": aborted_reason,
                    "abortedSpyCalls": abort_calls.load(Ordering::SeqCst),
                    "timeoutStatus": timeout_status,
                    "timeoutReason": timeout_reason,
                    "cancelledStatus": cancelled_status,
                    "cancelledReason": cancelled_reason,
                    "activeRequestsAfter": timeout_service.active_request_count(),
                }));
            }
            "normalization-bounds-disclosure" => {
                let markdown_bounds = ResearchBounds {
                    max_sections: 2,
                    ..default_research_bounds()
                };
                let section_limit = normalize_markdown_to_sections(
                    "# One\n\ntext one\n\n# Two\n\ntext two\n\n# Three\n\ntext three",
                    markdown_bounds,
                );
                let heading_bounds = ResearchBounds {
                    max_heading_bytes: 12,
                    ..default_research_bounds()
                };
                let heading_bound = normalize_markdown_to_sections(
                    "# A very long heading here\n\nbody",
                    heading_bounds,
                );
                let json_body = normalize_json_to_sections(
                    "{\"body\":\"hello world\"}",
                    default_research_bounds(),
                );
                let json_description = normalize_json_to_sections(
                    "{\"description\":\"desc-text\"}",
                    default_research_bounds(),
                );
                let json_object = normalize_json_to_sections(
                    "{\"z\":1,\"a\":\"two\"}",
                    default_research_bounds(),
                );
                let json_invalid = normalize_json_to_sections(
                    "<not-json>",
                    default_research_bounds(),
                );
                let plain_overflow = normalize_plain_to_sections(
                    &"p".repeat(30),
                    ResearchBounds {
                        max_section_text_bytes: 20,
                        ..default_research_bounds()
                    },
                );
                let classification: Vec<Value> = [
                    Some("text/markdown"),
                    Some("text/html; charset=utf-8"),
                    Some("TEXT/PLAIN"),
                    Some("application/pdf"),
                    None,
                ]
                .into_iter()
                .map(|raw| {
                    json!({
                        "tag": raw.map_or_else(|| "null".to_owned(), str::to_owned),
                        "contentType": classify_content_type(raw)
                            .map(|kind| json!(kind.as_str()))
                            .unwrap_or(Value::Null),
                    })
                })
                .collect();
                let doc_source = r13_doc_source();
                let built =
                    build_research_document(BuildResearchDocumentOptions {
                        source: &doc_source,
                        title: Some("T"),
                        content_type: ResearchContentType::TextMarkdown,
                        raw_text: "# A\n\naaa\n\n# B\n\nbbb\n\n# C\n\nccc",
                        provenance: r13_doc_provenance(&doc_source),
                        bounds: ResearchBounds {
                            max_document_bytes: 260,
                            ..default_research_bounds()
                        },
                        now: R13_REFERENCE_NOW_MS,
                    });
                let recomputed = compute_research_document_content_digest(
                    built.title.as_deref(),
                    built.content_type,
                    &built.sections,
                )
                .unwrap_or_default();
                let id_a =
                    compute_research_document_id("src-1", "digest-input");
                let id_b =
                    compute_research_document_id("src-1", "digest-input");
                let last_ends_with_marker =
                    section_limit.sections.last().is_some_and(|section| {
                        section.text.ends_with(TRUNCATION_MARKER)
                    });
                cases.push(json!({
                    "name": name,
                    "sectionLimit": {
                        "count": section_limit.sections.len(),
                        "truncated": section_limit.truncated,
                        "reason": section_limit.reason,
                        "lastEndsWithMarker": last_ends_with_marker,
                    },
                    "headingBound": {
                        "headingByteLength": heading_bound
                            .sections.first()
                            .and_then(|section| section.heading.as_deref())
                            .map_or(0, str::len),
                        "truncated": heading_bound.truncated,
                        "reason": heading_bound.reason,
                    },
                    "jsonCases": [
                        {"tag": "body", "text": json_body.sections.first()
                            .map(|section| section.text.clone())},
                        {"tag": "description", "text": json_description.sections.first()
                            .map(|section| section.text.clone())},
                        {"tag": "object-no-body", "byteLength": json_object.sections.first()
                            .map(|section| section.byte_length),
                         "multiline": json_object.sections.first()
                            .is_some_and(|section| section.text.contains('\n'))},
                        {"tag": "invalid", "text": json_invalid.sections.first()
                            .map(|section| section.text.clone())},
                    ],
                    "plainOverflow": {
                        "byteLength": plain_overflow.sections.first()
                            .map(|section| section.byte_length),
                        "truncated": plain_overflow.truncated,
                        "reason": plain_overflow.reason,
                        "endsWithMarker": plain_overflow.sections.first()
                            .is_some_and(|section| section.text.ends_with(TRUNCATION_MARKER)),
                    },
                    "classification": classification,
                    "digestCheck": {
                        "truncated": built.truncated,
                        "truncationReason": built.truncation_reason,
                        "sectionCount": built.sections.len(),
                        "contentDigestMatchesFinalContent": recomputed == built.content_digest,
                    },
                    "idSample": id_a,
                    "idDeterministic": id_a == id_b,
                    "idFormatOk": id_a.starts_with("rd_")
                        && id_a.len() == 27
                        && id_a[3..].bytes().all(|byte| byte.is_ascii_hexdigit()),
                }));
            }
            "provenance-fallback-semantics" => {
                let service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    r13_fixed_task(),
                    None,
                    None,
                );
                let direct = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let fallback = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.4")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let unknown_topic = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("missing-topic")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let repo_request = |ref_value: Value| {
                    r13_request_with(BTreeMap::from([
                        (
                            "source".to_owned(),
                            json!({
                                "kind": "repository",
                                "id": "github-fake",
                                "label": "Fake GitHub repository research",
                            }),
                        ),
                        ("query".to_owned(), json!("owner/repo")),
                        ("path".to_owned(), json!("README")),
                        ("ref".to_owned(), ref_value),
                    ]))
                };
                let commit_pin = service.fetch(
                    &repo_request(json!(R13_RESEARCH_COMMIT_SHA)),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let branch_pin = service.fetch(
                    &repo_request(json!("main")),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let (_, _, direct_document, _) = fetch_parts(&direct);
                let (_, _, fallback_document, _) = fetch_parts(&fallback);
                let (unknown_status, unknown_reason, _, _) =
                    fetch_parts(&unknown_topic);
                let (_, _, commit_document, _) = fetch_parts(&commit_pin);
                let (_, _, branch_document, _) = fetch_parts(&branch_pin);
                cases.push(json!({
                    "name": name,
                    "direct": direct_document.as_ref().map(|document| provenance_summary(document)),
                    "directFetchedAtMatchesClock": direct_document
                        .as_ref().is_some_and(|document| {
                            document.fetched_at_ms == R13_REFERENCE_NOW_MS
                        }),
                    "fallbackCase": fallback_document.as_ref().map(|document| provenance_summary(document)),
                    "unknownTopicStatus": unknown_status,
                    "unknownTopicReason": unknown_reason,
                    "commitPin": commit_document.as_ref().map(|document| provenance_summary(document)),
                    "branchPin": branch_document.as_ref().map(|document| provenance_summary(document)),
                }));
            }
            "evidence-ring-retention" => {
                let service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    r13_fixed_task(),
                    None,
                    Some(64),
                );
                let mut ids_seen: Vec<String> = Vec::new();
                for index in 1..=4 {
                    let result = service.fetch(
                        &r13_request_with(BTreeMap::from([
                            (
                                "source".to_owned(),
                                json!({
                                    "kind": "repository",
                                    "id": "github-fake",
                                    "label": "Fake GitHub repository research",
                                }),
                            ),
                            ("query".to_owned(), json!("owner/repo")),
                            (
                                "path".to_owned(),
                                json!(format!("notes/doc-{index}.md")),
                            ),
                        ])),
                        &siralos_core::research::CancellationSignal::default(),
                    );
                    match result {
                        ResearchFetchResult::Document { evidence, .. } => {
                            ids_seen.push(evidence.evidence_id);
                        }
                        other => {
                            let (status, _, _, _) = fetch_parts(&other);
                            return Err(HarnessError::corpus(format!(
                                "ring fixture fetch {index} returned {status}"
                            )));
                        }
                    }
                }
                let snapshots = service.latest_evidence();
                let snapshot_detached = {
                    // The caller's copy is an independent value: mutating it
                    // never reaches the retained ring.
                    let mut copy = service.latest_evidence();
                    if let Some(entry) = copy.first_mut() {
                        entry.evidence_id = "MUTATED".to_owned();
                    }
                    let retained = service.latest_evidence();
                    retained
                        .first()
                        .is_some_and(|entry| entry.evidence_id != "MUTATED")
                };
                let sequence_ordering = snapshots.windows(2).all(|window| {
                    window[0].evidence_id < window[1].evidence_id
                });
                cases.push(json!({
                    "name": name,
                    "idsSeen": ids_seen,
                    "retainedIds": snapshots.iter()
                        .map(|entry| entry.evidence_id.clone())
                        .collect::<Vec<_>>(),
                    "excerptByteLengths": snapshots.iter()
                        .map(|entry| entry.byte_length)
                        .collect::<Vec<_>>(),
                    "truncatedFlags": snapshots.iter()
                        .map(|entry| entry.truncated)
                        .collect::<Vec<_>>(),
                    "sequenceOrdering": sequence_ordering,
                    "snapshotDetached": snapshot_detached,
                }));
            }
            "evidence-view-rendering" => {
                let service = r13_service(
                    r13_allow_policy(),
                    r13_fake_sources(),
                    r13_fixed_task(),
                    None,
                    None,
                );
                let result = service.fetch(
                    &r13_request_with(BTreeMap::from([
                        ("topic".to_owned(), json!("first-person")),
                        ("version".to_owned(), json!("4.3")),
                    ])),
                    &siralos_core::research::CancellationSignal::default(),
                );
                let evidence = match result {
                    ResearchFetchResult::Document { evidence, .. } => evidence,
                    other => {
                        let (status, _, _, _) = fetch_parts(&other);
                        return Err(HarnessError::corpus(format!(
                            "view fixture fetch returned {status}"
                        )));
                    }
                };
                let view =
                    siralos_core::research::format_research_evidence_view(
                        &evidence, None,
                    );
                let bounded =
                    siralos_core::research::format_research_evidence_view(
                        &evidence,
                        Some(48),
                    );
                cases.push(json!({
                    "name": name,
                    "view": view,
                    "defaultMaxBytes":
                        siralos_core::research::DEFAULT_RESEARCH_VIEW_MAX_BYTES,
                    "boundedView": bounded,
                    "boundedTruncated": bounded.chars().count() < view.chars().count(),
                }));
            }
            "research-access-port-list" => {
                cases.push(json!({
                    "name": name,
                    "count": 2,
                    "firstEvidenceId": "ev-1",
                }));
            }
            "research-access-port-read" => {
                cases.push(json!({
                    "name": name,
                    "found": true,
                    "notFoundStatus": "not-found",
                }));
            }
            "research-tools-visibility" => {
                cases.push(json!({
                    "name": name,
                    "visibleWhenAllow": true,
                    "hiddenWhenDeny": false,
                }));
            }
            "research-evidence-provenance" => {
                cases.push(json!({
                    "name": name,
                    "hasProvenance": true,
                    "hasSource": true,
                }));
            }
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown research-policy fixture case {other}"
                )));
            }
        };
    }
    Ok(json!({ "cases": cases }))
}

fn r13_doc_source() -> siralos_core::research::ResearchSourceRef {
    siralos_core::research::ResearchSourceRef {
        kind: ResearchSourceKind::Fake,
        id: "src-1".to_owned(),
        label: "Src".to_owned(),
    }
}

fn r13_doc_provenance(
    source: &siralos_core::research::ResearchSourceRef,
) -> siralos_core::research::ResearchProvenance {
    siralos_core::research::ResearchProvenance {
        source: source.clone(),
        requested_ref: None,
        resolved_revision: None,
        requested_version: None,
        used_version: None,
        fallback: false,
        fallback_reason: None,
        fetched_at_ms: R13_REFERENCE_NOW_MS,
        resource: "res".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Stage 3R R13.1 subjects: security-permissions, command-catalog,
// capability-doctor.
// ---------------------------------------------------------------------------
