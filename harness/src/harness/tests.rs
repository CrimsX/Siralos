use super::{
    HarnessErrorKind, PLATFORM_POSIX, PLATFORM_WINDOWS,
    canonical_records_text, canonical_scenario_digest,
    cargo_workspace_version, execute_bounded_child, fuzz_scenario_bytes,
    load_corpus, platform_name, probe_state_dir_bytes, provider_turn_record,
    state_dir_record, validate_provider_turn_input, validate_scenario,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::Scenario;

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempCorpus(PathBuf);

impl TempCorpus {
    fn copy() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let source = root.join("tests/differential/corpus");
        let destination = std::env::temp_dir().join(format!(
            "siralos-rust-corpus-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&destination).expect("create temp corpus");
        for entry in std::fs::read_dir(source).expect("read corpus") {
            let entry = entry.expect("corpus entry");
            std::fs::copy(entry.path(), destination.join(entry.file_name()))
                .expect("copy corpus entry");
        }
        Self(destination)
    }
}

impl Drop for TempCorpus {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove temp corpus");
    }
}

fn scenario() -> Scenario {
    Scenario {
        id: "state-dir.set.posix".to_owned(),
        subject: "state-dir".to_owned(),
        platforms: vec!["posix".to_owned()],
        parity: "required".to_owned(),
        env: BTreeMap::from([("HOME".to_owned(), "/fixture/home".to_owned())]),
        input: None,
    }
}

#[test]
fn platform_name_is_one_of_the_contract_values() {
    assert!(matches!(platform_name(), PLATFORM_WINDOWS | PLATFORM_POSIX));
}

#[test]
fn state_dir_record_hashes_raw_ok_bytes() {
    let record = state_dir_record(
        "state-dir.set.windows",
        b"C:\\fixture\\home\\.siralos",
    );
    assert_eq!(record["outcome"], "COMPLETED");
    assert_eq!(record["result"]["stateDirSha256"].as_str().unwrap().len(), 64);
}

#[test]
fn state_dir_record_marks_only_the_exact_error_marker() {
    let record = state_dir_record("state-dir.unset.windows", b"ERR");
    assert_eq!(record["outcome"], "PRODUCT_FAILURE");
    assert_eq!(record["error"]["category"], "NO_HOME_DIRECTORY");

    let non_utf8 = state_dir_record("state-dir.set.posix", &[0xff]);
    assert_eq!(non_utf8["outcome"], "COMPLETED");
}

#[test]
fn records_are_canonical_with_one_trailing_newline() {
    let records = vec![
        json!({"scenarioId": "b", "kind": "ok"}),
        json!({"scenarioId": "a", "kind": "ok"}),
    ];
    assert_eq!(
        canonical_records_text(records),
        "{\"records\":[{\"kind\":\"ok\",\"scenarioId\":\"b\"},{\"kind\":\"ok\",\"scenarioId\":\"a\"}],\"schemaVersion\":1}\n"
    );
}

#[test]
fn scenario_validation_rejects_unknown_environment_authority() {
    let mut input = scenario();
    input.env.insert("NODE_OPTIONS".to_owned(), "--require=x".to_owned());
    assert!(validate_scenario(&input, "state-dir.set.posix.json").is_err());
}

#[test]
fn scenario_validation_rejects_id_file_mismatch() {
    assert!(validate_scenario(&scenario(), "other.json").is_err());
}

#[test]
fn scenario_validation_rejects_required_os_home_fallback() {
    let mut input = scenario();
    input.env.insert("HOME".to_owned(), String::new());
    let error = validate_scenario(&input, "state-dir.set.posix.json")
        .expect_err("required fallback is uncontrolled");
    assert!(error.to_string().contains("does not fully declare"));
}

#[test]
fn scenario_digest_matches_the_javascript_canonical_order() {
    assert_eq!(
        canonical_scenario_digest(&scenario()).expect("canonical"),
        "50508c3bb501926a100bbcbd9567bac8add9b125a4bd234da11cafbad84202ef"
    );
}

#[test]
fn strict_loader_accepts_the_checked_in_digest_bound_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let loaded =
        load_corpus(&root.join("tests/differential/corpus"), platform_name())
            .expect("checked-in corpus");
    assert_eq!(loaded.len(), 314);
}

#[test]
fn fuzz_entry_accepts_a_valid_scenario_shape() {
    let bytes = serde_json::to_vec(&scenario()).expect("bytes");
    assert!(fuzz_scenario_bytes(&bytes).is_ok());
}

#[test]
fn fuzz_entry_rejects_arbitrary_bytes_with_typed_errors() {
    let oversized = vec![b'x'; 17 * 1024];
    for input in [
        b"".as_slice(),
        b"{",
        b"[]",
        b"null",
        b"\xff\xfe",
        oversized.as_slice(),
    ] {
        let error =
            fuzz_scenario_bytes(input).expect_err("malformed rejected");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
    }
}

