//! Composition-family records: run profile, composition/evolve subjects, QA workflow, and run interaction.
use crate::harness::*;

pub(in crate::harness) fn visual_evidence_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        RuntimeBudgetInput, RuntimeMode, VISUAL_CAPTURE_CAPABILITY,
        VISUAL_CAPTURE_UNAVAILABLE_REASON, VisualCaptureRequest, VisualFrame,
        create_runtime_budget, create_visual_capture_evidence,
        decide_visual_capture_with_flag,
        is_identity_bound_visual_capture_primitive_available,
        render_visual_capture_evidence,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown visual-evidence op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
    let run_id = request_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let operation_id = request_value
        .get("operationId")
        .and_then(Value::as_str)
        .map(|s| s.to_owned());
    let mode_value =
        request_value.get("mode").and_then(Value::as_str).unwrap_or("");
    let Some(mode) = RuntimeMode::parse(mode_value) else {
        return Err(HarnessError::corpus(format!(
            "unknown visual-evidence mode {mode_value:?}"
        )));
    };
    let is_stale =
        request_value.get("isStale").and_then(Value::as_bool).unwrap_or(false);
    let mut frames = Vec::new();
    if let Some(frame_values) =
        request_value.get("frames").and_then(Value::as_array)
    {
        for (index, frame_value) in frame_values.iter().enumerate() {
            let payload = frame_value.as_str().unwrap_or("");
            frames.push(VisualFrame {
                index,
                bytes: payload.as_bytes().to_vec(),
            });
        }
    }
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
    let request =
        VisualCaptureRequest { run_id, operation_id, mode, is_stale, frames };
    let available = is_identity_bound_visual_capture_primitive_available();
    match decide_visual_capture_with_flag(
        &request,
        &policy,
        &budget,
        is_cancelled,
    ) {
        Ok(outcome) => {
            let evidence = create_visual_capture_evidence(&outcome, &request)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "outcome": {
                    "disposition": evidence.outcome.disposition().as_str(),
                    "reason": evidence.outcome.reason(),
                    "isUnavailable": evidence.outcome.is_unavailable(),
                },
                "available": available,
                "reason": VISUAL_CAPTURE_UNAVAILABLE_REASON,
                "capability": VISUAL_CAPTURE_CAPABILITY,
                "detail": {
                    "mode": evidence.detail.mode.as_str(),
                    "frameCount": evidence.detail.frame_count,
                    "frameDigests": evidence.detail.frame_digests,
                    "totalBytes": evidence.detail.total_bytes,
                },
                "captureDigest": evidence.capture_digest,
                "rendered": render_visual_capture_evidence(&evidence),
            }))
        }
        Err(error) => Ok(json!({ "error": error.message })),
    }
}

