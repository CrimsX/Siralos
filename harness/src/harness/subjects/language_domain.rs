//! Language and domain-neutral subjects: diagnostics, structure, definition, lifecycle, and capability.
use crate::harness::*;

/// One normalized diagnostic as a canonical record value.
fn diagnostic_value(diagnostic: &Diagnostic) -> Value {
    json!({
        "source": diagnostic.source,
        "severity": diagnostic.severity.as_str(),
        "path": diagnostic.path,
        "line": diagnostic.line,
        "column": diagnostic.column,
        "code": diagnostic.code,
        "message": diagnostic.message,
        "rawCategory": diagnostic.raw_category,
    })
}

/// Extract a raw 0-based position (None fields for malformed values,
/// mirroring the reference typeof/integer checks).
fn raw_position(value: &Value) -> RawPosition {
    let line = match value.get("line") {
        Some(Value::Number(number)) => {
            number.as_i64().filter(|line| *line >= 0)
        }
        _ => None,
    };
    let column = match value.get("character") {
        Some(Value::Number(number)) => {
            number.as_i64().filter(|column| *column >= 0)
        }
        _ => None,
    };
    RawPosition { line, column }
}

/// Extract a raw 0-based range; None when the value is not an object or
/// a position is absent.
fn raw_range(value: &Value) -> Option<RawRange> {
    let object = value.as_object()?;
    Some(RawRange {
        start: raw_position(object.get("start")?),
        end: raw_position(object.get("end")?),
    })
}

/// Parse one raw LSP-shaped diagnostic entry from the scenario input.
fn parse_raw_diagnostic(entry: &Value) -> Result<RawDiagnostic, HarnessError> {
    let severity = entry.get("severity").and_then(Value::as_i64);
    let code = match entry.get("code") {
        Some(Value::String(text)) => {
            Some(RawDiagnosticCode::Text(text.clone()))
        }
        Some(Value::Number(number)) => match number.as_i64() {
            Some(value) => Some(RawDiagnosticCode::Number(value)),
            None => Some(RawDiagnosticCode::Text(number.to_string())),
        },
        _ => None,
    };
    let message =
        entry.get("message").and_then(Value::as_str).map(str::to_owned);
    let source =
        entry.get("source").and_then(Value::as_str).map(str::to_owned);
    let range = entry.get("range").and_then(raw_range);
    Ok(RawDiagnostic { range, severity, code, message, source })
}

/// Canonical record for one language-diagnostics scenario.
pub(in crate::harness) fn language_diagnostics_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let root = scenario_string(input, "root")?;
    let source = scenario_string(input, "source")?;
    let run_max = scenario_u64(input, "runMax").map(|value| value as usize);
    let mut documents: Vec<Value> = Vec::new();
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    for document in scenario_array(input, "documents")? {
        let uri = scenario_string(document, "uri")?;
        let raw_entries = match document.get("diagnostics") {
            Some(Value::Array(entries)) => entries,
            _ => {
                // A non-array payload is rejected exactly like the
                // reference publish-diagnostics normalization.
                documents.push(json!({ "uri": uri, "status": "rejected" }));
                continue;
            }
        };
        let Some(path) = map_uri_to_workspace_relative(&uri, &root) else {
            // Out-of-workspace URIs are rejected, never guessed.
            documents.push(json!({ "uri": uri, "status": "rejected" }));
            continue;
        };
        let raw = raw_entries
            .iter()
            .map(parse_raw_diagnostic)
            .collect::<Result<Vec<_>, _>>()?;
        let per_set = scenario_u64(document, "max")
            .map(|value| value as usize)
            .unwrap_or(LANGUAGE_LIMITS.max_diagnostics_per_set);
        let limits = siralos_core::language::limits::LanguageLimits {
            max_diagnostics_per_set: per_set,
            ..LANGUAGE_LIMITS
        };
        let payload = normalize_diagnostic_payload(
            &raw,
            &source,
            &path,
            Some(&root),
            &limits,
        );
        let revision = match document.get("sha256") {
            Some(Value::String(sha256)) => {
                Some(compute_workspace_revision_handle(
                    &fingerprint,
                    &payload.path,
                    sha256,
                ))
            }
            _ => None,
        };
        let diagnostics = payload
            .diagnostics
            .iter()
            .map(diagnostic_value)
            .collect::<Vec<_>>();
        documents.push(json!({
            "uri": uri,
            "status": "normalized",
            "path": payload.path,
            "revision": revision,
            "diagnostics": diagnostics,
            "truncated": payload.truncated,
        }));
        all_diagnostics.extend(payload.diagnostics);
    }
    let run_bound = run_max.unwrap_or(LANGUAGE_LIMITS.max_diagnostics_per_run);
    let (aggregated, aggregated_truncated) =
        normalize_diagnostic_set(all_diagnostics, run_bound);
    let aggregate = json!({
        "diagnostics": aggregated.iter().map(diagnostic_value).collect::<Vec<_>>(),
        "truncated": aggregated_truncated,
    });
    Ok(json!({ "documents": documents, "aggregate": aggregate }))
}

