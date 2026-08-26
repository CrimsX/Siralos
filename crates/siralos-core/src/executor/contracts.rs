//! Versioned Siralos-owned execution contract and validation profile
//! reference (executor briefing foundation, ADR 0022).
//!
//! The execution contract carries the permanent, repeatedly-applied rules
//! for implementation tasks and references where each rule is actually
//! enforced. It is immutable and revisioned like TaskContract, grants
//! nothing, and never carries milestone-specific requirements.

use serde_json::json;

use crate::identity::{canonical_json_value, sha256_hex};

/// Stable reference to one immutable validation-profile revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationProfileRef {
    /// Host-owned profile id.
    pub profile_id: String,
    /// Immutable profile revision.
    pub revision: u64,
}

/// Hard bounds for the execution contract.
pub struct ExecutionContractLimits;

impl ExecutionContractLimits {
    /// Maximum id bytes.
    pub const MAX_ID_BYTES: usize = 64;
    /// Maximum rules per group.
    pub const MAX_RULES_PER_GROUP: usize = 24;
    /// Maximum rule-id bytes.
    pub const MAX_RULE_ID_BYTES: usize = 64;
    /// Maximum requirement bytes.
    pub const MAX_REQUIREMENT_BYTES: usize = 512;
    /// Maximum enforced-by bytes.
    pub const MAX_ENFORCED_BY_BYTES: usize = 256;
    /// Maximum reporting requirements.
    pub const MAX_REPORTING_REQUIREMENTS: usize = 12;
    /// Maximum reporting-requirement bytes.
    pub const MAX_REPORTING_REQUIREMENT_BYTES: usize = 512;
}

/// Execution rule kind (also the rule group).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionRuleKind {
    /// Git discipline.
    Git,
    /// Security posture.
    Security,
    /// Architecture boundaries.
    Architecture,
    /// Testing rules.
    Test,
    /// Process expectations.
    Process,
}

impl ExecutionRuleKind {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionRuleKind::Git => "git",
            ExecutionRuleKind::Security => "security",
            ExecutionRuleKind::Architecture => "architecture",
            ExecutionRuleKind::Test => "test",
            ExecutionRuleKind::Process => "process",
        }
    }
}

/// One permanent execution rule; `enforced_by` names the authoritative
/// enforcement mechanism instead of restating enforcement prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRule {
    /// Stable rule id (doubles as the acceptance id of the permanent rule).
    pub id: String,
    /// Rule kind/group.
    pub kind: ExecutionRuleKind,
    /// Concise machine-readable permanent requirement.
    pub requirement: String,
    /// Where the requirement is actually enforced.
    pub enforced_by: String,
}

/// One reporting requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportingRequirement {
    /// Stable id.
    pub id: String,
    /// Bounded requirement text.
    pub requirement: String,
}

/// Immutable revisioned execution contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContract {
    /// Stable contract id.
    pub id: String,
    /// Immutable revision identity; starts at 1 and only increases.
    pub revision: u64,
    /// Git rules.
    pub git_rules: Vec<ExecutionRule>,
    /// Security rules.
    pub security_rules: Vec<ExecutionRule>,
    /// Architecture rules.
    pub architecture_rules: Vec<ExecutionRule>,
    /// Referenced validation profile.
    pub validation_profile: ValidationProfileRef,
    /// Test rules.
    pub test_rules: Vec<ExecutionRule>,
    /// Reporting requirements.
    pub reporting_requirements: Vec<ReportingRequirement>,
}

/// Stable reference to one immutable contract revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContractRef {
    /// Contract id.
    pub id: String,
    /// Contract revision.
    pub revision: u64,
}

