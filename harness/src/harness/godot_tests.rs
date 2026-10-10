use super::godot_record;
use serde_json::{Value, json};

fn record(input: Value) -> Value {
    godot_record("godot-scene-resolve", &input)
        .expect("scene-resolve record succeeds")
}

#[test]
fn scene_resolve_transcribes_the_oracle_selection_rule() {
    // Null tres is a MISSING key, never a resource marker.
    assert_eq!(
        record(json!({"tres": null, "tscn": "[gd_scene]\n"})),
        json!({"status": "complete", "diagnostics": 0, "truncated": false})
    );
    // Unknown keys are ignored; empty scene text parses on default path "".
    assert_eq!(
        record(json!({"content": "[gd_resource type=\"Resource\"]"})),
        json!({"status": "invalid", "diagnostics": 0, "truncated": false})
    );
    // tres wins over tscn.
    assert_eq!(
        record(json!({"tres": "[gd_resource]", "tscn": "[gd_scene]"})),
        json!({"status": "partial", "diagnostics": 1, "truncated": false})
    );
    // A resource body under tscn parses as a resource; the typed
    // header alone is complete, while an untyped header is partial.
    assert_eq!(
        record(json!({"tscn": "[gd_resource type=\"Resource\"]"})),
        json!({"status": "complete", "diagnostics": 0, "truncated": false})
    );
}

#[test]
fn knowledge_search_validates_before_availability() {
    let invalid = godot_record(
        "godot-knowledge",
        &json!({"op": "search", "query": "   "}),
    )
    .unwrap();
    assert_eq!(
        invalid,
        json!({"status": "invalid_input", "message": "A non-empty query is required."})
    );
    let unavailable = godot_record(
        "godot-knowledge",
        &json!({"op": "search", "query": "node"}),
    )
    .unwrap();
    assert_eq!(unavailable["status"], "unavailable");
    assert_eq!(
        unavailable["message"],
        "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform."
    );
}

#[test]
fn knowledge_status_reports_unavailable_platform_facts() {
    let status = godot_record(
        "godot-knowledge",
        &json!({"op": "status", "platform": "linux"}),
    )
    .unwrap();
    assert_eq!(status["state"], "unavailable");
    assert_eq!(status["platform"], "linux");
    assert_eq!(status["cacheEnabled"], false);
    assert_eq!(status["schemaVersion"], 1);
}

#[test]
fn diagnostics_prepare_refuses_before_effects() {
    let prepared = godot_record(
        "godot-diagnostics",
        &json!({"op": "prepare", "paths": ["src/player.gd"]}),
    )
    .unwrap();
    assert_eq!(
        prepared,
        json!({
            "status": "unsupported",
            "message": "No trusted Godot installation is selected; GDScript diagnostics cannot run."
        })
    );
    let cancelled = godot_record(
        "godot-diagnostics",
        &json!({"op": "prepare", "cancelled": true}),
    )
    .unwrap();
    assert_eq!(
        cancelled,
        json!({"status": "cancelled", "message": "The Godot project operation was aborted."})
    );
    let executed =
        godot_record("godot-diagnostics", &json!({"op": "execute"})).unwrap();
    assert_eq!(
        executed,
        json!({
            "status": "failed",
            "message": "The prepared check is not valid for this session; prepare a new check."
        })
    );
}

#[test]
fn lsp_prepare_refuses_and_status_stays_empty() {
    let prepared =
        godot_record("godot-lsp", &json!({"op": "prepare"})).unwrap();
    assert_eq!(
        prepared,
        json!({
            "status": "unsupported",
            "message": "No trusted Godot installation is selected; the language session cannot start."
        })
    );
    let status = godot_record("godot-lsp", &json!({"op": "status"})).unwrap();
    assert_eq!(
        status,
        json!({
            "state": "unavailable",
            "openDocumentCount": 0,
            "diagnosticCount": 0,
            "networkIsolation": "unavailable",
        })
    );
}

#[test]
fn discovery_reports_missing_explicit_paths_fail_closed() {
    let explicit = godot_record(
        "godot-discovery",
        &json!({
            "op": "select",
            "preference": {"path": "/no-such-place/godot.exe"}
        }),
    )
    .unwrap();
    assert_eq!(explicit["ok"], false);
    assert!(
        explicit["error"]
            .as_str()
            .is_some_and(|message| message.contains("did not resolve"))
    );
}

#[test]
fn discovery_auto_with_no_candidates_stays_deterministic() {
    let discovered = godot_record(
        "godot-discovery",
        &json!({
            "op": "discover",
            "hostPath": "/no-such-a:/no-such-b",
            "config": {"discoverOnPath": true}
        }),
    )
    .unwrap();
    assert_eq!(discovered["ok"], true);
    assert_eq!(discovered["selected"], Value::Null);
    assert_eq!(discovered["candidates"], json!([]),);
    assert_eq!(
        discovered["configuration"],
        json!({
            "activeInstallation": null,
            "configuredCount": 0,
            "discoverOnPath": true,
            "overrides": [],
        })
    );
    assert_eq!(
        discovered["rationale"][0],
        "No selectable Godot installation was discovered."
    );
    assert_eq!(discovered["diagnostics"], json!([]));
}

#[test]
fn discovery_configured_missing_installation_is_invalid_not_selected() {
    let discovered = godot_record(
            "godot-discovery",
            &json!({
                "op": "discover",
                "config": {
                    "installations": [
                        {"id": "cfg-1", "path": "/missing/Godot.exe", "editionHint": "standard"}
                    ]
                }
            }),
        )
        .unwrap();
    assert_eq!(discovered["ok"], true);
    assert_eq!(discovered["selected"], Value::Null);
    let candidate = &discovered["candidates"][0];
    assert_eq!(candidate["id"], "cfg-1");
    assert_eq!(candidate["source"], "user-config");
    assert_eq!(candidate["selected"], false);
    assert!(
        candidate["invalid"]
            .as_str()
            .is_some_and(|message| message.contains("does not exist"))
    );
}
