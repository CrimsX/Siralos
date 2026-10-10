//! Workspace-ops subjects: fixture workspace setup and read/list/search/prepare/apply/revision/checkpoint records.
use crate::harness::*;

/// Deterministic fixture content generation mirroring the oracle probe.
fn fixture_bytes(spec: &Value) -> Result<Vec<u8>, HarnessError> {
    if let Some(content) = spec.get("content").and_then(Value::as_str) {
        return Ok(content.as_bytes().to_vec());
    }
    if let Some(bytes) = spec.get("bytes").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(bytes.len());
        for byte in bytes {
            let value = byte.as_u64().ok_or_else(|| {
                HarnessError::corpus(
                    "fixture bytes must be non-negative integers",
                )
            })?;
            out.push(value.min(255) as u8);
        }
        return Ok(out);
    }
    if let Some(kind) = spec.get("kind").and_then(Value::as_str) {
        match kind {
            "nul-after-probe" => {
                let mut out = vec![0x61; 9000];
                out.push(0);
                out.extend_from_slice(b"b");
                return Ok(out);
            }
            "crlf" => return Ok(b"a\r\nb\r\n".to_vec()),
            "unicode" => {
                return Ok("héllo wörld\nsnowman ☃\nemoji 😀\n"
                    .as_bytes()
                    .to_vec());
            }
            "many-lines" => {
                let lines: Vec<String> =
                    (1..=300).map(|index| format!("line {index}")).collect();
                return Ok(lines.join("\n").into_bytes());
            }
            "empty" => return Ok(Vec::new()),
            "no-trailing-newline" => return Ok(b"hello".to_vec()),
            _ => {}
        }
    }
    if let Some(size) = spec.get("size").and_then(Value::as_u64) {
        let fill =
            spec.get("fill").and_then(Value::as_str).ok_or_else(|| {
                HarnessError::corpus("size fixture requires a fill character")
            })?;
        let byte = fill.as_bytes().first().copied().unwrap_or(b'x');
        return Ok(vec![byte; size as usize]);
    }
    Err(HarnessError::corpus("unsupported fixture spec"))
}

/// Create the fixture workspace tree and return its canonical root.
pub(in crate::harness) fn create_fixture_workspace(
    input: &Value,
    label: &str,
) -> Result<PathBuf, HarnessError> {
    let root = std::env::temp_dir().join(format!("siralos-cand-ws-{label}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create fixture workspace: {error}"),
        )
    })?;
    if let Ok(files) = scenario_array(input, "files") {
        for spec in files {
            if spec.get("kind").and_then(Value::as_str) == Some("bulk") {
                let directory = root.join(scenario_string(spec, "path")?);
                let count = scenario_u64(spec, "count").ok_or_else(|| {
                    HarnessError::corpus("bulk fixture requires a count")
                })?;
                let content = spec
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        HarnessError::corpus("bulk fixture requires content")
                    })?;
                std::fs::create_dir_all(&directory).map_err(|error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!(
                            "cannot create bulk fixture directory: {error}"
                        ),
                    )
                })?;
                for index in 0..count {
                    let name = format!("f{index:03}.txt");
                    std::fs::write(directory.join(name), content).map_err(
                        |error| {
                            HarnessError::new(
                                HarnessErrorKind::ProbeSpawn,
                                format!("cannot write bulk fixture: {error}"),
                            )
                        },
                    )?;
                }
                continue;
            }
            let path = scenario_string(spec, "path")?;
            let target = root.join(&path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!("cannot create fixture parent: {error}"),
                    )
                })?;
            }
            std::fs::write(&target, fixture_bytes(spec)?).map_err(
                |error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!("cannot write fixture: {error}"),
                    )
                },
            )?;
        }
    }
    if let Ok(symlinks) = scenario_array(input, "symlinks") {
        for spec in symlinks {
            let link = root.join(scenario_string(spec, "link")?);
            let target = scenario_string(spec, "target")?;
            let directory =
                spec.get("directory").and_then(Value::as_bool) == Some(true);
            let target_path = if let Some(relative) =
                target.strip_prefix("../")
            {
                let outside =
                    root.parent().map(|parent| parent.join(relative));
                if let Some(outside) = &outside {
                    if directory {
                        let _ = std::fs::create_dir_all(outside);
                        let _ = std::fs::write(
                            outside.join("secret.txt"),
                            b"outside secret\n",
                        );
                    } else {
                        let _ = std::fs::write(outside, b"outside secret\n");
                    }
                }
                outside.unwrap_or_else(|| root.join(&target))
            } else {
                root.join(&target)
            };
            if let Some(parent) = link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target_path, &link);
            }
            #[cfg(windows)]
            {
                if directory {
                    // The oracle's Node `symlinkSync(target, link)` auto-detects
                    // a directory target and creates a directory symlink. A file
                    // symlink to a directory makes `std::fs::canonicalize` fail
                    // on windows-latest, which previously reported
                    // `unresolvable` where the reference reports the canonical
                    // escape (`outside_workspace`).
                    let _ =
                        std::os::windows::fs::symlink_dir(&target_path, &link);
                } else {
                    let _ = std::os::windows::fs::symlink_file(
                        &target_path,
                        &link,
                    );
                }
            }
        }
    }
    std::fs::canonicalize(&root).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot canonicalize fixture workspace: {error}"),
        )
    })
}