/// Parse one generic structural declaration (language-neutral).
fn parse_declaration(value: &Value) -> StructuralDeclaration {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .and_then(StructuralKind::parse)
        .unwrap_or(StructuralKind::Other);
    let name = value.get("name").and_then(Value::as_str).map(str::to_owned);
    let detail =
        value.get("detail").and_then(Value::as_str).map(str::to_owned);
    let line =
        value.get("line").and_then(Value::as_u64).filter(|line| *line >= 1);
    let attributes = parse_string_array(value.get("attributes"));
    let children = value
        .get("children")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().map(parse_declaration).collect())
        .unwrap_or_default();
    StructuralDeclaration { kind, name, detail, line, attributes, children }
}

/// Parse an opaque bounded string array (missing or non-array values
/// are empty; non-string entries are skipped).
fn parse_string_array(value: Option<&Value>) -> Vec<String> {
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

/// Parse the generic structural observation facts. Status and
/// truncation are derived by normalization, never read from the
/// input (the input carries parser facts only).
fn parse_structure(value: &Value) -> StructuralDocument {
    let path =
        value.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
    let declarations = value
        .get("declarations")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().map(parse_declaration).collect())
        .unwrap_or_default();
    let dependencies = parse_string_array(value.get("dependencies"));
    let issues = value
        .get("issues")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| StructuralIssue {
                    line: entry
                        .get("line")
                        .and_then(Value::as_u64)
                        .filter(|line| *line >= 1),
                    message: entry
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    StructuralDocument {
        path,
        revision: None,
        declarations,
        dependencies,
        status: StructureStatus::Complete,
        issues,
        truncated: false,
    }
}

/// Canonical record for one language-structure scenario.
pub(in crate::harness) fn language_structure_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let mut summaries: Vec<Value> = Vec::new();
    for document in scenario_array(input, "documents")? {
        let structure_value = document.get("structure").ok_or_else(|| {
            HarnessError::corpus(
                "language-structure document requires a structure object",
            )
        })?;
        let parsed = parse_structure(structure_value);
        let path = parsed.path.clone();
        let revision = match document.get("sha256") {
            Some(Value::String(sha256)) if !path.is_empty() => Some(
                compute_workspace_revision_handle(&fingerprint, &path, sha256),
            ),
            _ => None,
        };
        let mut normalized = normalize_structural_document(
            &path,
            parsed.declarations,
            parsed.dependencies,
            parsed.issues,
            &StructureOptions::default(),
        );
        normalized.revision = revision.clone();
        let options = SummaryOptions {
            max_bytes: scenario_u64(document, "maxBytes")
                .map(|value| value as usize),
            notable_declarations: scenario_u64(
                document,
                "notableDeclarations",
            )
            .map(|value| value as usize),
        };
        let summary = build_structural_summary(&normalized, &options);
        summaries.push(json!({
            "path": summary.path,
            "revision": summary.revision,
            "mode": "summary",
            "advisory": true,
            "truncated": summary.truncated,
            "bytes": summary.bytes,
            "text": summary.text,
        }));
    }
    Ok(json!({ "summaries": summaries }))
}

fn parse_raw_definition_entry(entry: &Value) -> RawDefinitionEntry {
    let uri = match entry.get("targetUri") {
        Some(Value::String(value)) => Some(value.clone()),
        _ => match entry.get("uri") {
            Some(Value::String(value)) => Some(value.clone()),
            _ => None,
        },
    };
    let range = match entry.get("targetRange") {
        Some(value) => raw_range(value),
        None => entry.get("range").and_then(raw_range),
    };
    RawDefinitionEntry { uri, range }
}