pub(in crate::harness) fn run_profile_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        RUN_PROFILE_CAPABILITY, RUN_PROFILE_UNAVAILABLE_REASON,
        RunProfileRequest, RuntimeBudgetInput, create_run_profile_evidence,
        create_runtime_budget, decide_run_profile_with_flag,
        is_identity_bound_profiling_primitive_available,
        render_run_profile_evidence,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown run-profile op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
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
    let samples = request_value
        .get("samples")
        .and_then(Value::as_array)
        .map(|sample_values| {
            sample_values
                .iter()
                .enumerate()
                .map(|(index, sample_value)| {
                    siralos_core::runtime::ProfileSample {
                        index,
                        label: sample_value.as_str().unwrap_or("").to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let policy_map = input
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut policy_rules = Vec::new();
    for (cap, rule_str) in policy_map {
        let rule = match rule_str.as_str().unwrap_or("deny") {
            "allow" => PermissionRule::Allow,
            "ask" => PermissionRule::Ask,
            _ => PermissionRule::Deny,
        };
        if let Ok(cap_id) = CapabilityId::parse(&cap) {
            policy_rules.push(PolicyRule { capability: cap_id, rule });
        }
    }
    let policy = PermissionPolicy::from_rules(policy_rules);
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
    let request =
        RunProfileRequest { run_id, operation_id, is_stale, samples };
    let available = is_identity_bound_profiling_primitive_available();
    match decide_run_profile_with_flag(
        &request,
        &policy,
        &budget,
        is_cancelled,
    ) {
        Ok(outcome) => {
            let evidence = create_run_profile_evidence(&outcome, &request)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "outcome": {
                    "disposition": evidence.outcome.disposition().as_str(),
                    "reason": evidence.outcome.reason(),
                    "isUnavailable": evidence.outcome.is_unavailable(),
                },
                "available": available,
                "reason": RUN_PROFILE_UNAVAILABLE_REASON,
                "capability": RUN_PROFILE_CAPABILITY,
                "detail": {
                    "sampleCount": evidence.detail.sample_count,
                    "sampleDigests": evidence.detail.sample_digests,
                    "totalBytes": evidence.detail.total_bytes,
                },
                "profileDigest": evidence.profile_digest,
                "rendered": render_run_profile_evidence(&evidence),
            }))
        }
        Err(error) => Ok(json!({ "error": error.message })),
    }
}

pub(in crate::harness) fn composition_profile_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_adapters::profile_config::parse_profile_document;
    use siralos_core::composition::{
        ProfileResolution, create_profile_evidence,
        default_profile_resolution, render_profile_evidence,
        resolve_profile_overlay,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "resolve" {
        return Err(HarnessError::corpus(format!(
            "unknown composition-profile op {op:?}"
        )));
    }
    let mut policy_rules = Vec::new();
    if let Some(policy) = input.get("hostPolicy").and_then(Value::as_object) {
        for (capability, rule) in policy {
            let rule_text = rule.as_str().unwrap_or("");
            let capability_id = CapabilityId::parse(capability)
                .map_err(|error| HarnessError::corpus(error.to_string()))?;
            let parsed_rule =
                PermissionRule::parse(rule_text).ok_or_else(|| {
                    HarnessError::corpus(format!(
                        "unknown host rule {rule_text:?}"
                    ))
                })?;
            policy_rules.push(PolicyRule {
                capability: capability_id,
                rule: parsed_rule,
            });
        }
    }
    let host = PermissionPolicy::from_rules(policy_rules);
    let resolution = match input.get("document").and_then(Value::as_str) {
        None => default_profile_resolution(),
        Some(document) => match parse_profile_document(document) {
            Ok(record) => resolve_profile_overlay(&record, &host)
                .map_err(|error| HarnessError::corpus(error.message))?,
            Err(error) => {
                return Ok(json!({
                    "disposition": "invalid",
                    "reason": error.message,
                    "profile": Value::Null,
                    "narrowedOverlay": Value::Null,
                    "profileDigest": Value::Null,
                    "rendered": format!("invalid: {}", error.message),
                }));
            }
        },
    };
    let evidence = match create_profile_evidence(&resolution) {
        Ok(evidence) => evidence,
        Err(error) => {
            return Ok(json!({
                "disposition": "invalid",
                "reason": error.message,
                "profile": Value::Null,
                "narrowedOverlay": Value::Null,
                "profileDigest": Value::Null,
                "rendered": format!("invalid: {}", error.message),
            }));
        }
    };
    let (profile, narrowed_overlay) = match &resolution {
        ProfileResolution::Resolved { name, narrowed } => {
            let map: serde_json::Map<String, Value> = narrowed
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.capability.as_str().to_owned(),
                        Value::String(entry.requested.as_str().to_owned()),
                    )
                })
                .collect();
            (
                json!({
                    "name": name,
                    "overlayEntries": narrowed.entries.len(),
                }),
                Value::Object(map),
            )
        }
        _ => (Value::Null, Value::Null),
    };
    let reason = match &resolution {
        ProfileResolution::Refused { reason } => Some(reason.clone()),
        _ => None,
    };
    Ok(json!({
        "disposition": resolution.disposition(),
        "reason": reason,
        "profile": profile,
        "narrowedOverlay": narrowed_overlay,
        "profileDigest": evidence.profile_digest,
        "rendered": render_profile_evidence(&evidence),
    }))
}

pub(in crate::harness) fn composition_effective_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_adapters::profile_config::parse_profile_document;
    use siralos_core::composition::{
        DeclaredProfile, compose_effective_policy,
        create_effective_policy_evidence, declare_profile,
        render_effective_policy_evidence,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let Some(host_policy) = input.get("hostPolicy").and_then(Value::as_object)
    else {
        return Err(HarnessError::corpus(
            "composition-effective requires a hostPolicy object".to_owned(),
        ));
    };
    let mut host_rules = Vec::new();
    for (capability, rule) in host_policy {
        let rule_text = rule.as_str().unwrap_or("");
        let capability_id = CapabilityId::parse(capability)
            .map_err(|error| HarnessError::corpus(error.to_string()))?;
        let parsed_rule =
            PermissionRule::parse(rule_text).ok_or_else(|| {
                HarnessError::corpus(format!(
                    "unknown host rule {rule_text:?}"
                ))
            })?;
        host_rules
            .push(PolicyRule { capability: capability_id, rule: parsed_rule });
    }
    let host = PermissionPolicy::from_rules(host_rules.clone());
    let declared = match input.get("document").and_then(Value::as_str) {
        None => DeclaredProfile::Absent,
        Some(document) => match parse_profile_document(document) {
            Ok(record) => declare_profile(Some(&record), &host),
            Err(error) => {
                DeclaredProfile::Invalid { diagnostic: error.message }
            }
        },
    };
    let effective = compose_effective_policy(&host_rules, &declared);
    let evidence = create_effective_policy_evidence(&effective)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let rules: serde_json::Map<String, Value> = effective
        .rules
        .iter()
        .map(|rule| {
            (
                rule.capability.as_str().to_owned(),
                Value::String(rule.rule.as_str().to_owned()),
            )
        })
        .collect();
    Ok(json!({
        "applied": effective.applied_profile.is_some(),
        "diagnostic": evidence.policy.diagnostic,
        "effective": Value::Object(rules),
        "effectiveDigest": evidence.effective_digest,
        "rendered": render_effective_policy_evidence(&evidence),
    }))
}