/// Run one R4 record builder under a fixture workspace with cleanup.
pub(in crate::harness) fn with_fixture_workspace(
    scenario_id: &str,
    input: &Value,
    build: impl FnOnce(&Path) -> Result<Value, HarnessError>,
) -> Result<Value, HarnessError> {
    let root = create_fixture_workspace(input, scenario_id)?;
    let outcome = build(&root);
    let _ = std::fs::remove_dir_all(&root);
    outcome
}

/// Canonical code for a read denial/failure message (mirror of the
/// oracle probe message mapping).
fn read_code(message: &str) -> &'static str {
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    if message.contains("Cannot inspect file") {
        return "inspect_failed";
    }
    if message.contains("Target is not a regular file.") {
        return "not_file";
    }
    if message.contains("File is too large") {
        return "too_large";
    }
    if message.contains("Cannot read file") {
        return "unreadable";
    }
    if message.contains("File appears to be binary.") {
        return "binary";
    }
    if message.contains("File is not valid UTF-8 text.") {
        return "not_utf8";
    }
    if message.contains("beyond the end of the file") {
        return "start_beyond";
    }
    "inspect_failed"
}

fn list_code(message: &str) -> &'static str {
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    if message.contains("Target is not a directory.") {
        return "not_directory";
    }
    if message.contains("Cannot inspect directory") {
        return "inspect_failed";
    }
    if message.contains("Cannot list directory") {
        return "list_failed";
    }
    if message.contains("Cannot inspect entry") {
        return "entry_inspect_failed";
    }
    "list_failed"
}

fn search_code(message: &str) -> &'static str {
    if message.contains("Tool input must be a JSON object.") {
        return "not_an_object";
    }
    if message.contains("\"query\" is required.") {
        return "query_required";
    }
    if message.contains("\"query\" must be a string.") {
        return "query_not_string";
    }
    if message.contains("\"path\" must be a string.") {
        return "path_not_string";
    }
    if message.contains("\"maxResults\" must be a positive integer.") {
        return "max_results_invalid";
    }
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    "query_required"
}

fn read_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let mut registry =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: fingerprint,
            max_entries: None,
        })
        .map_err(HarnessError::corpus)?;
    let mut reads: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "reads")? {
        let parsed = parse_read_input(entry);
        let request_path =
            entry.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(_) => {
                reads.push(json!({
                    "path": request_path,
                    "status": "invalid_input",
                    "code": "invalid_input",
                }));
                continue;
            }
        };
        let outcome = read_file(
            root,
            &parsed,
            &WORKSPACE_LIMITS,
            Some(&mut registry),
            false,
        );
        match outcome {
            ReadOutcome::Success {
                path: _,
                sha256,
                revision,
                content,
                start_line,
                end_line,
                total_lines,
                truncated,
            } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "success",
                    "sha256": sha256,
                    "revision": revision,
                    "content": content,
                    "startLine": start_line,
                    "endLine": end_line,
                    "totalLines": total_lines,
                    "truncated": truncated,
                }));
            }
            ReadOutcome::Unsupported {
                path: _,
                mode,
                revision,
                supported,
                reason,
            } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "success",
                    "mode": mode.as_str(),
                    "revision": revision,
                    "supported": supported,
                    "reason": reason,
                }));
            }
            ReadOutcome::Denied { message } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "denied",
                    "code": read_code(&message),
                }));
            }
            ReadOutcome::Failed { message } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "failed",
                    "code": read_code(&message),
                }));
            }
            ReadOutcome::InvalidInput { message } => {
                let _ = message;
                reads.push(json!({
                    "path": request_path,
                    "status": "invalid_input",
                    "code": "invalid_input",
                }));
            }
            ReadOutcome::Cancelled => {
                reads.push(
                    json!({ "path": request_path, "status": "cancelled" }),
                );
            }
        }
    }
    for entry in input
        .get("cancelledReads")
        .and_then(Value::as_array)
        .unwrap_or(&vec![])
    {
        let request_path =
            entry.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
        match parse_read_input(entry) {
            Ok(parsed) => {
                let outcome =
                    read_file(root, &parsed, &WORKSPACE_LIMITS, None, true);
                match outcome {
                    ReadOutcome::Cancelled => {
                        reads.push(json!({ "path": request_path, "status": "cancelled" }));
                    }
                    _ => {
                        reads.push(json!({ "path": request_path, "status": "failed" }));
                    }
                }
            }
            Err(_) => {
                reads.push(json!({ "path": request_path, "status": "invalid_input", "code": "invalid_input" }));
            }
        }
    }
    Ok(json!({ "reads": reads }))
}

