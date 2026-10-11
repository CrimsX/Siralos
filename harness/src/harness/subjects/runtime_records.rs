//! Runtime-family records: content identity, ICM, runtime readiness, runtime execution/evidence, and recovery taxonomy.
use crate::harness::*;

pub(in crate::harness) fn content_identity_artifact_digest_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let artifact_type =
        input.get("artifactType").and_then(Value::as_str).unwrap_or_default();
    let schema_version =
        input.get("schemaVersion").and_then(Value::as_u64).unwrap_or(1);
    let payload = input.get("payload").cloned().unwrap_or(Value::Null);
    let canonical = format!(
        "siralos:{artifact_type}:v{schema_version}\0{}",
        siralos_godot::godot::digest::canonicalize_json(&payload)
    );
    Ok(json!({ "digest": sha256_hex(canonical.as_bytes()) }))
}

pub(in crate::harness) fn content_identity_contract_digest_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::identity::CanonicalValue;
    use siralos_core::task::identity::{
        compute_contract_content_digest, compute_plan_content_digest_hex,
    };
    fn string_list(value: Option<&Value>) -> Vec<CanonicalValue> {
        value
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| {
                        CanonicalValue::Str(
                            entry.as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    let op = input.get("op").and_then(Value::as_str).unwrap_or_default();
    let payload = match op {
        "contract" => {
            let contract =
                input.get("contract").cloned().unwrap_or(Value::Null);
            let mut object = std::collections::BTreeMap::new();
            object.insert(
                "acceptanceCriteria".to_owned(),
                CanonicalValue::Array(
                    contract
                        .get("acceptanceCriteria")
                        .and_then(Value::as_array)
                        .map(|entries| {
                            entries
                                .iter()
                                .map(|criterion| {
                                    CanonicalValue::Object(
                                        std::collections::BTreeMap::from([
                                            (
                                                "description".to_owned(),
                                                CanonicalValue::Str(
                                                    criterion
                                                        .get("description")
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                            (
                                                "id".to_owned(),
                                                CanonicalValue::Str(
                                                    criterion
                                                        .get("id")
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                            (
                                                "verificationKind".to_owned(),
                                                CanonicalValue::Str(
                                                    criterion
                                                        .get(
                                                            "verificationKind",
                                                        )
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                        ]),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
            );
            object.insert(
                "constraints".to_owned(),
                CanonicalValue::Array(
                    contract
                        .get("constraints")
                        .and_then(Value::as_array)
                        .map(|entries| {
                            entries
                                .iter()
                                .map(|constraint| {
                                    CanonicalValue::Object(
                                        std::collections::BTreeMap::from([
                                            (
                                                "description".to_owned(),
                                                CanonicalValue::Str(
                                                    constraint
                                                        .get("description")
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                            (
                                                "id".to_owned(),
                                                CanonicalValue::Str(
                                                    constraint
                                                        .get("id")
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                            (
                                                "kind".to_owned(),
                                                CanonicalValue::Str(
                                                    constraint
                                                        .get("kind")
                                                        .and_then(
                                                            Value::as_str,
                                                        )
                                                        .unwrap_or_default()
                                                        .to_owned(),
                                                ),
                                            ),
                                        ]),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
            );
            if let Some(context) =
                contract.get("context").and_then(Value::as_str)
            {
                object.insert(
                    "context".to_owned(),
                    CanonicalValue::Str(context.to_owned()),
                );
            }
            object.insert(
                "id".to_owned(),
                CanonicalValue::Str(
                    contract
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
            );
            object.insert(
                "pausePolicy".to_owned(),
                CanonicalValue::Str(
                    contract
                        .get("pausePolicy")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
            );
            object.insert(
                "request".to_owned(),
                CanonicalValue::Str(
                    contract
                        .get("request")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
            );
            CanonicalValue::Object(object)
        }
        "plan" => {
            let plan = input.get("plan").cloned().unwrap_or(Value::Null);
            let section = |name: &str| {
                CanonicalValue::Array(string_list(plan.get(name)))
            };
            let mut object = std::collections::BTreeMap::new();
            object.insert(
                "objective".to_owned(),
                CanonicalValue::Str(
                    plan.get("objective")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
            );
            object.insert("scope".to_owned(), section("scope"));
            object.insert("nonGoals".to_owned(), section("nonGoals"));
            object.insert("touchpoints".to_owned(), section("touchpoints"));
            object.insert("constraints".to_owned(), section("constraints"));
            object.insert("risks".to_owned(), section("risks"));
            object.insert("steps".to_owned(), section("steps"));
            object.insert("validation".to_owned(), section("validation"));
            if let Some(rollback) =
                plan.get("rollback").and_then(Value::as_str)
            {
                object.insert(
                    "rollback".to_owned(),
                    CanonicalValue::Str(rollback.to_owned()),
                );
            }
            if let Some(rationale) =
                plan.get("rationale").and_then(Value::as_str)
            {
                object.insert(
                    "rationale".to_owned(),
                    CanonicalValue::Str(rationale.to_owned()),
                );
            }
            CanonicalValue::Object(object)
        }
        other => {
            return Err(HarnessError::corpus(format!(
                "unknown content-identity-contract-digest op {other}"
            )));
        }
    };
    let digest = match op {
        "contract" => {
            compute_contract_content_digest(&payload)
                .map_err(|error| HarnessError::corpus(error.message))?
                .value
        }
        _ => compute_plan_content_digest_hex(&payload)
            .map_err(|error| HarnessError::corpus(error.message))?,
    };
    Ok(json!({ "digest": digest }))
}

pub(in crate::harness) fn content_identity_manifests_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::identity::{
        GuidanceManifestEntry, create_guidance_manifest,
    };
    let entries: Vec<GuidanceManifestEntry> = input
        .get("entries")
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .map(|entry| GuidanceManifestEntry {
                    id: entry
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    kind: entry
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    path: entry
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    digest: entry
                        .get("digest")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let manifest = create_guidance_manifest(entries);
    Ok(json!({
        "aggregateDigest": manifest.aggregate_digest,
        "entryCount": manifest.entries.len(),
    }))
}

pub(in crate::harness) fn content_identity_delta_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let empty_map = Value::Object(serde_json::Map::new());
    let base = input.get("base").unwrap_or(&empty_map);
    let result = input.get("result").unwrap_or(&empty_map);
    let keys: Vec<String> = input
        .get("keys")
        .and_then(Value::as_array)
        .map(|array| {
            array.iter().filter_map(Value::as_str).map(str::to_owned).collect()
        })
        .unwrap_or_default();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for key in &keys {
        let base_value = base.get(key).cloned().unwrap_or(Value::Null);
        let result_value = result.get(key).cloned().unwrap_or(Value::Null);
        if siralos_godot::godot::digest::canonicalize_json(&base_value)
            == siralos_godot::godot::digest::canonicalize_json(&result_value)
        {
            unchanged.push(key.clone());
        } else {
            changed.push(key.clone());
        }
    }
    Ok(json!({
        "changed": changed,
        "unchanged": unchanged,
    }))
}

pub(in crate::harness) fn determinism_replay_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::determinism::{
        ClockPolicy, ProviderInputIdentity, RngPolicy, SourceRevision,
        create_reproducibility_manifest,
    };
    fn optional_string_field(value: Option<&Value>) -> Option<String> {
        // The oracle preserves empty strings (`?? null` only replaces
        // null/undefined), so an explicit "" must reach the manifest.
        value.and_then(Value::as_str).map(str::to_owned)
    }
    fn parse_source_revisions(value: &Value) -> Vec<SourceRevision> {
        value
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| SourceRevision {
                        path: entry
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        revision: entry
                            .get("revision")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    let provider_input = input.get("providerInput").and_then(|value| {
        if value.is_null() {
            return None;
        }
        Some(ProviderInputIdentity {
            provider_route: value
                .get("providerRoute")
                .and_then(Value::as_str)
                .map(str::to_owned),
            model_identity: value
                .get("modelIdentity")
                .and_then(Value::as_str)
                .map(str::to_owned),
            reasoning_mode: value
                .get("reasoningMode")
                .and_then(Value::as_str)
                .map(str::to_owned),
            temperature: value.get("temperature").and_then(Value::as_f64),
            top_p: value.get("topP").and_then(Value::as_f64),
            seed: value.get("seed").and_then(Value::as_u64),
            parameters: value
                .get("parameters")
                .and_then(Value::as_array)
                .map(|params| {
                    params
                        .iter()
                        .filter_map(|param| {
                            Some((
                                param.get("name")?.as_str()?.to_owned(),
                                param.get("value")?.as_str()?.to_owned(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    });
    let clock_policy =
        match input.pointer("/clockPolicy/mode").and_then(Value::as_str) {
            Some("fixed") => ClockPolicy::Fixed(
                input
                    .pointer("/clockPolicy/fixedMs")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ),
            _ => ClockPolicy::System,
        };
    let rng_policy =
        match input.pointer("/rngPolicy/mode").and_then(Value::as_str) {
            Some("seeded") => RngPolicy::Seeded(
                input
                    .pointer("/rngPolicy/seed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ),
            Some("system") => RngPolicy::System,
            _ => RngPolicy::None,
        };
    let manifest = create_reproducibility_manifest(
        siralos_core::determinism::ReproducibilityManifestInput {
            task_id: input
                .get("taskId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            execution_input_digest: optional_string_field(
                input.get("executionInputDigest"),
            ),
            environment_digest: optional_string_field(
                input.get("environmentDigest"),
            ),
            task_contract_digest: optional_string_field(
                input.get("taskContractDigest"),
            ),
            task_plan_digest: optional_string_field(
                input.get("taskPlanDigest"),
            ),
            guidance_digest: optional_string_field(
                input.get("guidanceDigest"),
            ),
            tool_surface_digest: optional_string_field(
                input.get("toolSurfaceDigest"),
            ),
            capability_digest: optional_string_field(
                input.get("capabilityDigest"),
            ),
            source_revision_set: parse_source_revisions(
                input.get("sourceRevisionSet").unwrap_or(&Value::Null),
            ),
            validation_profile: optional_string_field(
                input.get("validationProfile"),
            ),
            provider_input,
            clock_policy,
            rng_policy,
        },
    )
    .map_err(HarnessError::corpus)?;
    Ok(json!({ "digest": manifest.digest }))
}

// ---------------------------------------------------------------------------
// Stage 3R R10b subjects: ICM phase contracts + dependency manifests.
// ---------------------------------------------------------------------------

pub(in crate::harness) fn icm_phase_contract_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context::{
        CreatePhaseContractInput, PhaseAuthorityProfileInput,
        PhaseInputRequirement, PhaseOperation, PhaseOutputRequirement,
        PhaseVerificationRequirement, create_phase_contract, phase_contracts,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("create");
    if op == "registry" {
        let registry: Vec<Value> = phase_contracts()
            .iter()
            .map(|(id, contract)| {
                json!({ "id": id, "digest": contract.digest.value })
            })
            .collect();
        return Ok(json!({ "ok": true, "registry": registry }));
    }
    let declared = input.get("contract").cloned().unwrap_or(Value::Null);
    let string_list = |value: Option<&Value>| -> Vec<String> {
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
    };
    let authority = declared.get("authority").cloned().unwrap_or(Value::Null);
    let declaration = CreatePhaseContractInput {
        id: declared
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        version: declared.get("version").and_then(Value::as_u64).unwrap_or(0),
        phase: declared
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        inputs: declared
            .get("inputs")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| PhaseInputRequirement {
                        artifact_type: entry
                            .get("artifactType")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        optional: entry
                            .get("optional")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        reason: entry
                            .get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        authority: PhaseAuthorityProfileInput {
            read_only: authority
                .get("readOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            mutation: authority
                .get("mutation")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            approval_grant: authority
                .get("approvalGrant")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            acceptance_authority: authority
                .get("acceptanceAuthority")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            capability_narrowing: string_list(
                authority.get("capabilityNarrowing"),
            ),
        },
        process: declared
            .get("process")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| PhaseOperation {
                        id: entry
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        description: entry
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        outputs: declared
            .get("outputs")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| PhaseOutputRequirement {
                        artifact_type: entry
                            .get("artifactType")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        verification_kind: entry
                            .get("verificationKind")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        verification: declared
            .get("verification")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| PhaseVerificationRequirement {
                        id: entry
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        description: entry
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        evidence_class: entry
                            .get("evidenceClass")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        context_classes: string_list(declared.get("contextClasses")),
    };
    match create_phase_contract(&declaration) {
        Ok(contract) => Ok(json!({
            "ok": true,
            "id": contract.id,
            "version": contract.version,
            "digest": contract.digest.value,
        })),
        Err(error) => Ok(json!({ "ok": false, "error": error.message })),
    }
}

fn icm_parse_dependency(
    value: &Value,
) -> siralos_core::context::ArtifactDependency {
    use siralos_core::context::ArtifactDependency;
    ArtifactDependency {
        artifact_type: value
            .get("artifactType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        digest: value
            .get("digest")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

fn icm_manifest_value(
    manifest: &siralos_core::context::ArtifactDependencyManifest,
) -> Value {
    json!({
        "artifactType": manifest.artifact_type,
        "artifactId": manifest.artifact_id,
        "dependsOn": manifest.depends_on.iter().map(|entry| json!({
            "artifactType": entry.artifact_type,
            "digest": entry.digest,
        })).collect::<Vec<_>>(),
        "digest": manifest.digest,
    })
}

pub(in crate::harness) fn icm_dependency_manifests_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context::{
        ArtifactStalenessInput, build_dependency_manifest,
        compute_provenance_digest, compute_staleness_digest,
        create_artifact_dependency_manifest, create_context_provenance_ref,
        derive_artifact_staleness, is_prepared_mutation_stale,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or_default();
    match op {
        "staleness" => {
            let manifests: Vec<_> = input
                .get("manifests")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|manifest| {
                            let dependencies: Vec<_> = manifest
                                .get("dependsOn")
                                .and_then(Value::as_array)
                                .map(|dependencies| {
                                    dependencies
                                        .iter()
                                        .map(icm_parse_dependency)
                                        .collect()
                                })
                                .unwrap_or_default();
                            create_artifact_dependency_manifest(
                                manifest
                                    .get("artifactType")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                                manifest
                                    .get("artifactId")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                                &dependencies,
                            )
                            .map_err(|error| {
                                HarnessError::corpus(error.message)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let mut current = std::collections::BTreeMap::new();
            if let Some(map) =
                input.get("currentInputDigests").and_then(Value::as_object)
            {
                for (key, value) in map {
                    current.insert(
                        key.clone(),
                        value.as_str().unwrap_or_default().to_owned(),
                    );
                }
            }
            let result = derive_artifact_staleness(&ArtifactStalenessInput {
                manifests: &manifests,
                current_input_digests: &current,
            });
            let digest = compute_staleness_digest(&result)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "stale": result.stale,
                "current": result.current,
                "unrelatedChanges": result.unrelated_changes,
                "digest": digest,
            }))
        }
        "prepared-mutation-stale" => {
            let prepared: Vec<siralos_core::determinism::SourceRevision> =
                input
                    .get("preparedSourceRevisions")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|entry| {
                                siralos_core::determinism::SourceRevision {
                                    path: entry
                                        .get("path")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                    revision: entry
                                        .get("revision")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            let mut current = std::collections::BTreeMap::new();
            if let Some(map) =
                input.get("currentSourceRevisions").and_then(Value::as_object)
            {
                for (key, value) in map {
                    current.insert(
                        key.clone(),
                        value.as_str().unwrap_or_default().to_owned(),
                    );
                }
            }
            let result = is_prepared_mutation_stale(&prepared, &current);
            Ok(json!({
                "stale": result.stale,
                "stalePaths": result.stale_paths,
            }))
        }
        "manifest" => {
            if input.get("mode").and_then(Value::as_str) == Some("build") {
                let mut digests = std::collections::BTreeMap::new();
                if let Some(map) =
                    input.get("currentDigests").and_then(Value::as_object)
                {
                    for (key, value) in map {
                        digests.insert(
                            key.clone(),
                            value.as_str().map(str::to_owned),
                        );
                    }
                }
                let built = build_dependency_manifest(
                    input
                        .get("artifactType")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    input
                        .get("artifactId")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    &digests,
                )
                .map_err(|error| HarnessError::corpus(error.message))?;
                return Ok(json!({
                    "ok": true,
                    "manifest": built
                        .as_ref()
                        .map(icm_manifest_value),
                }));
            }
            let dependencies: Vec<siralos_core::context::ArtifactDependency> =
                input
                    .get("dependsOn")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries.iter().map(icm_parse_dependency).collect()
                    })
                    .unwrap_or_default();
            match create_artifact_dependency_manifest(
                input
                    .get("artifactType")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                input
                    .get("artifactId")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                &dependencies,
            ) {
                Ok(manifest) => Ok(
                    json!({ "ok": true, "manifest": icm_manifest_value(&manifest) }),
                ),
                Err(error) => {
                    Ok(json!({ "ok": false, "error": error.message }))
                }
            }
        }
        "provenance" => {
            let new_ref = input.get("newRef").cloned().unwrap_or(Value::Null);
            let created = match create_context_provenance_ref(
                new_ref
                    .get("item")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                new_ref
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                new_ref.get("id").and_then(Value::as_str).unwrap_or_default(),
                new_ref.get("digest").and_then(Value::as_str),
            ) {
                Ok(created) => created,
                Err(error) => {
                    return Ok(json!({ "ok": false, "error": error.message }));
                }
            };
            let mut refs: Vec<siralos_core::context::ContextProvenanceRef> =
                input
                    .get("refs")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|reference| {
                                let source = reference.get("source")?;
                                create_context_provenance_ref(
                                    reference
                                        .get("item")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default(),
                                    source
                                        .get("kind")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default(),
                                    source
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default(),
                                    source
                                        .get("digest")
                                        .and_then(Value::as_str),
                                )
                                .ok()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            refs.push(created.clone());
            let digest = compute_provenance_digest(&refs)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "created": {
                    "item": created.item,
                    "source": {
                        "kind": created.source.kind,
                        "id": created.source.id,
                        "digest": created.source.digest,
                    },
                },
                "digest": digest,
            }))
        }
        "why-validation-required" => {
            use siralos_core::context::render_why_validation_required;
            use siralos_core::determinism::ImpactRelationship;
            use siralos_core::determinism::decisions::ValidationRequirementClass;
            let plan_items: Vec<siralos_core::determinism::ValidationItem> =
                input
                    .get("planItems")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|entry| {
                                siralos_core::determinism::ValidationItem {
                                    id: entry
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                    class:
                                        ValidationRequirementClass::Required,
                                    rationale: entry
                                        .get("rationale")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_owned(),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            let string_list = |name: &str| -> Vec<String> {
                input
                    .get(name)
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let relations: Vec<ImpactRelationship> = input
                .get("impactRelations")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|relation| ImpactRelationship {
                            source: relation
                                .get("source")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            target: relation
                                .get("target")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let item_id = input
                .get("itemId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let diagnostic = siralos_core::context::why_validation_required(
                item_id,
                &plan_items,
                &string_list("changedSurfaces"),
                &relations,
                &string_list("acceptanceCriteria"),
            );
            Ok(json!({
                "found": diagnostic.is_some(),
                "itemId": item_id,
                "rendered": diagnostic
                    .as_ref()
                    .map(render_why_validation_required)
                    .unwrap_or_default(),
            }))
        }
        _ => Err(HarnessError::corpus(format!(
            "unknown icm.dependency-manifests op {op}"
        ))),
    }
}

/// Canonical runtime-readiness record (Stage 3R R10c subjects).
pub(in crate::harness) fn runtime_readiness_record(
    subject: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    match subject {
        SUBJECT_RR_IDENTITY => runtime_readiness_identity_record(input),
        SUBJECT_RR_BUDGETS => runtime_readiness_budgets_record(input),
        SUBJECT_RR_LIFECYCLE => runtime_readiness_lifecycle_record(input),
        SUBJECT_RR_DOCTOR => runtime_readiness_doctor_record(input),
        _ => unreachable!("runtime-readiness subject was routed"),
    }
}

fn rr_identity_run_id(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{RunIdentityInput, create_run_id};
    let outcome = create_run_id(&RunIdentityInput {
        task_id: input
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        phase_id: input
            .get("phaseId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        sequence: input.get("sequence").and_then(Value::as_u64).unwrap_or(0),
        kind: input.get("kind").and_then(Value::as_str),
    });
    Ok(match outcome {
        Ok(run_id) => json!({ "ok": true, "runId": run_id }),
        Err(error) => json!({ "ok": false, "error": error.message }),
    })
}

fn rr_identity_operation_id(input: &Value) -> Value {
    use siralos_core::runtime::create_operation_id;
    let operation_id = create_operation_id(
        input.get("runId").and_then(Value::as_str).unwrap_or_default(),
        input.get("operation").and_then(Value::as_str).unwrap_or_default(),
    );
    json!({ "operationId": operation_id })
}

fn rr_identity_trace_ref(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{create_run_trace_ref, format_run_trace_ref};
    let trace_value = input.get("trace").cloned().unwrap_or(Value::Null);
    let trace = create_run_trace_ref(
        trace_value.get("taskId").and_then(Value::as_str).unwrap_or_default(),
        trace_value.get("phaseId").and_then(Value::as_str).unwrap_or_default(),
        trace_value.get("runId").and_then(Value::as_str).unwrap_or_default(),
        trace_value.get("operationId").and_then(Value::as_str),
        trace_value
            .get("producer")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    Ok(json!({
        "ref": {
            "taskId": trace.task_id,
            "phaseId": trace.phase_id,
            "runId": trace.run_id,
            "operationId": trace.operation_id,
            "producer": trace.producer,
        },
        "formatted": format_run_trace_ref(&trace),
    }))
}

fn runtime_readiness_identity_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    match input.get("op").and_then(Value::as_str) {
        Some("run-id") => rr_identity_run_id(input),
        Some("operation-id") => Ok(rr_identity_operation_id(input)),
        Some("trace-ref") => rr_identity_trace_ref(input),
        other => Err(HarnessError::corpus(format!(
            "unknown runtime-readiness.identity op {other:?}"
        ))),
    }
}

fn rr_parse_budget(value: &Value) -> siralos_core::runtime::ArtifactBudget {
    use siralos_core::runtime::ArtifactBudget;
    let zero_or =
        |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    ArtifactBudget {
        max_artifact_bytes: zero_or("maxArtifactBytes"),
        max_artifacts_per_run: zero_or("maxArtifactsPerRun"),
        max_aggregate_bytes_per_run: zero_or("maxAggregateBytesPerRun"),
        max_retained_bytes_per_task: zero_or("maxRetainedBytesPerTask"),
    }
}

fn runtime_readiness_budgets_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        ArtifactAdmission, RuntimeBudgetInput, create_runtime_budget,
        enforce_artifact_budget, render_runtime_budget,
    };
    match input.get("op").and_then(Value::as_str) {
        Some("budget") => {
            let overrides =
                input.get("overrides").cloned().unwrap_or(Value::Null);
            let optional_u64 = |key: &str| -> Option<Option<u64>> {
                overrides.get(key).map(|value| value.as_u64())
            };
            let budget = create_runtime_budget(&RuntimeBudgetInput {
                startup_timeout_ms: optional_u64("startupTimeoutMs").flatten(),
                idle_timeout_ms: optional_u64("idleTimeoutMs").flatten(),
                hard_lifetime_ms: optional_u64("hardLifetimeMs").flatten(),
                stdout_bytes: optional_u64("stdoutBytes").flatten(),
                stderr_bytes: optional_u64("stderrBytes").flatten(),
                artifact_bytes: optional_u64("artifactBytes").flatten(),
                artifact_count: optional_u64("artifactCount").flatten(),
                child_process_count: optional_u64("childProcessCount")
                    .flatten(),
                memory_mb: optional_u64("memoryMb"),
                cpu_percent: overrides
                    .get("cpuPercent")
                    .map(|value| value.as_f64()),
            });
            Ok(json!({
                "digest": budget.digest,
                "rendered": render_runtime_budget(&budget),
            }))
        }
        Some("admission") => {
            let mut results = Vec::new();
            for case in input
                .get("cases")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let state = case.get("state").cloned().unwrap_or(Value::Null);
                // An absent budget means the oracle default budget, not
                // an unlimited one.
                let budget = match case.get("budget") {
                    Some(value) => rr_parse_budget(value),
                    None => {
                        siralos_core::runtime::DEFAULT_RUNTIME_ARTIFACT_BUDGET
                    }
                };
                let admission = enforce_artifact_budget(
                    &budget,
                    &siralos_core::runtime::ArtifactBudgetState {
                        artifact_count: state
                            .get("artifactCount")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                        aggregate_bytes: state
                            .get("aggregateBytes")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    },
                    case.get("incomingSize")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    case.get("incomingCount")
                        .and_then(Value::as_u64)
                        .unwrap_or(1),
                );
                results.push(match admission {
                    ArtifactAdmission::Admit { truncated } => json!({
                        "status": "admit",
                        "truncated": truncated,
                    }),
                    ArtifactAdmission::Limit { reason } => json!({
                        "status": "artifact_limit",
                        "reason": reason,
                    }),
                });
            }
            Ok(json!({ "cases": results }))
        }
        other => Err(HarnessError::corpus(format!(
            "unknown runtime-readiness.budgets op {other:?}"
        ))),
    }
}

fn rr_observation_to_json(
    observation: &siralos_core::runtime::SupervisorObservation,
) -> Value {
    use siralos_core::runtime::SupervisorObservation as Observation;
    match observation {
        Observation::StartupResult { ok, failure_kind } => json!({
            "type": "startup_result",
            "ok": ok,
            "failureKind": failure_kind.map(siralos_core::runtime::RuntimeFailureKind::as_str),
        }),
        Observation::OutputActivity => json!({ "type": "output_activity" }),
        Observation::Liveness { kind } => json!({
            "type": "liveness",
            "kind": kind.as_str(),
        }),
        Observation::IdleTimeout => json!({ "type": "idle_timeout" }),
        Observation::HardTimeout => json!({ "type": "hard_timeout" }),
        Observation::ResourceLimit { kind } => json!({
            "type": "resource_limit",
            "kind": kind.as_str(),
        }),
        Observation::CancelRequested => json!({ "type": "cancel_requested" }),
        Observation::ChildExit { exit_code } => json!({
            "type": "child_exit",
            "exitCode": exit_code,
        }),
        Observation::KillResult { ok } => json!({
            "type": "kill_result",
            "ok": ok,
        }),
        Observation::ChildRefusedTermination => {
            json!({ "type": "child_refused_termination" })
        }
    }
}

fn rr_json_to_observation(
    value: &Value,
) -> Result<siralos_core::runtime::SupervisorObservation, HarnessError> {
    use siralos_core::runtime::{
        LivenessKind, ResourceLimitKind, RuntimeFailureKind,
        SupervisorObservation as Observation,
    };
    let failure_kind = |raw: Option<&Value>| {
        raw.and_then(Value::as_str).and_then(RuntimeFailureKind::parse)
    };
    match value.get("type").and_then(Value::as_str) {
        Some("startup_result") => Ok(Observation::StartupResult {
            ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
            failure_kind: failure_kind(value.get("failureKind")),
        }),
        Some("output_activity") => Ok(Observation::OutputActivity),
        Some("liveness") => Ok(Observation::Liveness {
            kind: value
                .get("kind")
                .and_then(Value::as_str)
                .and_then(LivenessKind::parse)
                .ok_or_else(|| {
                    HarnessError::corpus("unknown liveness kind".to_owned())
                })?,
        }),
        Some("idle_timeout") => Ok(Observation::IdleTimeout),
        Some("hard_timeout") => Ok(Observation::HardTimeout),
        Some("resource_limit") => Ok(Observation::ResourceLimit {
            kind: value
                .get("kind")
                .and_then(Value::as_str)
                .and_then(ResourceLimitKind::parse)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "unknown resource limit kind".to_owned(),
                    )
                })?,
        }),
        Some("cancel_requested") => Ok(Observation::CancelRequested),
        Some("child_exit") => Ok(Observation::ChildExit {
            exit_code: value.get("exitCode").and_then(Value::as_i64),
        }),
        Some("kill_result") => Ok(Observation::KillResult {
            ok: value.get("ok").and_then(Value::as_bool).unwrap_or(false),
        }),
        Some("child_refused_termination") => {
            Ok(Observation::ChildRefusedTermination)
        }
        other => Err(HarnessError::corpus(format!(
            "unknown supervisor observation type {other:?}"
        ))),
    }
}

fn runtime_readiness_lifecycle_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        expected_failure_kind, initial_supervisor_state, observe_fault_script,
        transition_supervisor,
    };
    if input.get("op").and_then(Value::as_str) != Some("drive") {
        return Err(HarnessError::corpus(format!(
            "unknown runtime-readiness.lifecycle op {:?}",
            input.get("op")
        )));
    }
    let script = input
        .get("script")
        .and_then(Value::as_str)
        .and_then(siralos_core::runtime::FaultScript::parse)
        .ok_or_else(|| {
            HarnessError::corpus("unknown fault script".to_owned())
        })?;
    let mut view = initial_supervisor_state();
    let mut steps = Vec::new();
    for step in input
        .get("steps")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let at_ms = step.get("atMs").and_then(Value::as_u64).unwrap_or(0);
        let requested: Vec<String> = step
            .get("requested")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let borrowed: Vec<&str> =
            requested.iter().map(String::as_str).collect();
        let mut observations = observe_fault_script(script, at_ms, &borrowed);
        if let Some(inject) = step.get("inject") {
            observations.push(rr_json_to_observation(inject)?);
        }
        for observation in &observations {
            view = transition_supervisor(&view, observation, at_ms);
        }
        steps.push(json!({
            "atMs": at_ms,
            "observations": observations.iter().map(rr_observation_to_json).collect::<Vec<_>>(),
            "state": {
                "state": view.state.as_str(),
                "startedAtMs": view.started_at_ms,
                "terminatedAtMs": view.terminated_at_ms,
                "terminalDisposition": view.terminal_disposition.map(|status| status.as_str()),
                "failureKind": view.failure_kind.map(|kind| kind.as_str()),
            },
        }));
    }
    Ok(json!({
        "steps": steps,
        "expectedFailureKind":
            expected_failure_kind(script).map(|kind| kind.as_str()),
    }))
}

fn rr_capabilities(
    input: &Value,
) -> siralos_core::runtime::DoctorCapabilities {
    use siralos_core::runtime::DoctorCapabilities;
    let string_field =
        |key: &str| input.get(key).and_then(Value::as_str).map(str::to_owned);
    DoctorCapabilities {
        godot_executable_available: input
            .get("godotAvailable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        godot_executable_fingerprint: string_field("godotFingerprint"),
        project_identity: string_field("projectIdentity"),
        sandbox_available: input
            .get("sandboxAvailable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        process_supervision_supported: input
            .get("processSupervisionSupported")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        filesystem_isolation_available: input
            .get("filesystemIsolationAvailable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        user_data_redirect_available: input
            .get("userDataRedirectAvailable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        network_policy_resolvable: input
            .get("networkPolicyResolvable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        artifact_storage_available: input
            .get("artifactStorageAvailable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        display_available: match input.get("displayAvailable") {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        },
    }
}

fn runtime_readiness_doctor_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        RuntimeMode, build_runtime_readiness_diagnostic,
        evaluate_runtime_readiness, execution_allowed,
        render_runtime_readiness,
    };
    let capabilities =
        rr_capabilities(input.get("capabilities").unwrap_or(&Value::Null));
    match input.get("op").and_then(Value::as_str) {
        Some("readiness") => {
            let mode = input
                .get("mode")
                .and_then(Value::as_str)
                .and_then(RuntimeMode::parse)
                .ok_or_else(|| {
                    HarnessError::corpus("unknown runtime mode".to_owned())
                })?;
            let manifest = evaluate_runtime_readiness(&{
                let mut prepared = capabilities_to_readiness(&capabilities);
                prepared.runtime_mode = Some(mode);
                prepared
            })
            .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "ready": manifest.ready,
                "executionAllowed": execution_allowed(&manifest),
                "blockedReasons": manifest.blocked_reasons,
                "items": manifest.items.iter().map(|item| json!({
                    "id": item.id.as_str(),
                    "state": item.state.as_str(),
                    "detail": item.detail,
                })).collect::<Vec<_>>(),
                "digest": manifest.digest,
                "rendered": render_runtime_readiness(&manifest),
            }))
        }
        Some("diagnostic") => {
            let diagnostic = build_runtime_readiness_diagnostic(&capabilities)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "headless": {
                    "ready": diagnostic.headless.ready,
                    "digest": diagnostic.headless.digest,
                },
                "visual": {
                    "ready": diagnostic.visual.ready,
                    "digest": diagnostic.visual.digest,
                },
            }))
        }
        other => Err(HarnessError::corpus(format!(
            "unknown runtime-readiness.doctor op {other:?}"
        ))),
    }
}

pub(in crate::harness) fn runtime_execution_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        IDENTITY_BOUND_UNAVAILABLE_REASON, RuntimeBudgetInput,
        create_runtime_budget, decide_runtime_execution_with_flag,
        is_identity_bound_launch_primitive_available,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown runtime-execution op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
    let command = request_value
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let args: Vec<String> = request_value
        .get("args")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        })
        .unwrap_or_default();
    let run_id = request_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let operation_id = request_value
        .get("operationId")
        .and_then(Value::as_str)
        .map(|s| s.to_owned());
    let is_stale =
        request_value.get("isStale").and_then(Value::as_bool).unwrap_or(false);
    let requested_bytes = request_value
        .get("requestedBytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let policy_map = input
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rules = Vec::new();
    for (cap, rule_str) in policy_map {
        let rule = match rule_str.as_str().unwrap_or("deny") {
            "allow" => PermissionRule::Allow,
            "ask" => PermissionRule::Ask,
            _ => PermissionRule::Deny,
        };
        if let Ok(cap_id) = CapabilityId::parse(&cap) {
            rules.push(PolicyRule { capability: cap_id, rule });
        }
    }
    let policy = PermissionPolicy::from_rules(rules);
    let budget_value = input.get("budget").cloned().unwrap_or(Value::Null);
    let artifact_bytes = budget_value
        .get("artifactBytes")
        .and_then(Value::as_u64)
        .unwrap_or(64 * 1024 * 1024);
    let budget = create_runtime_budget(&RuntimeBudgetInput {
        artifact_bytes: Some(artifact_bytes),
        ..Default::default()
    });
    let is_cancelled =
        input.get("isCancelled").and_then(Value::as_bool).unwrap_or(false);
    let request = siralos_core::runtime::RuntimeExecutionRequest {
        command,
        args,
        run_id,
        operation_id,
        is_stale,
        requested_bytes,
    };
    let available = is_identity_bound_launch_primitive_available();
    match decide_runtime_execution_with_flag(
        &request,
        &policy,
        &budget,
        is_cancelled,
    ) {
        Ok(outcome) => Ok(json!({
            "outcome": {
                "disposition": outcome.disposition().as_str(),
                "reason": outcome.reason(),
                "isUnavailable": outcome.is_unavailable(),
            },
            "available": available,
            "reason": IDENTITY_BOUND_UNAVAILABLE_REASON,
        })),
        Err(error) => Ok(json!({
            "error": error.message,
            "available": available,
        })),
    }
}

pub(in crate::harness) fn runtime_evidence_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        RuntimeEvidenceInput, create_runtime_evidence, render_runtime_evidence,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "create" {
        return Err(HarnessError::corpus(format!(
            "unknown runtime-evidence op {op:?}"
        )));
    }
    let evidence_input_value =
        input.get("input").cloned().unwrap_or(Value::Null);
    let mut run_id = evidence_input_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut operation_id = evidence_input_value
        .get("operationId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let exit_code = evidence_input_value
        .get("exitCode")
        .and_then(Value::as_i64)
        .map(|v| v as i32);
    let duration_ms = evidence_input_value
        .get("durationMs")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut stdout = evidence_input_value
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut stderr = evidence_input_value
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if evidence_input_value
        .get("large")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        stdout = "a".repeat(1024 * 1024 + 10);
        stderr = "b".repeat(1024 * 1024 + 5);
    } else if evidence_input_value
        .get("largeWithEmoji")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let prefix = "a".repeat(1024 * 1024 - 2);
        stdout = format!("{prefix}😀");
        stderr = "b".repeat(1024 * 1024 + 5);
    }
    if evidence_input_value.get("runId").and_then(Value::as_str) == Some("") {
        run_id = String::new();
    }
    if evidence_input_value.get("operationId").and_then(Value::as_str)
        == Some("")
    {
        operation_id = String::new();
    }
    let evidence_input = RuntimeEvidenceInput {
        run_id,
        operation_id,
        exit_code,
        duration_ms,
        stdout,
        stderr,
    };
    match create_runtime_evidence(&evidence_input) {
        Ok(evidence) => Ok(json!({
            "evidence": {
                "runId": evidence.run_id,
                "operationId": evidence.operation_id,
                "exitCode": evidence.exit_code,
                "durationMs": evidence.duration_ms,
                "stdoutLength": evidence.stdout.len(),
                "stderrLength": evidence.stderr.len(),
                "truncated": evidence.truncated,
                "artifactDigest": evidence.artifact_digest,
                "digest": evidence.digest,
            },
            "rendered": render_runtime_evidence(&evidence),
        })),
        Err(error) => Ok(json!({ "error": error.message })),
    }
}

pub(in crate::harness) fn godot_runtime_launch_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        IDENTITY_BOUND_UNAVAILABLE_REASON, RuntimeBudgetInput,
        create_runtime_budget, is_identity_bound_launch_primitive_available,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    use siralos_godot::godot::runtime_adapter::{
        GodotLaunchMode, GodotLaunchRequest, decide_godot_launch,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown godot-runtime-launch op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
    let engine_id = request_value
        .get("engineId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let engine_version = request_value
        .get("engineVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let project_path = request_value
        .get("projectPath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mode_value =
        request_value.get("mode").and_then(Value::as_str).unwrap_or("");
    let Some(mode) = GodotLaunchMode::parse(mode_value) else {
        return Err(HarnessError::corpus(format!(
            "unknown godot-runtime-launch mode {mode_value:?}"
        )));
    };
    let run_id = request_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let operation_id = request_value
        .get("operationId")
        .and_then(Value::as_str)
        .map(|s| s.to_owned());
    let is_stale =
        request_value.get("isStale").and_then(Value::as_bool).unwrap_or(false);
    let requested_bytes = request_value
        .get("requestedBytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let policy_map = input
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut rules = Vec::new();
    for (cap, rule_str) in policy_map {
        let rule = match rule_str.as_str().unwrap_or("deny") {
            "allow" => PermissionRule::Allow,
            "ask" => PermissionRule::Ask,
            _ => PermissionRule::Deny,
        };
        if let Ok(cap_id) = CapabilityId::parse(&cap) {
            rules.push(PolicyRule { capability: cap_id, rule });
        }
    }
    let policy = PermissionPolicy::from_rules(rules);
    let artifact_bytes = input
        .get("budget")
        .and_then(|budget| budget.get("artifactBytes"))
        .and_then(Value::as_u64)
        .unwrap_or(64 * 1024 * 1024);
    let budget = create_runtime_budget(&RuntimeBudgetInput {
        artifact_bytes: Some(artifact_bytes),
        ..Default::default()
    });
    let is_cancelled =
        input.get("isCancelled").and_then(Value::as_bool).unwrap_or(false);
    let request = GodotLaunchRequest {
        engine_id,
        engine_version,
        project_path,
        mode,
        run_id,
        operation_id,
        is_stale,
        requested_bytes,
    };
    let available = is_identity_bound_launch_primitive_available();
    match decide_godot_launch(&request, &policy, &budget, is_cancelled) {
        Ok(decision) => Ok(json!({
            "outcome": {
                "disposition": decision.outcome.disposition().as_str(),
                "reason": decision.outcome.reason(),
                "isUnavailable": decision.outcome.is_unavailable(),
            },
            "available": available,
            "reason": IDENTITY_BOUND_UNAVAILABLE_REASON,
            "engine": {
                "engineId": decision.engine.engine_id,
                "engineVersion": decision.engine.engine_version,
                "projectPath": decision.engine.project_path,
                "mode": decision.engine.mode.as_str(),
            },
        })),
        Err(error) => Ok(json!({
            "error": error.message,
            "available": available,
        })),
    }
}

pub(in crate::harness) fn godot_runtime_evidence_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::RuntimeEvidenceInput;
    use siralos_godot::godot::runtime_adapter::{
        GodotLaunchMode, GodotRuntimeEvidenceDetail,
        create_godot_runtime_evidence, render_godot_runtime_evidence,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "create" {
        return Err(HarnessError::corpus(format!(
            "unknown godot-runtime-evidence op {op:?}"
        )));
    }
    let evidence_value = input.get("input").cloned().unwrap_or(Value::Null);
    let run_id = evidence_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let operation_id = evidence_value
        .get("operationId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let exit_code = evidence_value
        .get("exitCode")
        .and_then(Value::as_i64)
        .map(|value| value as i32);
    let duration_ms =
        evidence_value.get("durationMs").and_then(Value::as_u64).unwrap_or(0);
    let mut stdout = evidence_value
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut stderr = evidence_value
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if evidence_value.get("large").and_then(Value::as_bool).unwrap_or(false) {
        stdout = "a".repeat(1024 * 1024 + 10);
        stderr = "b".repeat(1024 * 1024 + 5);
    } else if evidence_value
        .get("largeWithEmoji")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let prefix = "a".repeat(1024 * 1024 - 2);
        stdout = format!("{prefix}\u{1F600}");
        stderr = "b".repeat(1024 * 1024 + 5);
    }
    let detail_value = input.get("detail").cloned().unwrap_or(Value::Null);
    let engine_id = detail_value
        .get("engineId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let engine_version = detail_value
        .get("engineVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let project_path = detail_value
        .get("projectPath")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mode_value =
        detail_value.get("mode").and_then(Value::as_str).unwrap_or("");
    let Some(mode) = GodotLaunchMode::parse(mode_value) else {
        return Err(HarnessError::corpus(format!(
            "unknown godot-runtime-evidence mode {mode_value:?}"
        )));
    };
    let evidence_input = RuntimeEvidenceInput {
        run_id,
        operation_id,
        exit_code,
        duration_ms,
        stdout,
        stderr,
    };
    let detail = GodotRuntimeEvidenceDetail {
        engine_id,
        engine_version,
        project_path,
        mode,
    };
    match create_godot_runtime_evidence(&evidence_input, &detail) {
        Ok(created) => Ok(json!({
            "evidence": {
                "runId": created.evidence.run_id,
                "operationId": created.evidence.operation_id,
                "exitCode": created.evidence.exit_code,
                "durationMs": created.evidence.duration_ms,
                "stdoutLength": created.evidence.stdout.len(),
                "stderrLength": created.evidence.stderr.len(),
                "truncated": created.evidence.truncated,
                "artifactDigest": created.evidence.artifact_digest,
                "digest": created.evidence.digest,
            },
            "detail": {
                "engineId": created.detail.engine_id,
                "engineVersion": created.detail.engine_version,
                "projectPath": created.detail.project_path,
                "mode": created.detail.mode.as_str(),
            },
            "godotDigest": created.godot_digest,
            "rendered": render_godot_runtime_evidence(&created),
        })),
        Err(error) => Ok(json!({ "error": error.message })),
    }
}

fn capabilities_to_readiness(
    capabilities: &siralos_core::runtime::DoctorCapabilities,
) -> siralos_core::runtime::RuntimeReadinessInput {
    use siralos_core::runtime::RuntimeReadinessInput;
    RuntimeReadinessInput {
        runtime_mode: None,
        godot_executable_available: capabilities.godot_executable_available,
        godot_executable_fingerprint: capabilities
            .godot_executable_fingerprint
            .clone(),
        project_identity: capabilities.project_identity.clone(),
        sandbox_backend_available: capabilities.sandbox_available,
        sandbox_supports_process_supervision: capabilities
            .process_supervision_supported,
        filesystem_isolation_available: capabilities
            .filesystem_isolation_available,
        user_data_redirect_available: capabilities
            .user_data_redirect_available,
        network_policy_resolvable: capabilities.network_policy_resolvable,
        artifact_storage_available: capabilities.artifact_storage_available,
        display_available: capabilities.display_available,
        memory_limit_enforced: false,
        cpu_limit_enforced: false,
    }
}

// ---------------------------------------------------------------------------
// Stage 3R R11.2 subject: recovery taxonomy.
// ---------------------------------------------------------------------------

pub(in crate::harness) fn recovery_taxonomy_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::determinism::{RetryCategory, classify_retry};
    use siralos_core::module::capability::CapabilityId;
    use siralos_core::module::failure::{
        ModuleFailure, ResourceExceededKind, failure_code,
    };
    use siralos_core::runtime::budget::IncompleteRunRecord;
    use siralos_core::runtime::classify_incomplete_run;
    let op = input.get("op").and_then(Value::as_str).unwrap_or_default();
    match op {
        "retry-classification" => {
            let mut cases = Vec::new();
            for entry in scenario_array(input, "cases")? {
                let category = scenario_string(entry, "category")?;
                let parsed =
                    RetryCategory::parse(&category).ok_or_else(|| {
                        HarnessError::corpus(format!(
                            "unknown retry category {category}"
                        ))
                    })?;
                let attempts_used = entry
                    .get("attemptsUsed")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let attempts_used =
                    u32::try_from(attempts_used).map_err(|_| {
                        HarnessError::corpus(
                            "retry attempts out of range".to_owned(),
                        )
                    })?;
                let result = classify_retry(
                    parsed,
                    attempts_used,
                    siralos_core::determinism::DEFAULT_RETRY_POLICY,
                );
                cases.push(json!({
                    "category": category,
                    "attemptsUsed": attempts_used,
                    "decision": result.decision.as_str(),
                    "reason": result.reason,
                    "nextBackoffMs": result.next_backoff_ms,
                }));
            }
            Ok(json!({ "cases": cases }))
        }
        "domain-failure" => {
            let kind =
                input.get("kind").and_then(Value::as_str).unwrap_or_default();
            let failure = match kind {
                "CAPABILITY_DENIED" => {
                    let mut missing = Vec::new();
                    for value in input
                        .get("missing")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or_default()
                    {
                        let raw = value.as_str().ok_or_else(|| {
                            HarnessError::corpus(
                                "capability ids must be strings".to_owned(),
                            )
                        })?;
                        missing.push(CapabilityId::parse(raw).map_err(
                            |failure| {
                                HarnessError::corpus(failure.code().to_owned())
                            },
                        )?);
                    }
                    ModuleFailure::CapabilityDenied { missing }
                }
                "STALE_ACTIVATION" => ModuleFailure::StaleActivation,
                "RESOURCE_EXCEEDED" => {
                    let resource_kind = input
                        .get("resourceKind")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let kind = match resource_kind {
                        "FUEL" => ResourceExceededKind::Fuel,
                        "MEMORY" => ResourceExceededKind::Memory,
                        "INPUT_BYTES" => ResourceExceededKind::InputBytes,
                        "OUTPUT_BYTES" => ResourceExceededKind::OutputBytes,
                        "HOST_CALLS" => ResourceExceededKind::HostCalls,
                        other => {
                            return Err(HarnessError::corpus(format!(
                                "unknown resource kind {other}"
                            )));
                        }
                    };
                    ModuleFailure::ResourceExceeded { kind }
                }
                "UNAVAILABLE" => ModuleFailure::Unavailable {
                    reason: input
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                },
                "CANCELLED" => ModuleFailure::Cancelled,
                other => {
                    return Err(HarnessError::corpus(format!(
                        "unknown domain-failure kind {other}"
                    )));
                }
            };
            let mut record = json!({ "code": failure_code(&failure) });
            if let ModuleFailure::CapabilityDenied { missing } = &failure {
                record["missing"] = json!(
                    missing.iter().map(|id| id.as_str()).collect::<Vec<_>>()
                );
            }
            if let ModuleFailure::ResourceExceeded { kind } = &failure {
                record["resourceKind"] = json!(kind.code());
            }
            if let ModuleFailure::Unavailable { reason } = &failure {
                record["reason"] = json!(reason);
            }
            Ok(record)
        }
        "incomplete-run" => {
            let mut cases = Vec::new();
            for entry in scenario_array(input, "cases")? {
                let record = IncompleteRunRecord {
                    run_id: entry
                        .get("runId")
                        .and_then(Value::as_str)
                        .unwrap_or("run_runtime_probe")
                        .to_owned(),
                    last_known_state: entry
                        .get("lastKnownState")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    last_observed_at_ms: entry
                        .get("lastObservedAtMs")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                };
                let may_exist = entry
                    .get("runStateMayExist")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let result = classify_incomplete_run(&record, may_exist);
                cases.push(json!({
                    "lastKnownState": record.last_known_state,
                    "runStateMayExist": may_exist,
                    "classification": result.classification.as_str(),
                    "reason": result.reason,
                }));
            }
            Ok(json!({ "cases": cases }))
        }
        other => Err(HarnessError::corpus(format!(
            "unknown recovery-taxonomy op {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// All-purpose provider subject: provider-generic (Stage 8, decision 67/68).