pub(in crate::harness) fn context_controls_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::context::{
        ContextControlOutcome, ContextPolicy, create_context_control_evidence,
        evaluate_context_policy, render_context_control_evidence,
    };
    let Some(policy_value) = input.get("policy").and_then(Value::as_object)
    else {
        return Err(HarnessError::corpus(
            "context-controls requires a policy object".to_owned(),
        ));
    };
    let kind = policy_value.get("kind").and_then(Value::as_str).unwrap_or("");
    let digest = policy_value.get("digest").and_then(Value::as_str);
    let policy = ContextPolicy::new(kind, digest)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let Some(actual_digest) =
        input.get("actualDigest").and_then(Value::as_str)
    else {
        return Err(HarnessError::corpus(
            "context-controls requires an actualDigest string".to_owned(),
        ));
    };
    let outcome = evaluate_context_policy(&policy, actual_digest);
    let evidence = create_context_control_evidence(&policy, &outcome)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let (expected_digest, bound_digest) = match &outcome {
        ContextControlOutcome::Fresh { bound } => (None, bound.clone()),
        ContextControlOutcome::Stale { expected, .. } => {
            (Some(expected.clone()), None)
        }
        ContextControlOutcome::Blocked { expected, .. } => {
            (Some(expected.clone()), None)
        }
    };
    Ok(json!({
        "actualDigest": actual_digest,
        "boundDigest": bound_digest,
        "controlDigest": evidence.control_digest,
        "disposition": outcome.disposition(),
        "expectedDigest": expected_digest,
        "rendered": render_context_control_evidence(&evidence),
    }))
}

pub(in crate::harness) fn composition_lock_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::lock::{
        LockModuleIdentity, create_workspace_lock,
    };
    let profile_digest = input.get("profileDigest").and_then(Value::as_str);
    let mut modules = Vec::new();
    // Frozen identifier — pinned corpus key; do not rename (ADR 0037 §8).
    if let Some(list) = input.get("plugins").and_then(Value::as_array) {
        for entry in list {
            let Some(table) = entry.as_object() else {
                return Err(HarnessError::corpus(
                    "composition-lock requires module objects".to_owned(),
                ));
            };
            let get = |key: &str| {
                table.get(key).and_then(Value::as_str).unwrap_or("")
            };
            modules.push(LockModuleIdentity {
                id: get("id").to_owned(),
                path: get("path").to_owned(),
                digest: get("digest").to_owned(),
            });
        }
    }
    let lock = create_workspace_lock(profile_digest, &modules)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let stored = input.get("storedLockDigest").and_then(Value::as_str);
    let (disposition, rendered) = match stored {
        None => {
            ("resolved", format!("resolved modules={}", lock.modules.len()))
        }
        Some(digest) if digest == lock.lock_digest => {
            ("current", "verified current".to_owned())
        }
        Some(digest) => (
            "stale",
            format!(
                "verified stale expected={} actual={}",
                &lock.lock_digest[..8.min(lock.lock_digest.len())],
                &digest[..8.min(digest.len())]
            ),
        ),
    };
    let identities: Vec<Value> = lock
        .modules
        .iter()
        .map(|identity| {
            json!({
                "digest": identity.digest,
                "id": identity.id,
                "path": identity.path,
            })
        })
        .collect();
    Ok(json!({
        "disposition": disposition,
        "identities": identities,
        "lockDigest": lock.lock_digest,
        "rendered": rendered,
    }))
}

pub(in crate::harness) fn composition_plugin_selection_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::{
        create_module_selection_evidence, render_module_selection_evidence,
        select_profile_modules,
    };
    let strings = |key: &str| -> Option<Vec<String>> {
        input.get(key).and_then(Value::as_array).map(|list| {
            list.iter()
                .map(|value| value.as_str().unwrap_or("").to_owned())
                .collect()
        })
    };
    let enabled = strings("enabled").unwrap_or_default();
    let selected = strings("selected");
    let selection = select_profile_modules(&enabled, selected.as_deref());
    let evidence = create_module_selection_evidence(&selection)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let reason = if selection.unknown.is_empty() {
        None
    } else {
        Some(format!(
            "selection names {} un-enabled module id(s); they stay inactive",
            selection.unknown.len()
        ))
    };
    Ok(json!({
        "activated": evidence.activated,
        "disposition": evidence.disposition,
        "reason": reason,
        "rendered": render_module_selection_evidence(&evidence),
        "selectionDigest": evidence.selection_digest,
    }))
}