fn list_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut lists: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "lists")? {
        let requested = match entry.get("path") {
            None => ".".to_owned(),
            Some(Value::String(value)) => value.clone(),
            Some(_) => {
                lists.push(json!({ "path": ".", "status": "invalid_input", "code": "invalid_input" }));
                continue;
            }
        };
        let outcome = list_directory(root, &requested, &WORKSPACE_LIMITS);
        match outcome {
            ListOutcome::Success { path, entries, truncated } => {
                let entries: Vec<Value> = entries
                    .into_iter()
                    .map(|entry| {
                        let (kind, size) = match entry.kind {
                            siralos_adapters::workspace::list::EntryKind::File { size } => {
                                ("file", Some(size))
                            }
                            siralos_adapters::workspace::list::EntryKind::Directory => ("directory", None),
                            siralos_adapters::workspace::list::EntryKind::Symlink => ("symlink", None),
                            siralos_adapters::workspace::list::EntryKind::Other => ("other", None),
                        };
                        match size {
                            Some(size) => json!({
                                "name": entry.name,
                                "path": entry.path,
                                "type": kind,
                                "size": size,
                            }),
                            None => json!({
                                "name": entry.name,
                                "path": entry.path,
                                "type": kind,
                            }),
                        }
                    })
                    .collect();
                lists.push(json!({
                    "path": requested,
                    "status": "success",
                    "resolvedPath": path,
                    "entries": entries,
                    "truncated": truncated,
                }));
            }
            ListOutcome::Denied { message } => {
                lists.push(json!({
                    "path": requested,
                    "status": "denied",
                    "code": list_code(&message),
                }));
            }
            ListOutcome::Failed { message } => {
                lists.push(json!({
                    "path": requested,
                    "status": "failed",
                    "code": list_code(&message),
                }));
            }
        }
    }
    Ok(json!({ "lists": lists }))
}

fn search_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut searches: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "searches")? {
        let parsed = match parse_search_input(entry, &WORKSPACE_LIMITS) {
            Ok(parsed) => parsed,
            Err(message) => {
                let query = entry
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                searches.push(json!({
                    "query": query,
                    "status": "invalid_input",
                    "code": search_code(&message),
                }));
                continue;
            }
        };
        let outcome = search(root, &parsed, &WORKSPACE_LIMITS, false);
        match outcome {
            SearchOutcome::Success {
                query,
                path,
                matches,
                scanned_files,
                skipped_files,
                truncated,
                truncation_reason,
            } => {
                let matches: Vec<Value> = matches
                    .into_iter()
                    .map(|match_entry| {
                        json!({
                            "path": match_entry.path,
                            "line": match_entry.line,
                            "column": match_entry.column,
                            "text": match_entry.text,
                        })
                    })
                    .collect();
                searches.push(json!({
                    "query": query,
                    "status": "success",
                    "path": path,
                    "matches": matches,
                    "scannedFiles": scanned_files,
                    "skippedFiles": skipped_files,
                    "truncated": truncated,
                    "truncationReason": truncation_reason.map(|reason| reason.as_str()),
                }));
            }
            SearchOutcome::Denied { message } => {
                let query = parsed.query;
                searches.push(json!({
                    "query": query,
                    "status": "denied",
                    "code": search_code(&message),
                }));
            }
            SearchOutcome::Cancelled => {
                searches.push(
                    json!({ "query": parsed.query, "status": "cancelled" }),
                );
            }
            SearchOutcome::InvalidInput { message } => {
                searches.push(json!({
                    "query": parsed.query,
                    "status": "invalid_input",
                    "code": search_code(&message),
                }));
            }
        }
    }
    Ok(json!({ "searches": searches }))
}