/// Canonical record for one language-definition scenario.
pub(in crate::harness) fn language_definition_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let root = scenario_string(input, "root")?;
    let mut queries: Vec<Value> = Vec::new();
    for query in scenario_array(input, "queries")? {
        let uri = scenario_string(query, "uri")?;
        let fallback = scenario_string(query, "path")?;
        let query_path =
            map_uri_to_workspace_relative(&uri, &root).unwrap_or(fallback);
        let entries = query
            .get("locations")
            .map(|locations| match locations {
                Value::Array(items) => items
                    .iter()
                    .map(parse_raw_definition_entry)
                    .collect::<Vec<_>>(),
                Value::Null => Vec::new(),
                _ => vec![parse_raw_definition_entry(locations)],
            })
            .unwrap_or_default();
        let result = normalize_definition_locations(
            &entries,
            &query_path,
            |target| map_uri_to_workspace_relative(target, &root),
            DefinitionLimits {
                max_locations: scenario_u64(query, "max")
                    .map(|value| value as usize)
                    .unwrap_or(LANGUAGE_LIMITS.max_definition_locations),
            },
        );
        let locations = result
            .locations
            .iter()
            .map(|location| {
                json!({
                    "path": location.path,
                    "range": {
                        "start": {
                            "line": location.range.start.line,
                            "column": location.range.start.column,
                        },
                        "end": {
                            "line": location.range.end.line,
                            "column": location.range.end.column,
                        },
                    },
                    "external": location.external,
                })
            })
            .collect::<Vec<_>>();
        queries.push(json!({
            "uri": uri,
            "path": result.path,
            "locations": locations,
            "truncated": result.truncated,
        }));
    }
    Ok(json!({ "queries": queries }))
}

// ---------------------------------------------------------------------------
// Stage 3R R6 subjects: domain-lifecycle and domain-capability.
// ---------------------------------------------------------------------------
//
// Executes each R6 scenario against the real siralos-core::domain
// lifecycle/capability semantics and builds the canonical R6
// observation object that the TypeScript oracle probe emits for the
// same inputs. The observations are semantic parity data, never a
// TypeScript object layout.

/// Failure record: the stable code plus typed detail fields.
fn module_failure_record(op: &str, failure: &ModuleFailure) -> Value {
    let mut record = serde_json::Map::new();
    record.insert("op".to_owned(), json!(op));
    record.insert("ok".to_owned(), json!(false));
    record.insert("code".to_owned(), json!(failure.code()));
    if let ModuleFailure::CapabilityDenied { missing }
    | ModuleFailure::UndeclaredCapability { missing } = failure
    {
        record.insert(
            "missing".to_owned(),
            json!(missing.iter().map(|id| id.as_str()).collect::<Vec<_>>()),
        );
    }
    Value::Object(record)
}

/// Parse one package descriptor from a scenario value (typed failure,
/// matching the reference parse order: id, digest, abi, capabilities).
fn parse_scenario_package(
    value: &Value,
) -> Result<ModulePackage, ModuleFailure> {
    let id = module_string(value.get("id"))?;
    let digest = module_string(value.get("digest"))?;
    let abi = module_string(value.get("abi"))?;
    let capabilities =
        module_string_array(value.get("requestedCapabilities"))?;
    ModulePackage::parse(&id, &digest, &abi, &capabilities)
}

/// Parse an activation request from a scenario value.
fn parse_scenario_request(
    value: &Value,
) -> Result<ActivationRequest, ModuleFailure> {
    let package_id = module_string(value.get("packageId"))?;
    let digest = module_string(value.get("digest"))?;
    let abi = module_string(value.get("abi"))?;
    let capabilities = module_string_array(value.get("capabilities"))?;
    ActivationRequest::parse(&package_id, &digest, &abi, &capabilities)
}

/// One scenario string field (typed invalid-input failure).
fn module_string(value: Option<&Value>) -> Result<String, ModuleFailure> {
    value.and_then(Value::as_str).map(str::to_owned).ok_or_else(|| {
        ModuleFailure::InvalidInput {
            reason: "missing string field".to_owned(),
        }
    })
}