pub(in crate::harness) fn composition_context_control_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::{
        create_context_control_decision_evidence, decide_context_control,
        render_context_control_decision,
    };
    use siralos_core::context::ContextPolicy;
    let actual =
        input.get("actual").and_then(Value::as_str).unwrap_or("").to_owned();
    let kind = input.get("kind").and_then(Value::as_str);
    let digest = input.get("digest").and_then(Value::as_str);
    let policy = match kind {
        None => None,
        Some(kind) => Some(
            ContextPolicy::new(kind, digest)
                .map_err(|error| HarnessError::corpus(error.message))?,
        ),
    };
    let decision = decide_context_control(policy.as_ref(), &actual);
    let evidence =
        create_context_control_decision_evidence(&decision, kind.is_some())
            .map_err(|error| HarnessError::corpus(error.message))?;
    Ok(json!({
        "controlDigest": evidence.control_digest,
        "disposition": evidence.disposition,
        "reason": evidence.reason,
        "rendered": render_context_control_decision(&evidence),
    }))
}
/// Stage 5.9 (decision 55): verify the on-disk lock against the
/// recomputed current lock over the pure composition seam.
pub(in crate::harness) fn composition_lock_verify_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::lock::{
        LockModuleIdentity, create_workspace_lock,
    };
    use siralos_core::composition::{
        StoredLockDigest, create_lock_verification_evidence,
        decide_lock_verification, render_lock_verification_evidence,
    };
    let build_identities = |value: Option<&Value>| {
        let mut modules = Vec::new();
        if let Some(list) = value.and_then(Value::as_array) {
            for entry in list {
                let table = entry.as_object();
                let get = |key: &str| {
                    table
                        .and_then(|map| map.get(key))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                };
                modules.push(LockModuleIdentity {
                    id: get("id").to_owned(),
                    path: get("path").to_owned(),
                    digest: get("digest").to_owned(),
                });
            }
        }
        modules
    };
    let profile_digest = input.get("profileDigest").and_then(Value::as_str);
    // Frozen identifier — pinned corpus key; do not rename (ADR 0037 §8).
    let current_lock = create_workspace_lock(
        profile_digest,
        &build_identities(input.get("plugins")),
    )
    .map_err(|error| HarnessError::corpus(error.message))?;
    let stored = match input.get("stored") {
        None | Some(Value::Null) => StoredLockDigest::Missing,
        Some(table) => {
            let stored_profile =
                table.get("profileDigest").and_then(Value::as_str);
            match create_workspace_lock(
                stored_profile,
                &build_identities(table.get("plugins")),
            ) {
                Err(error) => StoredLockDigest::Untrusted(error.message),
                Ok(stored_lock) => {
                    match table.get("recordedDigest").and_then(Value::as_str) {
                        None => {
                            StoredLockDigest::Trusted(stored_lock.lock_digest)
                        }
                        Some(recorded)
                            if recorded == stored_lock.lock_digest =>
                        {
                            StoredLockDigest::Trusted(stored_lock.lock_digest)
                        }
                        Some(recorded) => {
                            StoredLockDigest::Untrusted(format!(
                                "the on-disk lock does not match its recorded digest (recorded {recorded}, re-derived {})",
                                stored_lock.lock_digest
                            ))
                        }
                    }
                }
            }
        }
    };
    let decision = decide_lock_verification(stored, &current_lock.lock_digest);
    let evidence = create_lock_verification_evidence(
        &decision,
        &current_lock.lock_digest,
    )
    .map_err(|error| HarnessError::corpus(error.message))?;
    Ok(json!({
        "decision": decision.outcome.as_str(),
        "lockDigest": current_lock.lock_digest,
        "reason": decision.reason,
        "rendered": render_lock_verification_evidence(&evidence),
    }))
}
/// Stage 5.10 (decision 56): consume the frozen 5.6 skill seam at the
/// session boundary over the pure composition decision.
pub(in crate::harness) fn composition_skill_consumption_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::{
        SkillCatalogState, compose_skill_consumption,
        create_skill_consumption_evidence, render_skill_consumption_evidence,
    };
    use siralos_core::skills::{SkillCatalog, SkillDefinition};
    let mut definitions = Vec::new();
    if let Some(list) = input.get("skills").and_then(Value::as_array) {
        for entry in list {
            let Some(table) = entry.as_object() else {
                return Err(HarnessError::corpus(
                    "composition-skill-consumption requires skill objects"
                        .to_owned(),
                ));
            };
            let name = table.get("name").and_then(Value::as_str).unwrap_or("");
            let content =
                table.get("content").and_then(Value::as_str).unwrap_or("");
            definitions.push(
                SkillDefinition::new(name, content)
                    .map_err(|error| HarnessError::corpus(error.message))?,
            );
        }
    }
    let catalog = if definitions.is_empty() {
        None
    } else {
        Some(
            SkillCatalog::new(definitions)
                .map_err(|error| HarnessError::corpus(error.message))?,
        )
    };
    let catalog_state = match &catalog {
        Some(catalog) => SkillCatalogState::Loaded(catalog),
        None => SkillCatalogState::Absent,
    };
    let selected: Option<Vec<String>> =
        input.get("selected").and_then(Value::as_array).map(|list| {
            list.iter()
                .map(|value| value.as_str().unwrap_or("").to_owned())
                .collect()
        });
    let decision =
        compose_skill_consumption(selected.as_deref(), catalog_state);
    let evidence = create_skill_consumption_evidence(&decision)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let bound: Vec<Value> = evidence
        .resolution
        .bound
        .iter()
        .map(|reference| {
            json!({
                "digest": reference.digest,
                "name": reference.name,
            })
        })
        .collect();
    Ok(json!({
        "bound": bound,
        "consumptionDigest": evidence.consumption_digest,
        "disposition": evidence.outcome,
        "reason": evidence.resolution.unknown,
        "rendered": render_skill_consumption_evidence(&evidence),
    }))
}
pub(in crate::harness) fn evolve_corpus_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::evolution::{
        EvaluationCase, EvaluationCorpus, evaluate_corpus,
        render_corpus_evidence,
    };
    use std::collections::BTreeMap;
    let corpus = match input.get("corpus") {
        None | Some(Value::Null) => None,
        Some(Value::Object(table)) => {
            let id = table
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let cases = table
                .get("cases")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|entry| {
                            let obj = entry.as_object()?;
                            Some(EvaluationCase {
                                id: obj
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                                prompt: obj
                                    .get("prompt")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                                expected: obj
                                    .get("expected")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(EvaluationCorpus { id, cases })
        }
        Some(_) => {
            return Err(HarnessError::corpus(
                "evolve-corpus requires a corpus object or null".to_owned(),
            ));
        }
    };
    let candidate: BTreeMap<String, String> = input
        .get("candidate")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let evaluation = evaluate_corpus(corpus, &candidate);
    match evaluation {
        siralos_core::evolution::CorpusEvaluation::Valid {
            evidence,
            score,
        } => Ok(json!({
            "corpusDigest": evidence.corpus_digest,
            "corpusId": evidence.corpus_id,
            "disposition": "valid",
            "matches": score.matches,
            "reason": null,
            "rendered": render_corpus_evidence(&evidence),
            "score": evidence.score,
            "scoreValue": evidence.score_value,
            "total": score.total,
        })),
        siralos_core::evolution::CorpusEvaluation::Invalid { reason } => {
            Ok(json!({
                "corpusDigest": null,
                "corpusId": null,
                "disposition": "invalid",
                "matches": null,
                "reason": reason,
                "rendered": format!("corpus invalid ({reason})"),
                "score": null,
                "scoreValue": null,
                "total": null,
            }))
        }
    }
}
pub(in crate::harness) fn evolve_workflow_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::evolution::{
        EvaluationCase, EvaluationCorpus, evaluate_workflow,
    };
    use std::collections::BTreeMap;

    let parse_corpus =
        |value: &Value| -> Result<Option<EvaluationCorpus>, HarnessError> {
            match value {
                Value::Null => Ok(None),
                Value::Object(table) => {
                    let id = table
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let cases = table
                        .get("cases")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(|entry| {
                                    let obj = entry.as_object()?;
                                    Some(EvaluationCase {
                                        id: obj
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_owned(),
                                        prompt: obj
                                            .get("prompt")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_owned(),
                                        expected: obj
                                            .get("expected")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_owned(),
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    Ok(Some(EvaluationCorpus { id, cases }))
                }
                _ => Err(HarnessError::corpus(
                    "evolve-workflow corpus must be an object or null"
                        .to_owned(),
                )),
            }
        };
    let parse_candidate = |value: &Value| -> BTreeMap<String, String> {
        value
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| {
                        Some((k.clone(), v.as_str()?.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let baseline_input =
        input.get("baseline").and_then(Value::as_object).ok_or_else(|| {
            HarnessError::corpus(
                "evolve-workflow requires baseline object".to_owned(),
            )
        })?;
    let candidate_input = input
        .get("candidate")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            HarnessError::corpus(
                "evolve-workflow requires candidate object".to_owned(),
            )
        })?;
    let escalation = input
        .get("escalation")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let baseline_corpus =
        parse_corpus(baseline_input.get("corpus").unwrap_or(&Value::Null))?;
    let baseline_candidate = parse_candidate(
        baseline_input.get("candidate").unwrap_or(&Value::Null),
    );
    let candidate_corpus =
        parse_corpus(candidate_input.get("corpus").unwrap_or(&Value::Null))?;
    let candidate_candidate = parse_candidate(
        candidate_input.get("candidate").unwrap_or(&Value::Null),
    );
    let evaluation = evaluate_workflow(
        baseline_corpus,
        &baseline_candidate,
        candidate_corpus,
        &candidate_candidate,
        &escalation,
    );
    match evaluation {
        siralos_core::evolution::WorkflowEvaluation::Valid {
            evidence,
            decision,
        } => Ok(serde_json::json!({
            "baselineDigest": evidence.baseline.corpus_digest,
            "baselineScore": evidence.baseline.score_value,
            "candidateDigest": evidence.candidate.corpus_digest,
            "candidateScore": evidence.candidate.score_value,
            "decision": decision.as_str(),
            "disposition": "valid",
            "escalation": evidence.escalation.as_str(),
            "improvement": evidence.improvement_value,
            "reason": null,
            "rendered": siralos_core::evolution::render_workflow_evidence(&evidence),
            "workflowDigest": evidence.workflow_digest,
        })),
        siralos_core::evolution::WorkflowEvaluation::Invalid { reason } => {
            Ok(serde_json::json!({
                "baselineDigest": null,
                "baselineScore": null,
                "candidateDigest": null,
                "candidateScore": null,
                "decision": null,
                "disposition": "invalid",
                "escalation": null,
                "improvement": null,
                "reason": reason,
                "rendered": format!("workflow invalid ({reason})"),
                "workflowDigest": null,
            }))
        }
    }
}

pub(in crate::harness) fn evolve_proposal_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::evolution::{Escalation, Proposal, evaluate_proposal};
    let proposal = match input.get("proposal") {
        None | Some(Value::Null) => None,
        Some(Value::Object(table)) => {
            let id = table
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let workflow_digest = table
                .get("workflowDigest")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let kind_raw = table
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let kind = match Escalation::parse(&kind_raw) {
                Ok(k) => k,
                Err(error) => {
                    return Ok(serde_json::json!({
                        "disposition": "invalid",
                        "proposalDigest": null,
                        "proposalId": null,
                        "reason": error.message,
                        "rendered": format!("proposal invalid ({})", error.message),
                        "requiresHostApproval": null,
                    }));
                }
            };
            let description = table
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let requires_host_approval = table
                .get("requiresHostApproval")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(Proposal {
                id,
                workflow_digest,
                kind,
                description,
                requires_host_approval,
            })
        }
        Some(_) => {
            return Err(HarnessError::corpus(
                "evolve-proposal requires a proposal object or null"
                    .to_owned(),
            ));
        }
    };
    let evaluation = evaluate_proposal(proposal);
    match evaluation {
        siralos_core::evolution::ProposalEvaluation::Valid { evidence } => {
            Ok(serde_json::json!({
                "disposition": "valid",
                "proposalDigest": evidence.proposal_digest,
                "proposalId": evidence.proposal_id,
                "reason": null,
                "rendered": siralos_core::evolution::render_proposal_evidence(&evidence),
                "requiresHostApproval": evidence.requires_host_approval,
            }))
        }
        siralos_core::evolution::ProposalEvaluation::Invalid { reason } => {
            Ok(serde_json::json!({
                "disposition": "invalid",
                "proposalDigest": null,
                "proposalId": null,
                "reason": reason,
                "rendered": format!("proposal invalid ({reason})"),
                "requiresHostApproval": null,
            }))
        }
    }
}

pub(in crate::harness) fn evolve_packaging_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::evolution::{Compatibility, Release, evaluate_release};
    let release = match input.get("release") {
        None | Some(Value::Null) => None,
        Some(Value::Object(table)) => {
            let id = table
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let version = table
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let previous_version = table
                .get("previousVersion")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let compat_raw = table
                .get("compatibility")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let compatibility = match Compatibility::parse(&compat_raw) {
                Ok(c) => c,
                Err(error) => {
                    return Ok(serde_json::json!({
                        "compatibility": null,
                        "disposition": "invalid",
                        "reason": error.message,
                        "releaseDigest": null,
                        "releaseId": null,
                        "rendered": format!("release invalid ({})", error.message),
                        "version": null,
                    }));
                }
            };
            Some(Release { id, version, previous_version, compatibility })
        }
        Some(_) => {
            return Err(HarnessError::corpus(
                "evolve-packaging requires a release object or null"
                    .to_owned(),
            ));
        }
    };
    let evaluation = evaluate_release(release);
    match evaluation {
        siralos_core::evolution::ReleaseEvaluation::Valid { evidence } => {
            Ok(serde_json::json!({
                "compatibility": evidence.compatibility.as_str(),
                "disposition": "valid",
                "reason": null,
                "releaseDigest": evidence.release_digest,
                "releaseId": evidence.release_id,
                "rendered": siralos_core::evolution::render_release_evidence(&evidence),
                "version": evidence.version,
            }))
        }
        siralos_core::evolution::ReleaseEvaluation::Invalid { reason } => {
            Ok(serde_json::json!({
                "compatibility": null,
                "disposition": "invalid",
                "reason": reason,
                "releaseDigest": null,
                "releaseId": null,
                "rendered": format!("release invalid ({reason})"),
                "version": null,
            }))
        }
    }
}

pub(in crate::harness) fn composition_plugin_activation_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::composition::{
        create_module_activation_evidence, decide_module_activation,
        render_module_activation_evidence,
    };
    let enabled: Vec<String> = input
        .get("enabled")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|value| value.as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default();
    let selected: Option<Vec<String>> =
        input.get("selected").and_then(Value::as_array).map(|list| {
            list.iter()
                .map(|value| value.as_str().unwrap_or("").to_owned())
                .collect()
        });
    let requested = input
        .get("requested")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let decision =
        decide_module_activation(&enabled, selected.as_deref(), &requested);
    let evidence =
        create_module_activation_evidence(&decision, selected.is_some())
            .map_err(|error| HarnessError::corpus(error.message))?;
    Ok(json!({
        "activationDigest": evidence.activation_digest,
        "decision": evidence.decision,
        "reason": evidence.reason,
        "rendered": render_module_activation_evidence(&evidence, &requested),
    }))
}
pub(in crate::harness) fn composition_skills_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::skills::{
        SkillCatalog, SkillDefinition, create_skill_resolution_evidence,
        render_skill_resolution_evidence, resolve_profile_skills,
    };
    let mut definitions = Vec::new();
    if let Some(list) = input.get("skills").and_then(Value::as_array) {
        for entry in list {
            let Some(table) = entry.as_object() else {
                return Err(HarnessError::corpus(
                    "composition-skills requires skill objects".to_owned(),
                ));
            };
            let name = table.get("name").and_then(Value::as_str).unwrap_or("");
            let content =
                table.get("content").and_then(Value::as_str).unwrap_or("");
            definitions.push(
                SkillDefinition::new(name, content)
                    .map_err(|error| HarnessError::corpus(error.message))?,
            );
        }
    }
    let catalog = SkillCatalog::new(definitions)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let selected: Option<Vec<String>> =
        input.get("selected").and_then(Value::as_array).map(|list| {
            list.iter()
                .map(|value| value.as_str().unwrap_or("").to_owned())
                .collect()
        });
    let resolution = resolve_profile_skills(&catalog, selected.as_deref());
    let evidence = create_skill_resolution_evidence(&resolution)
        .map_err(|error| HarnessError::corpus(error.message))?;
    let reason = if resolution.unknown.is_empty() {
        None
    } else {
        Some(format!(
            "selection names {} undeclared skill name(s); they stay unbound",
            resolution.unknown.len()
        ))
    };
    let bound: Vec<Value> = evidence
        .bound
        .iter()
        .map(|reference| {
            json!({"digest": reference.digest, "name": reference.name})
        })
        .collect();
    Ok(json!({
        "bound": bound,
        "disposition": evidence.disposition,
        "reason": reason,
        "rendered": render_skill_resolution_evidence(&evidence),
        "resolutionDigest": evidence.resolution_digest,
    }))
}
pub(in crate::harness) fn qa_workflow_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        QA_WORKFLOW_CAPABILITY, QA_WORKFLOW_UNAVAILABLE_REASON,
        QaWorkflowRequest, RuntimeBudgetInput, create_qa_workflow_evidence,
        create_runtime_budget, decide_qa_workflow_with_flag,
        is_identity_bound_qa_workflow_primitive_available,
        render_qa_workflow_evidence,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown qa-workflow op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
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
    let steps = request_value
        .get("steps")
        .and_then(Value::as_array)
        .map(|step_values| {
            step_values
                .iter()
                .enumerate()
                .map(|(index, step_value)| {
                    siralos_core::runtime::QaWorkflowStep {
                        index,
                        spec: step_value.as_str().unwrap_or("").to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let policy_map = input
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut policy_rules = Vec::new();
    for (cap, rule_str) in policy_map {
        let rule = match rule_str.as_str().unwrap_or("deny") {
            "allow" => PermissionRule::Allow,
            "ask" => PermissionRule::Ask,
            _ => PermissionRule::Deny,
        };
        if let Ok(cap_id) = CapabilityId::parse(&cap) {
            policy_rules.push(PolicyRule { capability: cap_id, rule });
        }
    }
    let policy = PermissionPolicy::from_rules(policy_rules);
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
    let request = QaWorkflowRequest { run_id, operation_id, is_stale, steps };
    let available = is_identity_bound_qa_workflow_primitive_available();
    match decide_qa_workflow_with_flag(
        &request,
        &policy,
        &budget,
        is_cancelled,
    ) {
        Ok(outcome) => {
            let evidence = create_qa_workflow_evidence(&outcome, &request)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "outcome": {
                    "disposition": evidence.outcome.disposition().as_str(),
                    "reason": evidence.outcome.reason(),
                    "isUnavailable": evidence.outcome.is_unavailable(),
                },
                "available": available,
                "reason": QA_WORKFLOW_UNAVAILABLE_REASON,
                "capability": QA_WORKFLOW_CAPABILITY,
                "detail": {
                    "stepCount": evidence.detail.step_count,
                    "stepDigests": evidence.detail.step_digests,
                    "totalBytes": evidence.detail.total_bytes,
                },
                "workflowDigest": evidence.workflow_digest,
                "rendered": render_qa_workflow_evidence(&evidence),
            }))
        }
        Err(error) => Ok(json!({ "error": error.message })),
    }
}

pub(in crate::harness) fn run_interaction_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::runtime::{
        RUN_INTERACTION_CAPABILITY, RUN_INTERACTION_UNAVAILABLE_REASON,
        RunInteractionRequest, RuntimeBudgetInput,
        create_run_interaction_evidence, create_runtime_budget,
        decide_run_interaction_with_flag,
        is_identity_bound_interactive_run_primitive_available,
        render_run_interaction_evidence,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let op = input.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "decide" {
        return Err(HarnessError::corpus(format!(
            "unknown run-interaction op {op:?}"
        )));
    }
    let request_value = input.get("request").cloned().unwrap_or(Value::Null);
    let run_id = request_value
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let operation_id = request_value
        .get("operationId")
        .and_then(Value::as_str)
        .map(|s| s.to_owned());
    let is_interactive = request_value
        .get("isInteractive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_stale =
        request_value.get("isStale").and_then(Value::as_bool).unwrap_or(false);
    let rounds = request_value
        .get("rounds")
        .and_then(Value::as_array)
        .map(|round_values| {
            round_values
                .iter()
                .enumerate()
                .map(|(index, round_value)| {
                    siralos_core::runtime::InteractionRound {
                        index,
                        request: round_value.as_str().unwrap_or("").to_owned(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let policy_map = input
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut policy_rules = Vec::new();
    for (cap, rule_str) in policy_map {
        let rule = match rule_str.as_str().unwrap_or("deny") {
            "allow" => PermissionRule::Allow,
            "ask" => PermissionRule::Ask,
            _ => PermissionRule::Deny,
        };
        if let Ok(cap_id) = CapabilityId::parse(&cap) {
            policy_rules.push(PolicyRule { capability: cap_id, rule });
        }
    }
    let policy = PermissionPolicy::from_rules(policy_rules);
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
    let request = RunInteractionRequest {
        run_id,
        operation_id,
        is_interactive,
        is_stale,
        rounds,
    };
    let available = is_identity_bound_interactive_run_primitive_available();
    match decide_run_interaction_with_flag(
        &request,
        &policy,
        &budget,
        is_cancelled,
    ) {
        Ok(outcome) => {
            let evidence = create_run_interaction_evidence(&outcome, &request)
                .map_err(|error| HarnessError::corpus(error.message))?;
            Ok(json!({
                "outcome": {
                    "disposition": evidence.outcome.disposition().as_str(),
                    "reason": evidence.outcome.reason(),
                    "isUnavailable": evidence.outcome.is_unavailable(),
                },
                "available": available,
                "reason": RUN_INTERACTION_UNAVAILABLE_REASON,
                "capability": RUN_INTERACTION_CAPABILITY,
                "detail": {
                    "roundCount": evidence.detail.round_count,
                    "roundDigests": evidence.detail.round_digests,
                    "totalBytes": evidence.detail.total_bytes,
                },
                "interactionDigest": evidence.interaction_digest,
                "rendered": render_run_interaction_evidence(&evidence),
            }))
        }
        Err(error) => Ok(json!({ "error": error.message })),
    }
}
