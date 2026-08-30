//! The Skill seam (Stage 5.6, decision 52): reusable declarative
//! guidance for model reasoning per ADR 0036 §22.
//!
//! A Skill has **no authority** — it cannot grant filesystem access,
//! network access, process execution, credentials, or host mutation
//! ("Skill != Capability"). Resolution binds digest-bound references to
//! declared catalog entries only; the evidence digest literally binds
//! `authority = none`. Skills are opt-in: absent selection binds
//! nothing. Resolution is deterministic and order-independent.

use std::collections::{BTreeMap, BTreeSet};

use crate::identity::{CanonicalValue, compute_artifact_digest};

/// Maximum number of skills in one catalog.
pub const MAX_SKILL_CATALOG: usize = 32;
/// Maximum skill name length in UTF-8 bytes.
pub const MAX_SKILL_NAME_BYTES: usize = 64;
/// Maximum skill content length in UTF-8 bytes.
pub const MAX_SKILL_CONTENT_BYTES: usize = 64 * 1024;

/// A typed validation failure for malformed skill input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillValidationError {
    /// Deterministic, human-readable reason.
    pub message: String,
}

fn failure(message: impl Into<String>) -> SkillValidationError {
    SkillValidationError { message: message.into() }
}

/// One declarative skill: a bounded name plus guidance content,
/// bound by a content digest over the artifact-digest primitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    /// The skill name (unique within a catalog).
    pub name: String,
    /// The guidance content.
    pub content: String,
    /// The content digest binding name + content.
    pub digest: String,
}

impl SkillDefinition {
    /// Validate and digest-bind one skill definition.
    ///
    /// # Errors
    ///
    /// Returns [`SkillValidationError`] for empty/oversized names,
    /// empty/oversized content, NUL bytes, or digest-primitive failure.
    pub fn new(
        name: &str,
        content: &str,
    ) -> Result<Self, SkillValidationError> {
        if name.is_empty() || name.len() > MAX_SKILL_NAME_BYTES {
            return Err(failure(format!(
                "A skill name must be 1..={MAX_SKILL_NAME_BYTES} bytes."
            )));
        }
        if name.contains('\0') {
            return Err(failure(
                "A skill name must not contain NUL.".to_owned(),
            ));
        }
        if content.is_empty() || content.len() > MAX_SKILL_CONTENT_BYTES {
            return Err(failure(format!(
                "Skill {name:?} content must be 1..={MAX_SKILL_CONTENT_BYTES} bytes."
            )));
        }
        let payload = CanonicalValue::Object(BTreeMap::from([
            ("content".to_owned(), CanonicalValue::Str(content.to_owned())),
            ("name".to_owned(), CanonicalValue::Str(name.to_owned())),
        ]));
        let digest = compute_artifact_digest("SkillDefinition", 1, &payload)
            .map_err(|error| failure(error.message))?
            .value;
        Ok(Self { name: name.to_owned(), content: content.to_owned(), digest })
    }
}

/// The bounded, deterministically ordered declared skill set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCatalog {
    /// Sorted by name.
    pub skills: Vec<SkillDefinition>,
}

impl SkillCatalog {
    /// Validate and build a catalog; order-independent (sorted).
    ///
    /// # Errors
    ///
    /// Returns [`SkillValidationError`] for count overflow, duplicate
    /// names, or invalid definitions.
    pub fn new(
        skills: Vec<SkillDefinition>,
    ) -> Result<Self, SkillValidationError> {
        if skills.len() > MAX_SKILL_CATALOG {
            return Err(failure(format!(
                "The skill catalog exceeds the {MAX_SKILL_CATALOG}-skill bound."
            )));
        }
        let mut seen = BTreeSet::new();
        for skill in &skills {
            if skill.name.is_empty() || !seen.insert(skill.name.as_str()) {
                return Err(failure(format!(
                    "The skill catalog declares name {:?} more than once.",
                    skill.name
                )));
            }
        }
        let mut sorted = skills;
        sorted.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self { skills: sorted })
    }

    /// Look up one skill by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.iter().find(|skill| skill.name == name)
    }
}

/// One digest-bound reference handed to the model: guidance identity
/// only, never capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillReference {
    /// The bound content digest.
    pub digest: String,
    /// The skill name.
    pub name: String,
}

/// The result of applying an opt-in selection to the declared catalog:
/// `bound` is a subset of the catalog (intersection), `unknown` names
/// selections the workspace does not declare (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResolution {
    /// Sorted references that bind.
    pub bound: Vec<SkillReference>,
    /// Sorted selected names outside the catalog.
    pub unknown: Vec<String>,
}

impl SkillResolution {
    /// Typed disposition: `none` or `bound`.
    pub fn disposition(&self) -> &'static str {
        if self.bound.is_empty() { "none" } else { "bound" }
    }
}

/// Resolve an opt-in selection against the declared catalog. `selected`
/// is the profile/session list (`None` = nothing selected; skills are
/// opt-in, so nothing binds by default).
#[must_use]
pub fn resolve_profile_skills(
    catalog: &SkillCatalog,
    selected: Option<&[String]>,
) -> SkillResolution {
    let Some(selected) = selected else {
        return SkillResolution { bound: Vec::new(), unknown: Vec::new() };
    };
    let selected_set: BTreeSet<&String> = selected.iter().collect();
    let mut bound = Vec::new();
    let mut unknown = Vec::new();
    for name in &selected_set {
        match catalog.get(name) {
            Some(skill) => bound.push(SkillReference {
                digest: skill.digest.clone(),
                name: skill.name.clone(),
            }),
            None => unknown.push((*name).clone()),
        }
    }
    bound.sort_by(|left, right| left.name.cmp(&right.name));
    SkillResolution { bound, unknown }
}