#[test]
fn strict_loader_rejects_digest_tampering() {
    let corpus = TempCorpus::copy();
    let path = corpus.0.join("state-dir.set.posix.json");
    let text = std::fs::read_to_string(&path).expect("scenario");
    std::fs::write(&path, text.replace("/fixture/home", "/tampered"))
        .expect("tamper scenario");
    let error = load_corpus(&corpus.0, PLATFORM_POSIX)
        .err()
        .expect("tampering rejected");
    assert_eq!(error.kind(), HarnessErrorKind::Corpus);
    assert_eq!(error.code(), "CONTENT_MISMATCH");
    assert!(error.to_string().contains("manifest digest"));
}

#[test]
fn strict_loader_distinguishes_corpus_digest_failures() {
    for (replacement, expected_code) in [
        (None, "MISSING_DIGEST"),
        (Some("invalid"), "MALFORMED_DIGEST"),
        (
            Some(
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
            "CONTENT_MISMATCH",
        ),
    ] {
        let corpus = TempCorpus::copy();
        let path = corpus.0.join("manifest.json");
        let text = std::fs::read_to_string(&path).expect("manifest");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&text).expect("valid manifest JSON");
        let object = manifest.as_object_mut().expect("manifest object");
        if let Some(replacement) = replacement {
            object.insert(
                "corpusSha256".to_owned(),
                serde_json::Value::String(replacement.to_owned()),
            );
        } else {
            object.remove("corpusSha256");
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("alter manifest");
        let error = load_corpus(&corpus.0, PLATFORM_POSIX)
            .err()
            .expect("invalid corpus digest rejected");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
        assert_eq!(error.code(), expected_code);
    }
}

#[test]
fn strict_loader_rejects_unknown_fields() {
    let corpus = TempCorpus::copy();
    let path = corpus.0.join("state-dir.set.posix.json");
    let text = std::fs::read_to_string(&path).expect("scenario");
    std::fs::write(&path, text.replace("\n}", ",\n  \"unexpected\": true\n}"))
        .expect("alter scenario");
    let error = load_corpus(&corpus.0, PLATFORM_POSIX)
        .err()
        .expect("unknown field rejected");
    assert_eq!(error.kind(), HarnessErrorKind::Corpus);
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn strict_loader_rejects_manifest_path_traversal() {
    let corpus = TempCorpus::copy();
    let path = corpus.0.join("manifest.json");
    let text = std::fs::read_to_string(&path).expect("manifest");
    std::fs::write(
        &path,
        text.replace(
            "state-dir.set.windows.json",
            "../state-dir.set.windows.json",
        ),
    )
    .expect("alter manifest");
    let error = load_corpus(&corpus.0, PLATFORM_POSIX)
        .err()
        .expect("traversal rejected");
    assert_eq!(error.kind(), HarnessErrorKind::Corpus);
    assert!(error.to_string().contains("invalid file name"));
}

#[cfg(unix)]
#[test]
fn strict_loader_rejects_a_symlinked_corpus_root() {
    use std::os::unix::fs::symlink;

    let corpus = TempCorpus::copy();
    let alias = corpus.0.with_extension("alias");
    symlink(&corpus.0, &alias).expect("create corpus alias");
    let error = load_corpus(&alias, PLATFORM_POSIX)
        .err()
        .expect("symlink root rejected");
    std::fs::remove_file(alias).expect("remove corpus alias");
    assert_eq!(error.kind(), HarnessErrorKind::Corpus);
    assert!(error.to_string().contains("may not be a symlink"));
}

#[test]
fn child_nonzero_exit_is_a_typed_harness_error() {
    let executable = std::env::current_exe().expect("test executable");
    let mut command = Command::new(executable);
    command.arg("--definitely-not-a-test-runner-option");
    let error = execute_bounded_child(&mut command, Duration::from_secs(2))
        .expect_err("invalid invocation exits nonzero");
    assert_eq!(error.kind(), HarnessErrorKind::ProbeExit);
}

#[test]
fn child_timeout_is_a_typed_harness_error() {
    let executable = std::env::current_exe().expect("test executable");
    let mut command = Command::new(executable);
    command.args(["--ignored", "--exact", "harness::tests::timeout_child"]);
    let error = execute_bounded_child(&mut command, Duration::from_millis(50))
        .expect_err("sleeping child times out");
    assert_eq!(error.kind(), HarnessErrorKind::ProbeTimeout);
}

#[test]
#[ignore = "executed only as a bounded child by child_timeout_is_a_typed_harness_error"]
fn timeout_child() {
    std::thread::sleep(Duration::from_secs(5));
    print!("late");
}

#[test]
fn probe_reports_a_nonempty_outcome() {
    assert!(!probe_state_dir_bytes().expect("probe bytes").is_empty());
}

#[test]
fn provider_turn_record_runs_turn_and_detach_cases() {
    let input = json!({
        "cases": [
            {
                "turn": {
                    "provider": {"kind": "fake"},
                    "messages": [{"type": "user_message", "content": "hello"}],
                    "tools": []
                }
            },
            {
                "detach": {
                    "value": {"status": "success", "output": 1, "summary": "s"},
                    "maxBytes": 1024,
                    "actor": "test.tool"
                }
            }
        ]
    });
    let record = provider_turn_record(&input).expect("record");
    assert_eq!(record["cases"][0]["turn"]["kind"], "turn");
    assert_eq!(
        record["cases"][0]["turn"]["assistantText"],
        "Siralos received: hello"
    );
    assert_eq!(record["cases"][1]["detach"]["ok"], true);
    assert_eq!(record["cases"][1]["detach"]["result"]["status"], "success");
}

#[test]
fn provider_turn_repeat_markers_materialize() {
    let input = json!({
        "cases": [
            {
                "turn": {
                    "provider": {"kind": "fake"},
                    "messages": [{
                        "type": "user_message",
                        "content": {"$repeat": {"character": "a", "count": 5}}
                    }],
                    "tools": []
                }
            }
        ]
    });
    let record = provider_turn_record(&input).expect("record");
    assert_eq!(
        record["cases"][0]["turn"]["assistantText"],
        "Siralos received: aaaaa"
    );
}

#[test]
fn provider_turn_failure_records_typed_category_and_message() {
    let input = json!({
        "cases": [
            {
                "turn": {
                    "provider": {"kind": "scripted", "events": [
                        {"type": "text_delta", "text": {"$repeat": {"character": "a", "count": 65537}}}
                    ]},
                    "messages": [{"type": "user_message", "content": "hello"}],
                    "tools": []
                }
            }
        ]
    });
    let record = provider_turn_record(&input).expect("record");
    let turn = &record["cases"][0]["turn"];
    assert_eq!(turn["kind"], "failed");
    assert_eq!(turn["failure"], "LIMIT_ASSISTANT_TEXT_BYTES");
    assert_eq!(
        turn["message"],
        "The provider exceeded the assistant-text byte limit limit; the response was rejected."
    );
}

#[test]
fn provider_turn_cancellation_script_produces_cancelled() {
    let input = json!({
        "cases": [
            {
                "turn": {
                    "provider": {"kind": "scripted", "events": [
                        {"type": "text_delta", "text": "one"},
                        {"type": "completed"}
                    ]},
                    "messages": [{"type": "user_message", "content": "hello"}],
                    "tools": [],
                    "cancelAfterEvents": 1
                }
            }
        ]
    });
    let record = provider_turn_record(&input).expect("record");
    assert_eq!(record["cases"][0]["turn"]["kind"], "cancelled");
}

#[test]
fn provider_turn_input_validation_is_strict() {
    let input = json!({ "cases": [], "extra": true });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {"provider": {"kind": "fake"}, "messages": [], "tools": []},
            "detach": {"value": 1, "maxBytes": 10, "actor": "a"}
        }]
    });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {
                "provider": {"kind": "fake"},
                "messages": [],
                "tools": [],
                "unexpected": 1
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {
                "provider": {"kind": "openai"},
                "messages": [],
                "tools": []
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {
                "provider": {"kind": "scripted"},
                "messages": [],
                "tools": []
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {
                "provider": {"kind": "fake"},
                "messages": [{"type": "system_message", "content": "x"}],
                "tools": []
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_err());
    let input = json!({
        "cases": [{
            "turn": {
                "provider": {"kind": "fake"},
                "messages": [{"type": "user_message", "content": "hello"}],
                "tools": []
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_ok());
    let input = json!({
        "cases": [{
            "detach": {
                "value": {"status": "failed", "message": "boom"},
                "maxBytes": 128,
                "actor": "test.tool"
            }
        }]
    });
    assert!(validate_provider_turn_input(&input).is_ok());
}

#[test]
fn provider_turn_scripted_raw_events_use_the_validation_seam() {
    let input = json!({
        "cases": [
            {
                "turn": {
                    "provider": {"kind": "scripted", "events": [
                        {"type": "unexpected", "callId": "call-1", "toolName": "workspace.read", "input": {"path": "README.md"}}
                    ]},
                    "messages": [{"type": "user_message", "content": "hello"}],
                    "tools": []
                }
            }
        ]
    });
    let record = provider_turn_record(&input).expect("record");
    let turn = &record["cases"][0]["turn"];
    assert_eq!(turn["kind"], "failed");
    assert_eq!(turn["failure"], "UNKNOWN_EVENT_TYPE");
    assert_eq!(
        turn["message"],
        "The provider emitted an unknown event type; the response was rejected."
    );
}

#[test]
fn workspace_version_matches_the_workspace_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(
        cargo_workspace_version(&root).expect("workspace version"),
        "0.0.0"
    );
}
