//! Godot domain subject input validation (the `validate_godot_input` shape gate).
use crate::harness::*;

const GODOT_PLATFORMS: [&str; 3] = ["win32", "linux", "darwin"];

/// Schema-level validation for one Godot scenario input. Mirrors the
/// JS contract: platforms `["*"]`, empty env, a plain object bounded by
/// `MAX_GODOT_INPUT_BYTES`, plus per-subject key and type checks.
pub(in crate::harness) fn validate_godot_input(
    subject: &str,
    input: &Value,
) -> Result<(), HarnessError> {
    let reject = |message: String| {
        Err(HarnessError::corpus(format!(
            "scenario input rejected for subject {subject}: {message}"
        )))
    };
    let string_at = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("field {key} must be a string"))
    };
    if let Some(platform) = input.get("platform") {
        let Some(text) = platform.as_str() else {
            return reject("platform must be a string".to_owned());
        };
        if !GODOT_PLATFORMS.contains(&text) {
            return reject(format!("platform {text} is not a Godot platform"));
        }
    }
    match subject {
        SUBJECT_GODOT_SCENE_RESOLVE => {
            // The oracle ignores unknown keys; only the declared keys'
            // types are constrained here.
            for key in ["tscn", "tres"] {
                let value = input.get(key);
                if value.is_some_and(|value| {
                    !value.is_string() && !value.is_null()
                }) {
                    return reject(format!(
                        "field {key} must be a string or null"
                    ));
                }
            }
            if let Ok(path) = string_at("path") {
                if path.len() > 1024 {
                    return reject("field path exceeds its bound".to_owned());
                }
            } else if input.get("path").is_some() {
                return reject("field path must be a string".to_owned());
            }
            Ok(())
        }
        SUBJECT_GODOT_KNOWLEDGE => {
            match input.get("op").and_then(Value::as_str) {
                Some("status") | Some("refresh") => {
                    if let Some(cancelled) = input.get("cancelled") {
                        if !cancelled.is_boolean() {
                            return reject(
                                "cancelled must be a boolean".to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("search") => {
                    if let Err(message) = string_at("query") {
                        return reject(message);
                    }
                    if let Some(kinds) = input.get("kinds") {
                        let Some(entries) = kinds.as_array() else {
                            return reject(
                                "kinds must be an array".to_owned(),
                            );
                        };
                        const KINDS: [&str; 8] = [
                            "class", "method", "property", "signal",
                            "constant", "enum", "utility", "operator",
                        ];
                        if entries.len() > 16
                            || entries.iter().any(|entry| {
                                !entry
                                    .as_str()
                                    .is_some_and(|text| KINDS.contains(&text))
                            })
                        {
                            return reject(
                                "kinds must be at most 16 known kind strings"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(limit) = input.get("limit") {
                        if limit.as_u64().is_none_or(|limit| limit > 1000) {
                            return reject(
                                "limit must be an integer in 1..=1000"
                                    .to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("lookup") => {
                    if let Err(message) = string_at("symbol") {
                        return reject(message);
                    }
                    Ok(())
                }
                _ => reject(
                    "op must be status, refresh, search, or lookup".to_owned(),
                ),
            }
        }
        SUBJECT_GODOT_DISCOVERY => {
            match input.get("op").and_then(Value::as_str) {
                Some("discover" | "select") => {}
                _ => {
                    return reject("op must be discover or select".to_owned());
                }
            }
            if let Some(override_source) = input.get("overrideSource") {
                if !override_source.is_null()
                    && override_source.as_str() != Some("cli")
                {
                    return reject(
                        "overrideSource must be null or \"cli\"".to_owned(),
                    );
                }
            }
            for key in ["hostPath", "hostPathExt", "workspaceRoot"] {
                if let Some(value) = input.get(key) {
                    if !value.is_null() && value.as_str().is_none() {
                        return reject(format!(
                            "field {key} must be a string or null"
                        ));
                    }
                }
            }
            let Some(config) = input.get("config") else {
                return Ok(());
            };
            let Some(config) = config.as_object() else {
                return reject("config must be an object".to_owned());
            };
            if let Some(active) = config.get("activeInstallation") {
                if !active.is_null() && active.as_str().is_none() {
                    return reject(
                        "config.activeInstallation must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if let Some(discover_on_path) = config.get("discoverOnPath") {
                if !discover_on_path.is_boolean() {
                    return reject(
                        "config.discoverOnPath must be a boolean".to_owned(),
                    );
                }
            }
            let installations = config.get("installations");
            if installations.is_some_and(|value| !value.is_array()) {
                return reject(
                    "config.installations must be an array".to_owned(),
                );
            }
            for entry in
                installations.and_then(Value::as_array).into_iter().flatten()
            {
                let Some(entry) = entry.as_object() else {
                    return reject(
                        "config.installations entries must be objects"
                            .to_owned(),
                    );
                };
                if let Err(message) = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty() && id.len() <= 128)
                    .ok_or("entry id must be a non-empty bounded string")
                {
                    return reject(message.to_owned());
                }
                if let Err(message) = entry
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty() && path.len() <= 1024)
                    .ok_or("entry path must be a non-empty bounded string")
                {
                    return reject(message.to_owned());
                }
                match entry.get("editionHint").and_then(Value::as_str) {
                    None | Some("standard") | Some("dotnet")
                    | Some("unknown") => {}
                    Some(hint) => {
                        return reject(format!(
                            "entry editionHint {hint} is not standard, dotnet, or unknown"
                        ));
                    }
                }
            }
            Ok(())
        }
        SUBJECT_GODOT_DIAGNOSTICS => {
            match input.get("op").and_then(Value::as_str) {
                Some("support" | "status") => Ok(()),
                Some("prepare") => {
                    if let Some(paths) = input.get("paths") {
                        if paths.is_null() {
                            return Ok(());
                        }
                        let Some(entries) = paths.as_array() else {
                            return reject(
                                "paths must be an array or null".to_owned(),
                            );
                        };
                        if entries.len() > 64
                            || entries.iter().any(|entry| !entry.is_string())
                        {
                            return reject(
                                "paths must be at most 64 strings".to_owned(),
                            );
                        }
                    }
                    if let Some(cancelled) = input.get("cancelled") {
                        if !cancelled.is_boolean() {
                            return reject(
                                "cancelled must be a boolean".to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("execute") => {
                    if let Some(digest) = input.get("approvedDigest") {
                        let Some(text) = digest.as_str() else {
                            return reject(
                                "approvedDigest must be a string".to_owned(),
                            );
                        };
                        if text.len() != 64
                            || !text
                                .bytes()
                                .all(|byte| byte.is_ascii_hexdigit())
                        {
                            return reject(
                                "approvedDigest must be a hex SHA-256 digest"
                                    .to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                _ => reject(
                    "op must be support, prepare, execute, or status"
                        .to_owned(),
                ),
            }
        }
        SUBJECT_GODOT_LSP => match input.get("op").and_then(Value::as_str) {
            Some("support" | "status") => Ok(()),
            Some("prepare") => {
                if let Some(cancelled) = input.get("cancelled") {
                    if !cancelled.is_boolean() {
                        return reject(
                            "cancelled must be a boolean".to_owned(),
                        );
                    }
                }
                Ok(())
            }
            _ => reject("op must be support, prepare, or status".to_owned()),
        },
        SUBJECT_GODOT_REVIEW_CONTEXT => {
            const KEYS: [&str; 8] = [
                "taskContractRevision",
                "changedPaths",
                "edges",
                "signalConnections",
                "autoloads",
                "candidateTests",
                "mainScene",
                "revisions",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("taskContractRevision").and_then(Value::as_u64) {
                Some(revision) if revision >= 1 => {}
                _ => {
                    return reject(
                        "taskContractRevision must be a positive integer"
                            .to_owned(),
                    );
                }
            }
            if let Some(changed) = input.get("changedPaths") {
                let ok = changed.as_array().is_some_and(|entries| {
                    entries.iter().all(Value::is_string)
                });
                if !ok {
                    return reject(
                        "changedPaths must be an array of strings".to_owned(),
                    );
                }
            }
            let edge_kinds = [
                "script_attachment",
                "scene_inheritance",
                "scene_instancing",
                "resource_dependency",
                "script_dependency",
                "signal_connection",
                "autoload_global",
                "test_covers",
            ];
            if let Some(edges) = input.get("edges") {
                let Some(entries) = edges.as_array() else {
                    return reject("edges must be an array".to_owned());
                };
                for entry in entries {
                    let kind_ok = entry
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| edge_kinds.contains(&kind));
                    let strings_ok =
                        ["fromPath", "toPath"].iter().all(|field| {
                            entry.get(*field).is_some_and(Value::is_string)
                        });
                    if !kind_ok || !strings_ok {
                        return reject(
                            "edges entries need a known kind, fromPath, and toPath"
                                .to_owned(),
                        );
                    }
                }
            }
            for key in [
                "signalConnections",
                "candidateTests",
                "revisions",
                "autoloads",
            ] {
                if let Some(value) = input.get(key) {
                    if !value.is_array() {
                        return reject(format!("{key} must be an array"));
                    }
                }
            }
            if let Some(main_scene) = input.get("mainScene") {
                if !main_scene.is_null() && main_scene.as_str().is_none() {
                    return reject(
                        "mainScene must be a string or null".to_owned(),
                    );
                }
            }
            Ok(())
        }
        SUBJECT_GODOT_MUTATION_PREPARE => {
            const OPS: [&str; 11] = [
                "set_property",
                "remove_property",
                "add_node",
                "remove_node",
                "set_script_attachment",
                "change_resource_reference",
                "add_signal_connection",
                "remove_signal_connection",
                "create_subresource",
                "update_subresource",
                "remove_subresource",
            ];
            const KEYS: [&str; 10] = [
                "operations",
                "targetPath",
                "sourceRevision",
                "sourceSha256",
                "serializedAfter",
                "previewSummary",
                "previewDiff",
                "kind",
                "addedLines",
                "removedLines",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            let operations = input.get("operations");
            let Some(operations) = operations.and_then(Value::as_array) else {
                return reject("operations must be an array".to_owned());
            };
            if operations.is_empty() {
                return reject("operations must not be empty".to_owned());
            }
            for operation in operations {
                let known = operation
                    .get("op")
                    .and_then(Value::as_str)
                    .is_some_and(|op| OPS.contains(&op));
                if !known {
                    return reject(
                        "operations entries need a known op".to_owned(),
                    );
                }
            }
            for (key, max_bytes) in [
                ("targetPath", 1024usize),
                ("sourceRevision", 64),
                ("sourceSha256", 64),
                ("previewSummary", 8192),
                ("previewDiff", 65536),
            ] {
                match input.get(key).and_then(Value::as_str) {
                    Some(text)
                        if !text.trim().is_empty()
                            && text.len() <= max_bytes => {}
                    _ => {
                        return reject(format!(
                            "field {key} must be a bounded string"
                        ));
                    }
                }
            }
            if !input.get("serializedAfter").is_some_and(Value::is_string) {
                return reject("serializedAfter must be a string".to_owned());
            }
            match input.get("kind").and_then(Value::as_str) {
                Some("scene" | "resource") => {}
                _ => {
                    return reject(
                        "kind must be scene or resource".to_owned(),
                    );
                }
            }
            Ok(())
        }
        SUBJECT_GODOT_DEVELOP_PLAN => {
            const KEYS: [&str; 4] =
                ["request", "touchpoints", "projectSurfaces", "targets"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if !input.get("request").is_some_and(Value::is_string) {
                return reject("request must be a string".to_owned());
            }
            if let Some(touchpoints) = input.get("touchpoints") {
                let Some(entries) = touchpoints.as_array() else {
                    return reject("touchpoints must be an array".to_owned());
                };
                for entry in entries {
                    let status_ok = entry
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|status| {
                            matches!(status, "verified" | "candidate")
                        });
                    if !status_ok
                        || !entry.get("path").is_some_and(Value::is_string)
                    {
                        return reject(
                            "touchpoints entries need path and verified/candidate status"
                                .to_owned(),
                        );
                    }
                }
            }
            match input.get("projectSurfaces") {
                None | Some(Value::Null) => {}
                Some(surfaces) => {
                    let flags_ok = ["hasScenes", "hasResources", "hasScripts"]
                        .iter()
                        .all(|flag| {
                            surfaces.get(*flag).is_some_and(Value::is_boolean)
                        });
                    if !surfaces.is_object() || !flags_ok {
                        return reject(
                            "projectSurfaces needs hasScenes/hasResources/hasScripts booleans"
                                .to_owned(),
                        );
                    }
                }
            }
            if let Some(targets) = input.get("targets") {
                let Some(entries) = targets.as_array() else {
                    return reject("targets must be an array".to_owned());
                };
                for target in entries {
                    let scalars_ok =
                        ["targetId", "path"].iter().all(|field| {
                            target.get(*field).is_some_and(Value::is_string)
                        });
                    let references_ok =
                        target.get("references").is_none_or(|references| {
                            references.as_array().is_some_and(|entries| {
                                entries.iter().all(Value::is_string)
                            })
                        });
                    if !scalars_ok || !references_ok {
                        return reject(
                            "targets entries need targetId, path, and optional references"
                                .to_owned(),
                        );
                    }
                }
            }
            Ok(())
        }
        SUBJECT_CI_ARTIFACT_DIGEST | SUBJECT_CI_CONTRACT_DIGEST => {
            const CI_KEYS: [&str; 7] = [
                "op",
                "artifactType",
                "schemaVersion",
                "content",
                "payload",
                "contract",
                "plan",
            ];
            let mut kind_ok = true;
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !CI_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(artifact_type) = input.get("artifactType") {
                if artifact_type
                    .as_str()
                    .is_none_or(|t| t.is_empty() || t.len() > 64)
                {
                    return reject(
                        "artifactType must be a non-empty bounded string"
                            .to_owned(),
                    );
                }
            }
            if input.get("op").and_then(Value::as_str) == Some("contract") {
                // The typed seam pins type/schema itself; a fixture may
                // not smuggle caller-supplied identity strings.
                kind_ok = input.get("artifactType").is_none()
                    && input.get("schemaVersion").is_none();
            }
            if !kind_ok {
                return reject(
                    "typed contract op must not override artifact identity"
                        .to_owned(),
                );
            }
            Ok(())
        }
        SUBJECT_CI_MANIFESTS => {
            if let Some(entries) = input.get("entries") {
                if !entries.is_array() {
                    return reject("entries must be an array".to_owned());
                }
            }
            Ok(())
        }
        SUBJECT_CI_DELTA => {
            const DELTA_KEYS: [&str; 3] = ["base", "result", "keys"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !DELTA_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            Ok(())
        }
        SUBJECT_DET_REPLAY => {
            if !input.get("taskId").is_some_and(Value::is_string) {
                return reject("taskId must be a string".to_owned());
            }
            Ok(())
        }
        SUBJECT_ICM_PHASE_CONTRACT => {
            const PHASE_KEYS: [&str; 2] = ["op", "contract"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !PHASE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("create" | "registry") => Ok(()),
                _ => reject("unknown icm.phase-contract op".to_owned()),
            }
        }
        SUBJECT_ICM_DEP_MANIFESTS => {
            const DEP_KEYS: [&str; 17] = [
                "op",
                "mode",
                "artifactType",
                "artifactId",
                "dependsOn",
                "currentDigests",
                "manifests",
                "currentInputDigests",
                "preparedSourceRevisions",
                "currentSourceRevisions",
                "refs",
                "newRef",
                "itemId",
                "planItems",
                "changedSurfaces",
                "impactRelations",
                "acceptanceCriteria",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !DEP_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some(
                    "staleness"
                    | "prepared-mutation-stale"
                    | "manifest"
                    | "provenance"
                    | "why-validation-required",
                ) => Ok(()),
                _ => reject("unknown icm.dependency-manifests op".to_owned()),
            }
        }
        SUBJECT_RR_IDENTITY => {
            const IDENTITY_KEYS: [&str; 8] = [
                "op",
                "taskId",
                "phaseId",
                "sequence",
                "kind",
                "runId",
                "operation",
                "trace",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !IDENTITY_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("run-id" | "operation-id" | "trace-ref") => Ok(()),
                _ => {
                    reject("unknown runtime-readiness.identity op".to_owned())
                }
            }
        }
        SUBJECT_RR_BUDGETS => {
            const BUDGET_KEYS: [&str; 3] = ["op", "overrides", "cases"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !BUDGET_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("budget" | "admission") => Ok(()),
                _ => reject("unknown runtime-readiness.budgets op".to_owned()),
            }
        }
        SUBJECT_RR_LIFECYCLE => {
            const DRIVE_KEYS: [&str; 3] = ["op", "script", "steps"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !DRIVE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("drive")
                    if input
                        .get("script")
                        .and_then(Value::as_str)
                        .is_some_and(|script| {
                            siralos_core::runtime::FaultScript::parse(script)
                                .is_some()
                        }) =>
                {
                    Ok(())
                }
                _ => {
                    reject("unknown runtime-readiness.lifecycle op".to_owned())
                }
            }
        }
        SUBJECT_RR_DOCTOR => {
            const DOCTOR_KEYS: [&str; 12] = [
                "op",
                "mode",
                "capabilities",
                "godotAvailable",
                "godotFingerprint",
                "projectIdentity",
                "sandboxAvailable",
                "processSupervisionSupported",
                "filesystemIsolationAvailable",
                "userDataRedirectAvailable",
                "networkPolicyResolvable",
                "artifactStorageAvailable",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !DOCTOR_KEYS.contains(&key.as_str())
                    && key != "displayAvailable"
                {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("readiness" | "diagnostic") => Ok(()),
                _ => reject("unknown runtime-readiness.doctor op".to_owned()),
            }
        }
        SUBJECT_RUNTIME_EXECUTION => {
            const EXECUTION_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !EXECUTION_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("decide") => Ok(()),
                _ => reject("unknown runtime-execution op".to_owned()),
            }
        }
        SUBJECT_RUNTIME_EVIDENCE => {
            const EVIDENCE_KEYS: [&str; 2] = ["op", "input"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !EVIDENCE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("create" | "render") => Ok(()),
                _ => reject("unknown runtime-evidence op".to_owned()),
            }
        }
        SUBJECT_VISUAL_EVIDENCE => {
            const CAPTURE_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            const REQUEST_KEYS: [&str; 5] =
                ["runId", "operationId", "mode", "isStale", "frames"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !CAPTURE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("decide") {
                return reject("unknown visual-evidence op".to_owned());
            }
            let Some(request) =
                input.get("request").and_then(Value::as_object)
            else {
                return reject("request must be an object".to_owned());
            };
            for key in request.keys() {
                if !REQUEST_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected request field {key}"));
                }
            }
            if !request.get("runId").is_some_and(Value::is_string) {
                return reject("request runId must be a string".to_owned());
            }
            if let Some(operation_id) = request.get("operationId") {
                if !operation_id.is_null() && !operation_id.is_string() {
                    return reject(
                        "request operationId must be a string or null"
                            .to_owned(),
                    );
                }
            }
            match request.get("mode").and_then(Value::as_str) {
                Some("visual" | "headless") => {}
                _ => {
                    return reject(
                        "request mode is not a runtime mode".to_owned(),
                    );
                }
            }
            if !request.get("isStale").is_some_and(Value::is_boolean) {
                return reject("request isStale must be a boolean".to_owned());
            }
            let Some(frames) = request.get("frames").and_then(Value::as_array)
            else {
                return reject("request frames must be an array".to_owned());
            };
            if frames.is_empty() {
                return reject("request frames must not be empty".to_owned());
            }
            for frame in frames {
                if !frame.is_string() {
                    return reject(
                        "request frames must be strings".to_owned(),
                    );
                }
            }
            if !input.get("isCancelled").is_some_and(Value::is_boolean) {
                return reject("isCancelled must be a boolean".to_owned());
            }
            Ok(())
        }
        SUBJECT_RUN_INTERACTION => {
            const INTERACTION_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            const REQUEST_KEYS: [&str; 5] =
                ["runId", "operationId", "isInteractive", "isStale", "rounds"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !INTERACTION_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("decide") {
                return reject("unknown run-interaction op".to_owned());
            }
            let Some(request) =
                input.get("request").and_then(Value::as_object)
            else {
                return reject("request must be an object".to_owned());
            };
            for key in request.keys() {
                if !REQUEST_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected request field {key}"));
                }
            }
            if !request.get("runId").is_some_and(Value::is_string) {
                return reject("request runId must be a string".to_owned());
            }
            if let Some(operation_id) = request.get("operationId") {
                if !operation_id.is_null() && !operation_id.is_string() {
                    return reject(
                        "request operationId must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if !request.get("isInteractive").is_some_and(Value::is_boolean) {
                return reject(
                    "request isInteractive must be a boolean".to_owned(),
                );
            }
            if !request.get("isStale").is_some_and(Value::is_boolean) {
                return reject("request isStale must be a boolean".to_owned());
            }
            let Some(rounds) = request.get("rounds").and_then(Value::as_array)
            else {
                return reject("request rounds must be an array".to_owned());
            };
            if rounds.is_empty() {
                return reject("request rounds must not be empty".to_owned());
            }
            for round in rounds {
                if !round.is_string() {
                    return reject(
                        "request rounds must be strings".to_owned(),
                    );
                }
            }
            if !input.get("isCancelled").is_some_and(Value::is_boolean) {
                return reject("isCancelled must be a boolean".to_owned());
            }
            Ok(())
        }
        SUBJECT_QA_WORKFLOW => {
            const WORKFLOW_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            const REQUEST_KEYS: [&str; 4] =
                ["runId", "operationId", "isStale", "steps"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !WORKFLOW_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("decide") {
                return reject("unknown qa-workflow op".to_owned());
            }
            let Some(request) =
                input.get("request").and_then(Value::as_object)
            else {
                return reject("request must be an object".to_owned());
            };
            for key in request.keys() {
                if !REQUEST_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected request field {key}"));
                }
            }
            if !request.get("runId").is_some_and(Value::is_string) {
                return reject("request runId must be a string".to_owned());
            }
            if let Some(operation_id) = request.get("operationId") {
                if !operation_id.is_null() && !operation_id.is_string() {
                    return reject(
                        "request operationId must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if !request.get("isStale").is_some_and(Value::is_boolean) {
                return reject("request isStale must be a boolean".to_owned());
            }
            let Some(steps) = request.get("steps").and_then(Value::as_array)
            else {
                return reject("request steps must be an array".to_owned());
            };
            for step in steps {
                if !step.is_string() {
                    return reject("request steps must be strings".to_owned());
                }
            }
            if !input.get("isCancelled").is_some_and(Value::is_boolean) {
                return reject("isCancelled must be a boolean".to_owned());
            }
            Ok(())
        }
        SUBJECT_RUN_PROFILE => {
            const PROFILE_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            const REQUEST_KEYS: [&str; 4] =
                ["runId", "operationId", "isStale", "samples"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !PROFILE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("decide") {
                return reject("unknown run-profile op".to_owned());
            }
            let Some(request) =
                input.get("request").and_then(Value::as_object)
            else {
                return reject("request must be an object".to_owned());
            };
            for key in request.keys() {
                if !REQUEST_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected request field {key}"));
                }
            }
            if !request.get("runId").is_some_and(Value::is_string) {
                return reject("request runId must be a string".to_owned());
            }
            if let Some(operation_id) = request.get("operationId") {
                if !operation_id.is_null() && !operation_id.is_string() {
                    return reject(
                        "request operationId must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if !request.get("isStale").is_some_and(Value::is_boolean) {
                return reject("request isStale must be a boolean".to_owned());
            }
            let Some(samples) =
                request.get("samples").and_then(Value::as_array)
            else {
                return reject("request samples must be an array".to_owned());
            };
            for sample in samples {
                if !sample.is_string() {
                    return reject(
                        "request samples must be strings".to_owned(),
                    );
                }
            }
            if !input.get("isCancelled").is_some_and(Value::is_boolean) {
                return reject("isCancelled must be a boolean".to_owned());
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_PROFILE => {
            const PROFILE_KEYS: [&str; 3] = ["op", "document", "hostPolicy"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !PROFILE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("resolve") {
                return reject("unknown composition-profile op".to_owned());
            }
            if let Some(document) = input.get("document") {
                if !document.is_string() {
                    return reject("document must be a string".to_owned());
                }
            }
            if let Some(policy) = input.get("hostPolicy") {
                let Some(map) = policy.as_object() else {
                    return reject("hostPolicy must be an object".to_owned());
                };
                for (key, value) in map {
                    if !value.is_string() {
                        return reject(format!(
                            "hostPolicy rule for {key} must be a string"
                        ));
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_EFFECTIVE => {
            const EFFECTIVE_KEYS: [&str; 2] = ["document", "hostPolicy"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !EFFECTIVE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(document) = input.get("document") {
                if !document.is_string() {
                    return reject("document must be a string".to_owned());
                }
            }
            let Some(policy) =
                input.get("hostPolicy").and_then(Value::as_object)
            else {
                return reject("hostPolicy must be an object".to_owned());
            };
            for (key, value) in policy {
                if !value.is_string() {
                    return reject(format!(
                        "hostPolicy rule for {key} must be a string"
                    ));
                }
            }
            Ok(())
        }
        SUBJECT_CONTEXT_CONTROLS => {
            const CONTROL_KEYS: [&str; 2] = ["actualDigest", "policy"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !CONTROL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if !input.get("actualDigest").and_then(Value::as_str).is_some() {
                return reject("actualDigest must be a string".to_owned());
            }
            let Some(policy) = input.get("policy").and_then(Value::as_object)
            else {
                return reject("policy must be an object".to_owned());
            };
            let Some(kind) = policy.get("kind").and_then(Value::as_str) else {
                return reject("policy requires a kind string".to_owned());
            };
            if !matches!(kind, "live" | "pinned" | "frozen") {
                return reject(format!("unknown policy kind {kind}"));
            }
            if kind != "live"
                && !policy.get("digest").and_then(Value::as_str).is_some()
            {
                return reject(format!(
                    "policy {kind} requires a digest string"
                ));
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_LOCK => {
            const LOCK_KEYS: [&str; 3] =
                ["plugins", "profileDigest", "storedLockDigest"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !LOCK_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(digest) = input.get("profileDigest") {
                if digest.as_str().is_none() {
                    return reject(
                        "profileDigest must be a string".to_owned(),
                    );
                }
            }
            if let Some(stored) = input.get("storedLockDigest") {
                if stored.as_str().is_none() {
                    return reject(
                        "storedLockDigest must be a string".to_owned(),
                    );
                }
            }
            if let Some(plugins) = input.get("plugins") {
                let Some(list) = plugins.as_array() else {
                    return reject("plugins must be an array".to_owned());
                };
                if list.len() > 16 {
                    return reject(
                        "plugins exceeds the 16-entry bound".to_owned(),
                    );
                }
                for entry in list {
                    let Some(table) = entry.as_object() else {
                        return reject(
                            "each plugin entry must be an object".to_owned(),
                        );
                    };
                    for key in table.keys() {
                        if !matches!(key.as_str(), "digest" | "id" | "path") {
                            return reject(format!(
                                "unexpected plugin field {key}"
                            ));
                        }
                    }
                    for key in ["digest", "id", "path"] {
                        if table.get(key).and_then(Value::as_str).is_none() {
                            return reject(format!(
                                "plugin entry requires a {key} string"
                            ));
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_MODULE_SELECTION => {
            const SELECTION_KEYS: [&str; 2] = ["enabled", "selected"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !SELECTION_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            for key in ["enabled", "selected"] {
                if let Some(list) = input.get(key) {
                    let Some(entries) = list.as_array() else {
                        return reject(format!("{key} must be an array"));
                    };
                    if entries.len() > 16 {
                        return reject(format!(
                            "{key} exceeds the 16-entry bound"
                        ));
                    }
                    for entry in entries {
                        if entry.as_str().is_none() {
                            return reject(format!(
                                "{key} entries must be strings"
                            ));
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_SKILLS => {
            const SKILL_KEYS: [&str; 2] = ["selected", "skills"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !SKILL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(list) = input.get("selected") {
                let Some(entries) = list.as_array() else {
                    return reject("selected must be an array".to_owned());
                };
                if entries.len() > 32 {
                    return reject(
                        "selected exceeds the 32-entry bound".to_owned(),
                    );
                }
                for entry in entries {
                    if entry.as_str().is_none() {
                        return reject(
                            "selected entries must be strings".to_owned(),
                        );
                    }
                }
            }
            if let Some(list) = input.get("skills") {
                let Some(entries) = list.as_array() else {
                    return reject("skills must be an array".to_owned());
                };
                if entries.len() > 32 {
                    return reject(
                        "skills exceeds the 32-entry bound".to_owned(),
                    );
                }
                for entry in entries {
                    let Some(table) = entry.as_object() else {
                        return reject(
                            "each skill must be an object".to_owned(),
                        );
                    };
                    for key in table.keys() {
                        if !matches!(key.as_str(), "content" | "name") {
                            return reject(format!(
                                "unexpected skill field {key}"
                            ));
                        }
                    }
                    for key in ["content", "name"] {
                        if table.get(key).and_then(Value::as_str).is_none() {
                            return reject(format!(
                                "skill requires a {key} string"
                            ));
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_MODULE_ACTIVATION => {
            const ACTIVATION_KEYS: [&str; 3] =
                ["enabled", "requested", "selected"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !ACTIVATION_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            let Some(enabled) = input.get("enabled").and_then(Value::as_array)
            else {
                return reject("enabled must be an array".to_owned());
            };
            if enabled.len() > 16 {
                return reject(
                    "enabled exceeds the 16-entry bound".to_owned(),
                );
            }
            for entry in enabled {
                if entry.as_str().is_none() {
                    return reject(
                        "enabled entries must be strings".to_owned(),
                    );
                }
            }
            if input.get("requested").and_then(Value::as_str).is_none() {
                return reject("requested must be a string".to_owned());
            }
            if let Some(list) = input.get("selected") {
                let Some(entries) = list.as_array() else {
                    return reject("selected must be an array".to_owned());
                };
                if entries.len() > 16 {
                    return reject(
                        "selected exceeds the 16-entry bound".to_owned(),
                    );
                }
                for entry in entries {
                    if entry.as_str().is_none() {
                        return reject(
                            "selected entries must be strings".to_owned(),
                        );
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_CONTEXT_CONTROL => {
            const CONTROL_KEYS: [&str; 3] = ["actual", "digest", "kind"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !CONTROL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            let actual =
                input.get("actual").and_then(Value::as_str).unwrap_or("");
            if actual.is_empty() || actual.len() > 128 {
                return reject(
                    "actual must be a non-empty bounded string".to_owned(),
                );
            }
            if let Some(kind) = input.get("kind") {
                let Some(kind_text) = kind.as_str() else {
                    return reject("kind must be a string".to_owned());
                };
                if !["live", "pinned", "frozen"].contains(&kind_text) {
                    return reject(format!(
                        "unknown context kind {kind_text}"
                    ));
                }
            }
            if let Some(digest) = input.get("digest") {
                let Some(digest_text) = digest.as_str() else {
                    return reject("digest must be a string".to_owned());
                };
                if digest_text.is_empty() || digest_text.len() > 128 {
                    return reject(
                        "digest must be a bounded string".to_owned(),
                    );
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_LOCK_VERIFY => {
            const LOCK_KEYS: [&str; 3] =
                ["plugins", "profileDigest", "stored"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !LOCK_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(digest) = input.get("profileDigest") {
                let Some(digest_text) = digest.as_str() else {
                    return reject(
                        "profileDigest must be a string".to_owned(),
                    );
                };
                if digest_text.len() != 64 {
                    return reject(
                        "profileDigest must be 64 hex characters".to_owned(),
                    );
                }
            }
            let lock_plugins = |value: &Value| -> Result<(), String> {
                let Some(list) = value.as_array() else {
                    return Err("plugins must be an array".to_owned());
                };
                for entry in list {
                    let Some(table) = entry.as_object() else {
                        return Err(
                            "plugin entries must be objects".to_owned()
                        );
                    };
                    for key in ["id", "path", "digest"] {
                        if !table.get(key).and_then(Value::as_str).is_some_and(
                            |text| !text.is_empty() && text.len() <= 256,
                        ) {
                            return Err(format!(
                                "plugin field {key} must be a bounded string"
                            ));
                        }
                    }
                }
                Ok(())
            };
            if let Some(plugins) = input.get("plugins") {
                if let Err(message) = lock_plugins(plugins) {
                    return reject(message);
                }
            }
            match input.get("stored") {
                None | Some(Value::Null) => {}
                Some(stored) => {
                    let Some(table) = stored.as_object() else {
                        return reject(
                            "stored must be null or an object".to_owned(),
                        );
                    };
                    for key in table.keys() {
                        if !["plugins", "profileDigest", "recordedDigest"]
                            .contains(&key.as_str())
                        {
                            return reject(format!(
                                "unexpected stored field {key}"
                            ));
                        }
                    }
                    if let Some(plugins) = table.get("plugins") {
                        if let Err(message) = lock_plugins(plugins) {
                            return reject(message);
                        }
                    }
                    if let Some(recorded) = table.get("recordedDigest") {
                        let Some(text) = recorded.as_str() else {
                            return reject(
                                "recordedDigest must be a string".to_owned(),
                            );
                        };
                        if text.len() != 64 {
                            return reject(
                                "recordedDigest must be 64 hex characters"
                                    .to_owned(),
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_COMPOSITION_SKILL_CONSUMPTION => {
            const SKILL_KEYS: [&str; 2] = ["selected", "skills"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !SKILL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(skills) = input.get("skills") {
                let Some(list) = skills.as_array() else {
                    return reject("skills must be an array".to_owned());
                };
                for entry in list {
                    let Some(table) = entry.as_object() else {
                        return reject(
                            "skill entries must be objects".to_owned(),
                        );
                    };
                    for key in ["content", "name"] {
                        let Some(text) =
                            table.get(key).and_then(Value::as_str)
                        else {
                            return Err(HarnessError::corpus(format!(
                                "skill field {key} must be a string"
                            )));
                        };
                        if text.is_empty() || text.len() > 4096 {
                            return reject(format!(
                                "skill field {key} must be a bounded string"
                            ));
                        }
                    }
                }
            }
            if let Some(selected) = input.get("selected") {
                let Some(list) = selected.as_array() else {
                    return reject("selected must be an array".to_owned());
                };
                for name in list {
                    let Some(text) = name.as_str() else {
                        return reject(
                            "selected entries must be strings".to_owned(),
                        );
                    };
                    if text.is_empty() || text.len() > 256 {
                        return reject(
                            "selected entries must be bounded strings"
                                .to_owned(),
                        );
                    }
                }
            }
            Ok(())
        }
        SUBJECT_EVOLVE_CORPUS => {
            const EVOLVE_KEYS: [&str; 2] = ["candidate", "corpus"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !EVOLVE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(corpus) = input.get("corpus") {
                if !corpus.is_null() && !corpus.is_object() {
                    return reject(
                        "corpus must be an object or null".to_owned(),
                    );
                }
                if let Some(table) = corpus.as_object() {
                    for key in table.keys() {
                        if !["id", "cases"].contains(&key.as_str()) {
                            return reject(format!(
                                "unexpected corpus field {key}"
                            ));
                        }
                    }
                    if let Some(id) = table.get("id") {
                        if !id.is_string() {
                            return reject(
                                "corpus id must be a string".to_owned(),
                            );
                        }
                    }
                    if let Some(cases) = table.get("cases") {
                        let Some(list) = cases.as_array() else {
                            return reject(
                                "corpus cases must be an array".to_owned(),
                            );
                        };
                        if list.len() > 64 {
                            return reject(
                                "corpus cases exceeds 64".to_owned(),
                            );
                        }
                    }
                }
            }
            if let Some(candidate) = input.get("candidate") {
                if !candidate.is_null() && !candidate.is_object() {
                    return reject(
                        "candidate must be an object or null".to_owned(),
                    );
                }
            }
            Ok(())
        }
        SUBJECT_EVOLVE_WORKFLOW => {
            const WORKFLOW_KEYS: [&str; 3] =
                ["baseline", "candidate", "escalation"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !WORKFLOW_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("escalation").and_then(Value::as_str).is_none() {
                return reject("escalation must be a string".to_owned());
            }
            for side in ["baseline", "candidate"] {
                let Some(table) = input.get(side).and_then(Value::as_object)
                else {
                    return reject(format!("{side} must be an object"));
                };
                for key in table.keys() {
                    if !["candidate", "corpus"].contains(&key.as_str()) {
                        return reject(format!(
                            "unexpected {side} field {key}"
                        ));
                    }
                }
                if let Some(corpus) = table.get("corpus") {
                    if !corpus.is_null() && !corpus.is_object() {
                        return reject(format!(
                            "{side} corpus must be object or null"
                        ));
                    }
                }
                if let Some(candidate) = table.get("candidate") {
                    if !candidate.is_null() && !candidate.is_object() {
                        return reject(format!(
                            "{side} candidate must be object or null"
                        ));
                    }
                }
            }
            Ok(())
        }
        SUBJECT_EVOLVE_PROPOSAL => {
            const PROPOSAL_KEYS: [&str; 1] = ["proposal"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !PROPOSAL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(proposal) = input.get("proposal") {
                if !proposal.is_null() && !proposal.is_object() {
                    return reject(
                        "proposal must be an object or null".to_owned(),
                    );
                }
                if let Some(table) = proposal.as_object() {
                    for key in table.keys() {
                        if ![
                            "description",
                            "id",
                            "kind",
                            "requiresHostApproval",
                            "workflowDigest",
                        ]
                        .contains(&key.as_str())
                        {
                            return reject(format!(
                                "unexpected proposal field {key}"
                            ));
                        }
                    }
                    if let Some(id) = table.get("id") {
                        if !id.is_string() {
                            return reject(
                                "proposal id must be a string".to_owned(),
                            );
                        }
                    }
                    if let Some(digest) = table.get("workflowDigest") {
                        if !digest.is_string() {
                            return reject(
                                "proposal workflowDigest must be a string"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(kind) = table.get("kind") {
                        if !kind.is_string() {
                            return reject(
                                "proposal kind must be a string".to_owned(),
                            );
                        }
                    }
                    if let Some(desc) = table.get("description") {
                        if !desc.is_string() {
                            return reject(
                                "proposal description must be a string"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(flag) = table.get("requiresHostApproval") {
                        if !flag.is_boolean() {
                            return reject(
                                "proposal requiresHostApproval must be a boolean".to_owned(),
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_EVOLVE_PACKAGING => {
            const PACKAGING_KEYS: [&str; 1] = ["release"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !PACKAGING_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if let Some(release) = input.get("release") {
                if !release.is_null() && !release.is_object() {
                    return reject(
                        "release must be an object or null".to_owned(),
                    );
                }
                if let Some(table) = release.as_object() {
                    for key in table.keys() {
                        if ![
                            "compatibility",
                            "id",
                            "previousVersion",
                            "version",
                        ]
                        .contains(&key.as_str())
                        {
                            return reject(format!(
                                "unexpected release field {key}"
                            ));
                        }
                    }
                    if let Some(id) = table.get("id") {
                        if !id.is_string() {
                            return reject(
                                "release id must be a string".to_owned(),
                            );
                        }
                    }
                    if let Some(version) = table.get("version") {
                        if !version.is_string() {
                            return reject(
                                "release version must be a string".to_owned(),
                            );
                        }
                    }
                    if let Some(prev) = table.get("previousVersion") {
                        if !prev.is_string() {
                            return reject(
                                "release previousVersion must be a string"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(compat) = table.get("compatibility") {
                        if !compat.is_string() {
                            return reject(
                                "release compatibility must be a string"
                                    .to_owned(),
                            );
                        }
                    }
                }
            }
            Ok(())
        }
        SUBJECT_GODOT_RUNTIME_LAUNCH => {
            const LAUNCH_KEYS: [&str; 5] =
                ["op", "request", "policy", "budget", "isCancelled"];
            const REQUEST_KEYS: [&str; 8] = [
                "engineId",
                "engineVersion",
                "projectPath",
                "mode",
                "runId",
                "operationId",
                "isStale",
                "requestedBytes",
            ];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !LAUNCH_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("decide") {
                return reject("unknown godot-runtime-launch op".to_owned());
            }
            let Some(request) =
                input.get("request").and_then(Value::as_object)
            else {
                return reject("request must be an object".to_owned());
            };
            for key in request.keys() {
                if !REQUEST_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected request field {key}"));
                }
            }
            for key in ["engineId", "engineVersion", "projectPath", "runId"] {
                if !request.get(key).is_some_and(Value::is_string) {
                    return reject(format!("request {key} must be a string"));
                }
            }
            match request.get("mode").and_then(Value::as_str) {
                Some(
                    "project" | "check-only" | "recovery-project" | "lsp-only",
                ) => {}
                _ => {
                    return reject(
                        "request mode is not a Godot launch mode".to_owned(),
                    );
                }
            }
            if let Some(operation_id) = request.get("operationId") {
                if !operation_id.is_null() && !operation_id.is_string() {
                    return reject(
                        "request operationId must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if !request.get("isStale").is_some_and(Value::is_boolean) {
                return reject("request isStale must be a boolean".to_owned());
            }
            if !request.get("requestedBytes").is_some_and(Value::is_u64) {
                return reject(
                    "request requestedBytes must be a non-negative integer"
                        .to_owned(),
                );
            }
            if !input.get("isCancelled").is_some_and(Value::is_boolean) {
                return reject("isCancelled must be a boolean".to_owned());
            }
            Ok(())
        }
        SUBJECT_GODOT_RUNTIME_EVIDENCE => {
            const EVIDENCE_KEYS: [&str; 3] = ["op", "input", "detail"];
            const EVIDENCE_INPUT_KEYS: [&str; 8] = [
                "runId",
                "operationId",
                "exitCode",
                "durationMs",
                "stdout",
                "stderr",
                "large",
                "largeWithEmoji",
            ];
            const DETAIL_KEYS: [&str; 4] =
                ["engineId", "engineVersion", "projectPath", "mode"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !EVIDENCE_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            if input.get("op").and_then(Value::as_str) != Some("create") {
                return reject("unknown godot-runtime-evidence op".to_owned());
            }
            let Some(evidence) = input.get("input").and_then(Value::as_object)
            else {
                return reject("input must be an object".to_owned());
            };
            for key in evidence.keys() {
                if !EVIDENCE_INPUT_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected input field {key}"));
                }
            }
            for key in ["runId", "operationId", "stdout", "stderr"] {
                if !evidence.get(key).is_some_and(Value::is_string) {
                    return reject(format!("input {key} must be a string"));
                }
            }
            if !evidence
                .get("exitCode")
                .is_some_and(|value| value.is_null() || value.is_i64())
            {
                return reject(
                    "input exitCode must be an integer or null".to_owned(),
                );
            }
            if !evidence.get("durationMs").is_some_and(Value::is_u64) {
                return reject(
                    "input durationMs must be a non-negative integer"
                        .to_owned(),
                );
            }
            for key in ["large", "largeWithEmoji"] {
                if let Some(flag) = evidence.get(key) {
                    if !flag.is_boolean() {
                        return reject(format!(
                            "input {key} must be a boolean"
                        ));
                    }
                }
            }
            let Some(detail) = input.get("detail").and_then(Value::as_object)
            else {
                return reject("detail must be an object".to_owned());
            };
            for key in detail.keys() {
                if !DETAIL_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected detail field {key}"));
                }
            }
            for key in DETAIL_KEYS {
                if !detail.get(key).is_some_and(Value::is_string) {
                    return reject(format!("detail {key} must be a string"));
                }
            }
            match detail.get("mode").and_then(Value::as_str) {
                Some(
                    "project" | "check-only" | "recovery-project" | "lsp-only",
                ) => {}
                _ => {
                    return reject(
                        "detail mode is not a Godot launch mode".to_owned(),
                    );
                }
            }
            Ok(())
        }
        SUBJECT_RECOVERY_TAXONOMY => {
            const TAXONOMY_KEYS: [&str; 6] =
                ["op", "cases", "kind", "missing", "resourceKind", "reason"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !TAXONOMY_KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            match input.get("op").and_then(Value::as_str) {
                Some("retry-classification" | "incomplete-run") => Ok(()),
                Some("domain-failure")
                    if input
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| {
                            matches!(
                                kind,
                                "CAPABILITY_DENIED"
                                    | "STALE_ACTIVATION"
                                    | "RESOURCE_EXCEEDED"
                                    | "UNAVAILABLE"
                                    | "CANCELLED"
                            )
                        }) =>
                {
                    Ok(())
                }
                _ => reject("unknown recovery-taxonomy op".to_owned()),
            }
        }
        _ => unreachable!("godot subject was validated above"),
    }
}