fn prepare_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut prepares: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "prepares")? {
        let tool_name = scenario_string(entry, "tool")?;
        let tool = match tool_name.as_str() {
            "workspace.create_file" => MutationTool::CreateFile,
            "workspace.edit_file" => MutationTool::EditFile,
            "workspace.delete_file" => MutationTool::DeleteFile,
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown mutation tool {other}"
                )));
            }
        };
        let cancelled =
            entry.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
        match prepare_mutation(tool, cancelled) {
            PreparationOutcome::Unavailable { .. } => {
                prepares.push(json!({
                    "tool": tool_name,
                    "status": "unavailable",
                    "code": "mutation_unavailable",
                }));
            }
            PreparationOutcome::Cancelled { .. } => {
                prepares
                    .push(json!({ "tool": tool_name, "status": "cancelled" }));
            }
        }
    }
    let verify_path = scenario_string(input, "verifyPath")?;
    let verify = read_file(
        root,
        &ReadInput {
            path: verify_path,
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        },
        &WORKSPACE_LIMITS,
        None,
        false,
    );
    let workspace_sha256 = match verify {
        ReadOutcome::Success { sha256, .. } => sha256,
        _ => "missing".to_owned(),
    };
    let checkpoint_root = std::env::temp_dir()
        .join(format!("siralos-cand-cp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    let store =
        open_checkpoint_store(root, &checkpoint_root).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot open fixture checkpoint store: {error}"),
            )
        })?;
    let checkpoint_count = store.list(None, None).len();
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    Ok(json!({
        "prepares": prepares,
        "workspaceSha256": workspace_sha256,
        "checkpointCount": checkpoint_count,
    }))
}

fn apply_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut applies: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "applies")? {
        let tool_name = scenario_string(entry, "tool")?;
        let tool = match tool_name.as_str() {
            "workspace.create_file" => MutationTool::CreateFile,
            "workspace.edit_file" => MutationTool::EditFile,
            "workspace.delete_file" => MutationTool::DeleteFile,
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown mutation tool {other}"
                )));
            }
        };
        // Binding state is deliberately irrelevant here (see the
        // oracle probe): the boundary refuses before any input
        // inspection, so stale or malformed prepared payloads produce
        // the identical typed outcome.
        match apply_mutation(tool) {
            ApplicationOutcome::Unavailable { .. } => {
                applies.push(json!({
                    "tool": tool_name,
                    "status": "unavailable",
                    "code": "mutation_unavailable",
                }));
            }
        }
    }
    let classified: Vec<String> = scenario_array(input, "paths")?
        .iter()
        .filter_map(|value| value.as_str())
        .filter(|path| is_protected_behavioral_config_path(path))
        .map(str::to_owned)
        .collect();
    let verify_path = scenario_string(input, "verifyPath")?;
    let verify = read_file(
        root,
        &ReadInput {
            path: verify_path,
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        },
        &WORKSPACE_LIMITS,
        None,
        false,
    );
    let workspace_sha256 = match verify {
        ReadOutcome::Success { sha256, .. } => sha256,
        _ => "missing".to_owned(),
    };
    let checkpoint_root = std::env::temp_dir()
        .join(format!("siralos-cand-cp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    let store =
        open_checkpoint_store(root, &checkpoint_root).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot open fixture checkpoint store: {error}"),
            )
        })?;
    let checkpoint_count = store.list(None, None).len();
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    Ok(json!({
        "applies": applies,
        "classified": classified,
        "workspaceSha256": workspace_sha256,
        "checkpointCount": checkpoint_count,
    }))
}

fn git_record() -> Value {
    let disposition = git_inspection_disposition();
    match disposition {
        siralos_core::workspace::git::GitInspectionDisposition::Unavailable { code, .. } => json!({
            "disposition": "unavailable",
            "code": code.as_str(),
        }),
    }
}

