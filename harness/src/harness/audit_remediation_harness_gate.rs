use siralos_adapters::profile_config::WorkspaceProfileLoad;
use siralos_core::composition::{compose_effective_policy, declare_profile};
use siralos_core::tool::{
    CapabilityId, PermissionPolicy, PermissionRule, PolicyRule,
};
use std::fs::write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce =
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir()
        .join(format!("siralos-harness-gate-{label}-{nonce}"));
    std::fs::create_dir(&p).expect("temp");
    p
}

#[test]
fn refused_profile_leaves_subsystem_off_in_harness_record() {
    let host_rules = vec![PolicyRule {
        capability: CapabilityId::parse("workspace.read").expect("cap"),
        rule: PermissionRule::Allow,
    }];
    let record = siralos_core::composition::ProfileRecord {
        name: "dev".to_owned(),
        overlay: vec![],
        plugins: None,
        context: None,
        context_system_enabled: true,
        provider: None,
        model: None,
        credential: None,
        endpoint: None,
        skills: None,
        record_replay: false,
        replay: false,
        protocol: siralos_core::composition::Protocol::default(),
        model_display_name: None,
    };
    let declared_invalid =
        siralos_core::composition::DeclaredProfile::Invalid {
            diagnostic: "refused".to_owned(),
        };
    let effective_invalid =
        compose_effective_policy(&host_rules, &declared_invalid);
    assert!(effective_invalid.applied_profile.is_none());
    let enabled_invalid = if effective_invalid.applied_profile.is_some() {
        record.context_system_enabled
    } else {
        false
    };
    assert!(!enabled_invalid, "refused profile must gate enabled false");
    let declared_valid = declare_profile(
        Some(&record),
        &PermissionPolicy::from_rules(host_rules.clone()),
    );
    let effective_valid =
        compose_effective_policy(&host_rules, &declared_valid);
    assert!(effective_valid.applied_profile.is_some());
    let enabled_valid = if effective_valid.applied_profile.is_some() {
        record.context_system_enabled
    } else {
        false
    };
    assert!(enabled_valid);
    let root = temp_dir("refused");
    write(root.join("siralos.toml"), "[profile]\nname = \"dev\"\nplugins = \"not-an-array\"\n[profile.context_system]\nenabled = true\n").expect("write toml");
    let loaded =
        siralos_adapters::profile_config::load_workspace_profile(&root);
    assert!(matches!(loaded, WorkspaceProfileLoad::Invalid { .. }));
    let declared2 = match &loaded {
        WorkspaceProfileLoad::Record(r) => declare_profile(
            Some(r),
            &PermissionPolicy::from_rules(host_rules.clone()),
        ),
        WorkspaceProfileLoad::Absent => {
            siralos_core::composition::DeclaredProfile::Absent
        }
        WorkspaceProfileLoad::Invalid { diagnostic } => {
            siralos_core::composition::DeclaredProfile::Invalid {
                diagnostic: diagnostic.clone(),
            }
        }
    };
    let effective2 = compose_effective_policy(&host_rules, &declared2);
    let enabled2 = if effective2.applied_profile.is_some() {
        match &loaded {
            WorkspaceProfileLoad::Record(r) => r.context_system_enabled,
            _ => false,
        }
    } else {
        false
    };
    assert!(!enabled2, "malformed profile must keep harness record OFF");
    let _ = std::fs::remove_dir_all(&root);
}