/// One scenario string array field (typed invalid-input failure).
fn module_string_array(
    value: Option<&Value>,
) -> Result<Vec<String>, ModuleFailure> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| {
                        ModuleFailure::InvalidInput {
                            reason: "array entry is not a string".to_owned(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| {
            Err(ModuleFailure::InvalidInput {
                reason: "array field expected".to_owned(),
            })
        })
}

/// The canonical inspect observation for the current lifecycle state.
fn module_inspect_value(lifecycle: &ModuleLifecycle) -> Value {
    let package = lifecycle.installed_package();
    let active = lifecycle.active();
    json!({
        "op": "inspect",
        "state": lifecycle.state().as_str(),
        "available": lifecycle.available(),
        "enabled": lifecycle.enabled(),
        "active": active.is_some(),
        "package": package.map(|package| json!({
            "id": package.id().as_str(),
            "digest": package.digest().as_str(),
            "abi": package.abi().as_str(),
            "requestedCapabilities": package
                .requested_capabilities()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
        })),
        "activation": active.map(|active| json!({
            "sessionId": active.session_id(),
            "binding": {
                "packageId": active.binding().package_id().as_str(),
                "digest": active.binding().digest().as_str(),
                "abi": active.binding().abi().as_str(),
            },
            "grant": active
                .grant()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
        })),
    })
}

/// Execute one domain-lifecycle operation and return its canonical
/// observation.
fn run_module_lifecycle_op(
    lifecycle: &mut ModuleLifecycle,
    op: &str,
    entry: &Value,
    supported_abi: &ModuleAbi,
    authority: &HostAuthority,
) -> Result<Value, HarnessError> {
    match op {
        "inspect" => Ok(module_inspect_value(lifecycle)),
        "install" => {
            let package = match parse_scenario_package(object_field(
                entry, "package",
            )?) {
                Ok(package) => package,
                Err(failure) => {
                    return Ok(module_failure_record("install", &failure));
                }
            };
            let computed = scenario_string(entry, "computedDigest")?;
            let computed_digest = match PackageDigest::parse(&computed) {
                Ok(digest) => digest,
                Err(failure) => {
                    return Ok(module_failure_record("install", &failure));
                }
            };
            if let Err(failure) =
                verify_package_digest(package.digest(), &computed_digest)
            {
                return Ok(module_failure_record("install", &failure));
            }
            match lifecycle.install(package) {
                Ok(()) => Ok(json!({
                    "op": "install",
                    "ok": true,
                    "state": lifecycle.state().as_str(),
                })),
                Err(failure) => Ok(module_failure_record("install", &failure)),
            }
        }
        "uninstall" | "enable" | "disable" | "deactivate" => {
            let result = match op {
                "uninstall" => lifecycle.uninstall(),
                "enable" => lifecycle.enable(),
                "disable" => lifecycle.disable(),
                _ => lifecycle.deactivate(),
            };
            Ok(match result {
                Ok(()) => json!({ "op": op, "ok": true }),
                Err(failure) => module_failure_record(op, &failure),
            })
        }
        "eligibility" => {
            let request = match parse_scenario_request(object_field(
                entry,
                "activation",
            )?) {
                Ok(request) => request,
                Err(failure) => {
                    return Ok(module_failure_record("eligibility", &failure));
                }
            };
            let runtime = match RuntimeCheckResult::parse(&scenario_string(
                entry, "runtime",
            )?) {
                Ok(runtime) => runtime,
                Err(failure) => {
                    return Ok(module_failure_record("eligibility", &failure));
                }
            };
            let eligibility = lifecycle.eligibility(
                &request,
                supported_abi,
                authority,
                &runtime,
            );
            Ok(json!({
                "op": "eligibility",
                "ready": eligibility.ready(),
                "reasons": eligibility
                    .reasons()
                    .iter()
                    .map(|reason| reason.code())
                    .collect::<Vec<_>>(),
            }))
        }
        "activate" => {
            let request = match parse_scenario_request(object_field(
                entry,
                "activation",
            )?) {
                Ok(request) => request,
                Err(failure) => {
                    return Ok(module_failure_record("activate", &failure));
                }
            };
            let runtime = match RuntimeCheckResult::parse(&scenario_string(
                entry, "runtime",
            )?) {
                Ok(runtime) => runtime,
                Err(failure) => {
                    return Ok(module_failure_record("activate", &failure));
                }
            };
            match lifecycle.activate(
                request,
                supported_abi,
                authority,
                runtime,
            ) {
                Ok(active) => Ok(json!({
                    "op": "activate",
                    "ok": true,
                    "sessionId": active.session_id(),
                    "binding": {
                        "packageId": active.binding().package_id().as_str(),
                        "digest": active.binding().digest().as_str(),
                        "abi": active.binding().abi().as_str(),
                    },
                    "grant": active
                        .grant()
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>(),
                })),
                Err(failure) => {
                    Ok(module_failure_record("activate", &failure))
                }
            }
        }
        "workspaceScan" => {
            let files = scenario_string_array(entry, "files")?;
            let classified = files
                .iter()
                .map(|name| json!({ "name": name, "kind": classify_workspace_file(name) }))
                .collect::<Vec<_>>();
            let scan = workspace_module_scan(&files);
            Ok(json!({
                "op": "workspaceScan",
                "files": classified,
                "candidates": scan.candidates,
                "installs": scan.installs,
                "enables": scan.enables,
                "activations": scan.activations,
                "downloads": scan.downloads,
                "recommendations": scan.recommendations,
            }))
        }
        _ => Ok(json!({ "op": op, "ok": false, "code": "INVALID_INPUT" })),
    }
}

/// Canonical record for one domain-lifecycle scenario.
pub(in crate::harness) fn module_lifecycle_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let supported_abi =
        ModuleAbi::parse(&scenario_string(input, "supportedAbi")?).map_err(
            |failure| {
                HarnessError::corpus(format!(
                    "domain input has an invalid supported ABI: {}",
                    failure.code()
                ))
            },
        )?;
    let authority =
        HostAuthority::parse(&scenario_string_array(input, "authority")?)
            .map_err(|failure| {
                HarnessError::corpus(format!(
                    "domain input has an invalid authority: {}",
                    failure.code()
                ))
            })?;
    let mut lifecycle = ModuleLifecycle::new();
    let mut ops: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "ops")? {
        let op = scenario_string(entry, "op")?;
        let observation = run_module_lifecycle_op(
            &mut lifecycle,
            &op,
            entry,
            &supported_abi,
            &authority,
        )?;
        ops.push(observation);
    }
    Ok(json!({ "ops": ops }))
}