/// Digest-bound evidence for one skill resolution. The payload binds
/// `authority = none`: the digest is only valid for a resolution that
/// grants no capability at all ("Skill != Capability").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResolutionEvidence {
    /// Sorted references that bind.
    pub bound: Vec<SkillReference>,
    /// Typed disposition (`none` or `bound`).
    pub disposition: String,
    /// The bound resolution digest (includes the no-authority term).
    pub resolution_digest: String,
    /// Sorted selected names outside the catalog.
    pub unknown: Vec<String>,
}

/// Create digest-bound evidence for a skill resolution.
///
/// # Errors
///
/// Returns [`SkillValidationError`] when the digest primitive fails.
pub fn create_skill_resolution_evidence(
    resolution: &SkillResolution,
) -> Result<SkillResolutionEvidence, SkillValidationError> {
    let bound: Vec<CanonicalValue> = resolution
        .bound
        .iter()
        .map(|reference| {
            CanonicalValue::Object(BTreeMap::from([
                (
                    "digest".to_owned(),
                    CanonicalValue::Str(reference.digest.clone()),
                ),
                (
                    "name".to_owned(),
                    CanonicalValue::Str(reference.name.clone()),
                ),
            ]))
        })
        .collect();
    let unknown: Vec<CanonicalValue> = resolution
        .unknown
        .iter()
        .map(|name| CanonicalValue::Str(name.clone()))
        .collect();
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("authority".to_owned(), CanonicalValue::Str("none".to_owned())),
        ("bound".to_owned(), CanonicalValue::Array(bound)),
        (
            "disposition".to_owned(),
            CanonicalValue::Str(resolution.disposition().to_owned()),
        ),
        ("unknown".to_owned(), CanonicalValue::Array(unknown)),
    ]));
    let resolution_digest =
        compute_artifact_digest("SkillResolutionEvidence", 1, &payload)
            .map_err(|error| failure(error.message))?
            .value;
    Ok(SkillResolutionEvidence {
        bound: resolution.bound.clone(),
        disposition: resolution.disposition().to_owned(),
        resolution_digest,
        unknown: resolution.unknown.clone(),
    })
}

/// Deterministic report-safe rendering of skill-resolution evidence.
#[must_use]
pub fn render_skill_resolution_evidence(
    evidence: &SkillResolutionEvidence,
) -> String {
    match evidence.disposition.as_str() {
        "none" => "none skills=0".to_owned(),
        _ => {
            let base = format!(
                "bound skills={} (guidance only)",
                evidence.bound.len()
            );
            if evidence.unknown.is_empty() {
                base
            } else {
                format!("{base} unknown={}", evidence.unknown.len())
            }
        }
    }
}

#[cfg(test)]
mod skills_tests {
    use super::{
        MAX_SKILL_NAME_BYTES, SkillCatalog, SkillDefinition,
        create_skill_resolution_evidence, render_skill_resolution_evidence,
        resolve_profile_skills,
    };

    fn skill(name: &str) -> SkillDefinition {
        SkillDefinition::new(name, &format!("guidance for {name}"))
            .expect("skill")
    }

    #[test]
    fn resolution_is_opt_in_intersecting_and_deterministic() {
        let catalog = SkillCatalog::new(vec![
            skill("zeta"),
            skill("alpha"),
            skill("guest"),
        ])
        .expect("catalog");
        let selection = resolve_profile_skills(
            &catalog,
            Some(&["guest".to_owned(), "ghost".to_owned()]),
        );
        assert_eq!(selection.disposition(), "bound");
        assert_eq!(selection.bound.len(), 1);
        assert_eq!(selection.bound[0].name, "guest");
        assert_eq!(selection.unknown, vec!["ghost".to_owned()]);
        let none = resolve_profile_skills(&catalog, None);
        assert_eq!(none.disposition(), "none");
        assert!(none.bound.is_empty());
        let empty = resolve_profile_skills(&catalog, Some(&[]));
        assert_eq!(empty.disposition(), "none");
        let evidence =
            create_skill_resolution_evidence(&selection).expect("evidence");
        assert_eq!(
            render_skill_resolution_evidence(&evidence),
            "bound skills=1 (guidance only) unknown=1"
        );
        let plain = create_skill_resolution_evidence(&resolve_profile_skills(
            &catalog,
            Some(&["alpha".to_owned()]),
        ))
        .expect("evidence");
        assert_eq!(
            render_skill_resolution_evidence(&plain),
            "bound skills=1 (guidance only)"
        );
    }

    #[test]
    fn catalog_is_order_independent_and_bounded() {
        let first = SkillCatalog::new(vec![skill("zeta"), skill("alpha")])
            .expect("catalog");
        let second = SkillCatalog::new(vec![skill("alpha"), skill("zeta")])
            .expect("catalog");
        assert_eq!(first, second);
        let selection_a = resolve_profile_skills(
            &first,
            Some(&["zeta".to_owned(), "alpha".to_owned()]),
        );
        let selection_b = resolve_profile_skills(
            &second,
            Some(&["alpha".to_owned(), "zeta".to_owned()]),
        );
        let evidence_a =
            create_skill_resolution_evidence(&selection_a).expect("evidence");
        let evidence_b =
            create_skill_resolution_evidence(&selection_b).expect("evidence");
        assert_eq!(evidence_a.resolution_digest, evidence_b.resolution_digest);
        let duplicate = SkillCatalog::new(vec![skill("dup"), skill("dup")]);
        let error = duplicate.expect_err("duplicate refused");
        assert!(error.message.contains("more than once"));
        let error = SkillDefinition::new(
            &"x".repeat(MAX_SKILL_NAME_BYTES + 1),
            "content",
        );
        assert!(
            error.expect_err("oversize refused").message.contains("64 bytes")
        );
    }
}