fn is_contract_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn is_rule_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_uppercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn is_reporting_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn copy_rules(
    kind: ExecutionRuleKind,
    kind_label: &str,
    rules: &[ExecutionRule],
) -> Result<Vec<ExecutionRule>, String> {
    if rules.len() > ExecutionContractLimits::MAX_RULES_PER_GROUP {
        return Err(format!(
            "The {kind_label} rule group accepts at most {} rules.",
            ExecutionContractLimits::MAX_RULES_PER_GROUP
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for rule in rules {
        if !is_rule_id(&rule.id) {
            return Err(format!("Invalid execution rule id: {}", rule.id));
        }
        if rule.kind != kind {
            return Err(format!(
                "Execution rule {} declares kind {} but belongs to the {} group.",
                rule.id,
                rule.kind.as_str(),
                kind_label
            ));
        }
        let requirement = rule.requirement.trim();
        if requirement.is_empty() {
            return Err(format!(
                "Execution rule {} requires a non-empty requirement.",
                rule.id
            ));
        }
        if requirement.len() > ExecutionContractLimits::MAX_REQUIREMENT_BYTES {
            return Err(format!(
                "Execution rule {} exceeds {} UTF-8 bytes.",
                rule.id,
                ExecutionContractLimits::MAX_REQUIREMENT_BYTES
            ));
        }
        let enforced_by = rule.enforced_by.trim();
        if enforced_by.is_empty() {
            return Err(format!(
                "Execution rule {} requires a non-empty enforcedBy reference.",
                rule.id
            ));
        }
        if enforced_by.len() > ExecutionContractLimits::MAX_ENFORCED_BY_BYTES {
            return Err(format!(
                "Execution rule {} exceeds {} UTF-8 bytes for enforcedBy.",
                rule.id,
                ExecutionContractLimits::MAX_ENFORCED_BY_BYTES
            ));
        }
        if seen.contains(&rule.id.as_str()) {
            return Err(format!("Duplicate execution rule id: {}", rule.id));
        }
        seen.push(rule.id.as_str());
    }
    Ok(rules
        .iter()
        .map(|rule| ExecutionRule {
            id: rule.id.clone(),
            kind: rule.kind,
            requirement: rule.requirement.trim().to_owned(),
            enforced_by: rule.enforced_by.trim().to_owned(),
        })
        .collect())
}

fn validate_reporting_requirements(
    requirements: &[ReportingRequirement],
) -> Result<Vec<ReportingRequirement>, String> {
    if requirements.len() > ExecutionContractLimits::MAX_REPORTING_REQUIREMENTS
    {
        return Err(format!(
            "An execution contract accepts at most {} reporting requirements.",
            ExecutionContractLimits::MAX_REPORTING_REQUIREMENTS
        ));
    }
    let mut ids: Vec<&str> = Vec::new();
    for requirement in requirements {
        if !is_reporting_id(&requirement.id) {
            return Err(format!(
                "Invalid reporting requirement id: {}",
                requirement.id
            ));
        }
        if ids.contains(&requirement.id.as_str()) {
            return Err(format!(
                "Duplicate reporting requirement id: {}",
                requirement.id
            ));
        }
        ids.push(requirement.id.as_str());
        let text = requirement.requirement.trim();
        if text.is_empty() {
            return Err(format!(
                "Reporting requirement {} requires text.",
                requirement.id
            ));
        }
        if text.len()
            > ExecutionContractLimits::MAX_REPORTING_REQUIREMENT_BYTES
        {
            return Err(format!(
                "Reporting requirement {} exceeds {} UTF-8 bytes.",
                requirement.id,
                ExecutionContractLimits::MAX_REPORTING_REQUIREMENT_BYTES
            ));
        }
    }
    Ok(requirements
        .iter()
        .map(|requirement| ReportingRequirement {
            id: requirement.id.clone(),
            requirement: requirement.requirement.trim().to_owned(),
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn validate_contract_shape(
    id: &str,
    revision: u64,
    git_rules: &[ExecutionRule],
    security_rules: &[ExecutionRule],
    architecture_rules: &[ExecutionRule],
    validation_profile: &ValidationProfileRef,
    test_rules: &[ExecutionRule],
    reporting_requirements: &[ReportingRequirement],
) -> Result<ExecutionContract, String> {
    if !is_contract_id(id) {
        return Err(format!("Invalid execution contract id: {id}"));
    }
    if revision < 1 {
        return Err(
            "An execution contract revision must be at least 1.".to_owned()
        );
    }
    if validation_profile.revision < 1 {
        return Err(
            "A validation profile revision must be at least 1.".to_owned()
        );
    }
    if validation_profile.profile_id.trim().is_empty() {
        return Err("A validation profile requires a profile id.".to_owned());
    }
    Ok(ExecutionContract {
        id: id.to_owned(),
        revision,
        git_rules: copy_rules(ExecutionRuleKind::Git, "git", git_rules)?,
        security_rules: copy_rules(
            ExecutionRuleKind::Security,
            "security",
            security_rules,
        )?,
        architecture_rules: copy_rules(
            ExecutionRuleKind::Architecture,
            "architecture",
            architecture_rules,
        )?,
        validation_profile: ValidationProfileRef {
            profile_id: validation_profile.profile_id.trim().to_owned(),
            revision: validation_profile.revision,
        },
        test_rules: copy_rules(ExecutionRuleKind::Test, "test", test_rules)?,
        reporting_requirements: validate_reporting_requirements(
            reporting_requirements,
        )?,
    })
}

/// Validate and detach a contract at a runtime boundary.
///
/// # Errors
///
/// Exact reference messages.
pub fn validate_execution_contract(
    input: &ExecutionContract,
) -> Result<ExecutionContract, String> {
    validate_contract_shape(
        &input.id,
        input.revision,
        &input.git_rules,
        &input.security_rules,
        &input.architecture_rules,
        &input.validation_profile,
        &input.test_rules,
        &input.reporting_requirements,
    )
}

/// Input to [`create_execution_contract`].
pub struct CreateExecutionContractInput {
    /// Stable contract id.
    pub id: String,
    /// Git rules.
    pub git_rules: Vec<ExecutionRule>,
    /// Security rules.
    pub security_rules: Vec<ExecutionRule>,
    /// Architecture rules.
    pub architecture_rules: Vec<ExecutionRule>,
    /// Validation profile reference.
    pub validation_profile: ValidationProfileRef,
    /// Test rules.
    pub test_rules: Vec<ExecutionRule>,
    /// Reporting requirements.
    pub reporting_requirements: Vec<ReportingRequirement>,
}

/// Create the first immutable contract revision.
///
/// # Errors
///
/// Exact reference messages.
pub fn create_execution_contract(
    input: CreateExecutionContractInput,
) -> Result<ExecutionContract, String> {
    validate_contract_shape(
        &input.id,
        1,
        &input.git_rules,
        &input.security_rules,
        &input.architecture_rules,
        &input.validation_profile,
        &input.test_rules,
        &input.reporting_requirements,
    )
}

/// Optional per-group overrides for [`revise_execution_contract`].
#[derive(Default)]
pub struct ReviseExecutionContractInput<'a> {
    /// Replacement git rules.
    pub git_rules: Option<&'a [ExecutionRule]>,
    /// Replacement security rules.
    pub security_rules: Option<&'a [ExecutionRule]>,
    /// Replacement architecture rules.
    pub architecture_rules: Option<&'a [ExecutionRule]>,
    /// Replacement validation profile.
    pub validation_profile: Option<&'a ValidationProfileRef>,
    /// Replacement test rules.
    pub test_rules: Option<&'a [ExecutionRule]>,
    /// Replacement reporting requirements.
    pub reporting_requirements: Option<&'a [ReportingRequirement]>,
}

/// Produce the next immutable contract revision; the previous object is
/// untouched.
///
/// # Errors
///
/// Exact reference messages.
pub fn revise_execution_contract(
    previous: &ExecutionContract,
    changes: &ReviseExecutionContractInput<'_>,
) -> Result<ExecutionContract, String> {
    if previous.revision < 1 || previous.revision >= u64::MAX - 1 {
        return Err(
            "A previous execution contract revision must be an incrementable safe integer."
                .to_owned(),
        );
    }
    validate_contract_shape(
        &previous.id,
        previous.revision + 1,
        changes.git_rules.unwrap_or(&previous.git_rules),
        changes.security_rules.unwrap_or(&previous.security_rules),
        changes.architecture_rules.unwrap_or(&previous.architecture_rules),
        changes.validation_profile.unwrap_or(&previous.validation_profile),
        changes.test_rules.unwrap_or(&previous.test_rules),
        changes
            .reporting_requirements
            .unwrap_or(&previous.reporting_requirements),
    )
}

fn contract_value(contract: &ExecutionContract) -> serde_json::Value {
    let rule = |rule: &ExecutionRule| {
        json!({
            "id": rule.id,
            "kind": rule.kind.as_str(),
            "requirement": rule.requirement,
            "enforcedBy": rule.enforced_by,
        })
    };
    json!({
        "id": contract.id,
        "revision": contract.revision,
        "gitRules": contract.git_rules.iter().map(rule).collect::<Vec<_>>(),
        "securityRules": contract.security_rules.iter().map(rule).collect::<Vec<_>>(),
        "architectureRules": contract.architecture_rules.iter().map(rule).collect::<Vec<_>>(),
        "validationProfile": {
            "profileId": contract.validation_profile.profile_id,
            "revision": contract.validation_profile.revision,
        },
        "testRules": contract.test_rules.iter().map(rule).collect::<Vec<_>>(),
        "reportingRequirements": contract.reporting_requirements.iter()
            .map(|requirement| json!({
                "id": requirement.id,
                "requirement": requirement.requirement,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Deterministic digest over a contract revision (canonical JSON).
pub fn compute_execution_contract_digest(
    contract: &ExecutionContract,
) -> String {
    sha256_hex(canonical_json_value(&contract_value(contract)).as_bytes())
}

/// Stable reference to one immutable contract revision.
pub fn execution_contract_ref(
    contract: &ExecutionContract,
) -> ExecutionContractRef {
    ExecutionContractRef {
        id: contract.id.clone(),
        revision: contract.revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_digest_matches_the_reference_canonical_form() {
        // Digest captured by executing the TypeScript reference
        // executionContractFixture (R13.4 differential fixture parity).
        let first = create_execution_contract(CreateExecutionContractInput {
            id: "siralos-execution-contract".to_owned(),
            validation_profile: ValidationProfileRef {
                profile_id: "standard-repo-validation".to_owned(),
                revision: 1,
            },
            git_rules: vec![
                ExecutionRule {
                    id: "CORE.GIT.NO_PUSH".to_owned(),
                    kind: ExecutionRuleKind::Git,
                    requirement: "Never push or rewrite history.".to_owned(),
                    enforced_by: "AGENTS.md Git discipline".to_owned(),
                },
                ExecutionRule {
                    id: "CORE.GIT.LOGICAL_COMMITS".to_owned(),
                    kind: ExecutionRuleKind::Git,
                    requirement: "Use small logical commits.".to_owned(),
                    enforced_by: "AGENTS.md Verification section".to_owned(),
                },
            ],
            security_rules: vec![ExecutionRule {
                id: "CORE.SECURITY.UNTRUSTED_OUTPUT".to_owned(),
                kind: ExecutionRuleKind::Security,
                requirement: "Provider output is untrusted data.".to_owned(),
                enforced_by: "Provider protocol and terminal sanitizer"
                    .to_owned(),
            }],
            architecture_rules: Vec::new(),
            test_rules: vec![ExecutionRule {
                id: "CORE.TEST.STANDARD_VALIDATION".to_owned(),
                kind: ExecutionRuleKind::Test,
                requirement:
                    "Apply the standard validation profile before handoff."
                        .to_owned(),
                enforced_by: "STANDARD_REPO_VALIDATION profile".to_owned(),
            }],
            reporting_requirements: vec![ReportingRequirement {
                id: "REPORT.MACHINE_KNOWN".to_owned(),
                requirement: "Report machine-known facts from host evidence."
                    .to_owned(),
            }],
        })
        .expect("valid contract");
        assert_eq!(
            compute_execution_contract_digest(&first),
            "596d8796eb70b0f96a7a98bc953561e0c7b705d4d5a4c433931b19d5ef1e825d"
        );
    }
}