pub(in crate::harness) fn workspace_record(
    scenario_id: &str,
    subject: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    match subject {
        SUBJECT_WORKSPACE_READ => {
            with_fixture_workspace(scenario_id, input, |root| {
                read_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_LIST => {
            with_fixture_workspace(scenario_id, input, |root| {
                list_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_SEARCH => {
            with_fixture_workspace(scenario_id, input, |root| {
                search_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_PREPARE => {
            with_fixture_workspace(scenario_id, input, |root| {
                prepare_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_APPLY => {
            with_fixture_workspace(scenario_id, input, |root| {
                apply_record(root, input)
            })
        }
        SUBJECT_GIT_INSPECTION => Ok(git_record()),
        _ => Err(HarnessError::corpus(format!(
            "unsupported workspace subject {subject}"
        ))),
    }
}

pub(in crate::harness) fn revision_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let _ = scenario_id;
    let fingerprint = scenario_string(input, "fingerprint")?;
    let limit = scenario_u64(input, "limit").map(|value| value as usize);
    let mut registry =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: fingerprint,
            max_entries: limit,
        })
        .map_err(HarnessError::corpus)?;
    let mut other =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: "fixture-other-workspace".to_owned(),
            max_entries: None,
        })
        .map_err(HarnessError::corpus)?;
    let mut ops: Vec<Value> = Vec::new();
    for op in scenario_array(input, "ops")? {
        let op_name = scenario_string(op, "op")?;
        let result: Value = match op_name.as_str() {
            "issue" => {
                json!(registry.issue(
                    &scenario_string(op, "path")?,
                    &scenario_string(op, "sha256")?,
                ))
            }
            "resolve" => {
                match registry.resolve(&scenario_string(op, "handle")?) {
                    Some(identity) => json!({
                        "workspaceFingerprint": identity.workspace_fingerprint,
                        "path": identity.path,
                        "sha256": identity.sha256,
                    }),
                    None => Value::Null,
                }
            }
            "current" => registry
                .current_revision(&scenario_string(op, "path")?)
                .map(|handle| json!(handle))
                .unwrap_or(Value::Null),
            "state" => registry
                .revision_for_state(
                    &scenario_string(op, "path")?,
                    &scenario_string(op, "sha256")?,
                )
                .map(|handle| json!(handle))
                .unwrap_or(Value::Null),
            "invalidate" => {
                registry.invalidate_path(&scenario_string(op, "path")?);
                Value::Null
            }
            "observe" => {
                let path = scenario_string(op, "path")?;
                if let Some(handle) =
                    registry.current_revision(&path).map(str::to_owned)
                {
                    let mode = match scenario_string(op, "mode")?.as_str() {
                        "exact" => ObservedReadMode::Exact,
                        "structural" => ObservedReadMode::Structural,
                        "summary" => ObservedReadMode::Summary,
                        other => {
                            return Err(HarnessError::corpus(format!(
                                "unsupported read mode {other}"
                            )));
                        }
                    };
                    registry.observe_read(&path, &handle, mode);
                }
                Value::Null
            }
            "observed" => {
                let reads: Vec<Value> = registry
                    .observed_reads()
                    .iter()
                    .map(|read| {
                        json!({
                            "path": read.path,
                            "revision": read.revision,
                            "mode": read.mode.as_str(),
                            "atMs": read.at_ms,
                        })
                    })
                    .collect();
                json!(reads)
            }
            "size" => json!(registry.size()),
            "clear" => {
                registry.clear();
                Value::Null
            }
            "foreign-resolve" => {
                match other.resolve(&scenario_string(op, "handle")?) {
                    Some(identity) => json!({
                        "workspaceFingerprint": identity.workspace_fingerprint,
                        "path": identity.path,
                        "sha256": identity.sha256,
                    }),
                    None => Value::Null,
                }
            }
            "foreign-issue" => json!(other.issue(
                &scenario_string(op, "path")?,
                &scenario_string(op, "sha256")?,
            )),
            "compute" => json!(compute_workspace_revision_handle(
                &scenario_string(op, "workspace")?,
                &scenario_string(op, "path")?,
                &scenario_string(op, "sha256")?,
            )),
            other => {
                return Err(HarnessError::corpus(format!(
                    "unsupported revision op {other}"
                )));
            }
        };
        ops.push(json!({ "op": op_name, "result": result }));
    }
    Ok(json!({ "ops": ops }))
}

fn checkpoint_json(checkpoint: &FileCheckpoint, fingerprint: &str) -> Value {
    let before = &checkpoint.before;
    let after = &checkpoint.after;
    json!({
        "id": checkpoint.id,
        "operation": checkpoint.operation.as_str(),
        "state": checkpoint.state.as_str(),
        "relativePath": checkpoint.relative_path,
        "before": {
            "exists": before.exists,
            "sha256": before.sha256,
            "byteLength": before.byte_length,
        },
        "after": {
            "exists": after.exists,
            "sha256": after.sha256,
            "byteLength": after.byte_length,
        },
        "preview": {
            "addedLines": checkpoint.preview.added_lines,
            "removedLines": checkpoint.preview.removed_lines,
        },
        "fingerprintValid": checkpoint.workspace_fingerprint == fingerprint,
    })
}

fn write_fixture_checkpoint(
    store_root: &Path,
    fingerprint: &str,
    spec: &Value,
) -> Result<(), HarnessError> {
    let id = scenario_string(spec, "id")?;
    let directory = store_root.join(fingerprint).join(&id);
    std::fs::create_dir_all(&directory).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create fixture checkpoint directory: {error}"),
        )
    })?;
    if let Some(raw) = spec.get("raw").and_then(Value::as_str) {
        std::fs::write(directory.join("metadata.json"), raw).map_err(
            |error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!(
                        "cannot write fixture checkpoint metadata: {error}"
                    ),
                )
            },
        )?;
        return Ok(());
    }
    if let Some(record_json) = spec.get("recordJson").and_then(Value::as_str) {
        let resolved = record_json.replace("__FINGERPRINT__", fingerprint);
        std::fs::write(directory.join("metadata.json"), resolved).map_err(
            |error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!(
                        "cannot write fixture checkpoint metadata: {error}"
                    ),
                )
            },
        )?;
        return Ok(());
    }
    let record = scenario_object(spec, "record")?;
    let stored_fingerprint =
        if spec.get("foreignFingerprint").and_then(Value::as_bool)
            == Some(true)
        {
            "0".repeat(64)
        } else {
            fingerprint.to_owned()
        };
    let stored = json!({
        "version": 1,
        "id": id,
        "workspaceFingerprint": stored_fingerprint,
        "relativePath": record.get("relativePath").and_then(Value::as_str).unwrap_or(""),
        "operation": record.get("operation").and_then(Value::as_str).unwrap_or(""),
        "toolName": record.get("toolName").and_then(Value::as_str).unwrap_or(""),
        "createdAt": record.get("createdAt").and_then(Value::as_str).unwrap_or(""),
        "state": record.get("state").and_then(Value::as_str).unwrap_or(""),
        "before": record.get("before").cloned().unwrap_or(Value::Null),
        "after": record.get("after").cloned().unwrap_or(Value::Null),
        "preview": record.get("preview").cloned().unwrap_or(Value::Null),
    });
    let serialized =
        serde_json::to_string_pretty(&stored).map_err(|error| {
            HarnessError::corpus(format!(
                "cannot serialize fixture checkpoint: {error}"
            ))
        })?;
    std::fs::write(directory.join("metadata.json"), format!("{serialized}\n"))
        .map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot write fixture checkpoint metadata: {error}"),
            )
        })?;
    Ok(())
}