/// Canonical record for one domain-capability scenario.
pub(in crate::harness) fn module_capability_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let authority = match HostAuthority::parse(&scenario_string_array(
        input,
        "authority",
    )?) {
        Ok(authority) => authority,
        Err(failure) => {
            return Ok(json!({
                "authority": [],
                "ops": [{"op": "invalid", "ok": false, "code": failure.code()}],
            }));
        }
    };
    let mut ops: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "ops")? {
        let op = scenario_string(entry, "op")?;
        match op.as_str() {
            "decide" => {
                let request = match CapabilityRequest::parse(
                    &scenario_string_array(entry, "request")?,
                ) {
                    Ok(request) => request,
                    Err(failure) => {
                        ops.push(json!({
                            "op": "decide",
                            "ok": false,
                            "code": failure.code(),
                        }));
                        continue;
                    }
                };
                match decide_grant(&request, &authority) {
                    GrantDecision::Granted(grant) => ops.push(json!({
                        "op": "decide",
                        "granted": true,
                        "grant": grant
                            .iter()
                            .map(|id| id.as_str())
                            .collect::<Vec<_>>(),
                    })),
                    GrantDecision::Denied { missing } => ops.push(json!({
                        "op": "decide",
                        "granted": false,
                        "missing": missing
                            .iter()
                            .map(|id| id.as_str())
                            .collect::<Vec<_>>(),
                    })),
                }
            }
            "inspectAuthority" => ops.push(json!({
                "op": "inspectAuthority",
                "authority": authority
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            })),
            _ => ops.push(json!({
                "op": op,
                "ok": false,
                "code": "INVALID_INPUT",
            })),
        }
    }
    Ok(json!({ "ops": ops }))
}