pub(in crate::harness) fn checkpoint_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let workspace =
        std::env::temp_dir().join(format!("siralos-cand-cpws-{scenario_id}"));
    let store_root = std::env::temp_dir()
        .join(format!("siralos-cand-cproot-{scenario_id}"));
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&store_root);
    std::fs::create_dir_all(&workspace).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create checkpoint fixture workspace: {error}"),
        )
    })?;
    if let Ok(files) = scenario_object(input, "workspaceFiles") {
        for (path, content) in files {
            let target = workspace.join(path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, content.as_str().unwrap_or("")).map_err(
                |error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!(
                            "cannot write checkpoint fixture file: {error}"
                        ),
                    )
                },
            )?;
        }
    }
    if let Ok(symlinks) = scenario_array(input, "workspaceSymlinks") {
        for spec in symlinks {
            let link = workspace.join(scenario_string(spec, "link")?);
            let target = scenario_string(spec, "target")?;
            let directory =
                spec.get("directory").and_then(Value::as_bool) == Some(true);
            let target_path = if let Some(relative) =
                target.strip_prefix("../")
            {
                let outside =
                    workspace.parent().map(|parent| parent.join(relative));
                if let Some(outside) = &outside {
                    if directory {
                        let _ = std::fs::create_dir_all(outside);
                        let _ = std::fs::write(
                            outside.join("secret.txt"),
                            b"outside secret\n",
                        );
                    } else {
                        let _ = std::fs::write(outside, b"outside secret\n");
                    }
                }
                outside.unwrap_or_else(|| workspace.join(&target))
            } else {
                workspace.join(&target)
            };
            if let Some(parent) = link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target_path, &link);
            }
            #[cfg(windows)]
            {
                if directory {
                    // The oracle's Node `symlinkSync(target, link)` auto-detects
                    // a directory target and creates a directory symlink. A file
                    // symlink to a directory makes `std::fs::canonicalize` fail
                    // on windows-latest, which previously reported
                    // `unresolvable` where the reference reports the canonical
                    // escape (`outside_workspace`).
                    let _ =
                        std::os::windows::fs::symlink_dir(&target_path, &link);
                } else {
                    let _ = std::os::windows::fs::symlink_file(
                        &target_path,
                        &link,
                    );
                }
            }
        }
    }
    let store =
        open_checkpoint_store(&workspace, &store_root).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot open fixture checkpoint store: {error}"),
            )
        })?;
    let fingerprint = store.workspace_fingerprint().to_owned();
    if let Ok(checkpoints) = scenario_array(input, "checkpoints") {
        for spec in checkpoints {
            write_fixture_checkpoint(&store_root, &fingerprint, spec)?;
        }
    }
    let mut ops: Vec<Value> = Vec::new();
    for op in scenario_array(input, "ops")? {
        let op_name = scenario_string(op, "op")?;
        match op_name.as_str() {
            "list" | "list-after" => {
                let states =
                    op.get("states").and_then(Value::as_array).map(|states| {
                        states
                            .iter()
                            .filter_map(|state| {
                                CheckpointState::parse(state.as_str()?)
                            })
                            .collect::<Vec<CheckpointState>>()
                    });
                let checkpoints = store.list(states.as_deref(), None);
                let checkpoints: Vec<Value> = checkpoints
                    .iter()
                    .map(|checkpoint| {
                        checkpoint_json(checkpoint, &fingerprint)
                    })
                    .collect();
                ops.push(json!({ "op": op_name, "checkpoints": checkpoints }));
            }
            "get" => {
                let checkpoint = store.get(&scenario_string(op, "id")?);
                ops.push(json!({
                    "op": "get",
                    "checkpoint": checkpoint.as_ref().map(|c| checkpoint_json(c, &fingerprint)),
                }));
            }
            "reconcile" => {
                let report = reconcile_checkpoints(&store, 1024 * 1024);
                ops.push(json!({
                    "op": "reconcile",
                    "checked": report.checked,
                    "abandoned": report.abandoned,
                    "applied": report.applied,
                    "uncertain": report.uncertain,
                    "undoneAfterRestore": report.undone_after_restore,
                }));
            }
            "undo-plan" => {
                let id = scenario_string(op, "id")?;
                let checkpoint = store.get(&id).ok_or_else(|| {
                    HarnessError::corpus(format!(
                        "undo-plan requires a valid checkpoint {id}"
                    ))
                })?;
                let current = scenario_object(op, "current")?;
                let current_state = WorkspaceFileState {
                    exists: current
                        .get("exists")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    sha256: current
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
                let decision = match plan_undo(&checkpoint, &current_state) {
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyCreate => "ready_create",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyRestore => "ready_restore",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyDelete => "ready_delete",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::Conflict => "conflict",
                };
                ops.push(json!({ "op": "undo-plan", "decision": decision }));
            }
            other => {
                return Err(HarnessError::corpus(format!(
                    "unsupported checkpoint op {other}"
                )));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&store_root);
    Ok(json!({ "ops": ops }))
}
// Stage 3R R5 subjects: language-diagnostics / language-structure /
// language-definition (generic language intelligence parity).
// ---------------------------------------------------------------------------
//
// Executes each R5 scenario against the real siralos-core language
// modules (plus the generic URI mapping in siralos-adapters) and builds
// the canonical R5 observation object that the TypeScript oracle probe
// emits for the same inputs. The observation is semantic parity data,
// never a TypeScript object layout.
