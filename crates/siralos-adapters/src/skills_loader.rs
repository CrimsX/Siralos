//! The workspace skill catalog adapter (Stage 5.6, decision 52).
//!
//! Read-only loading of declarative skill files from
//! `.siralos/skills/*.md`: each file is one skill (the file stem is the
//! name, the body is the guidance content). The listing is
//! deterministic (sorted by file name), every file is lstat-verified
//! as a regular file and byte-bounded before parsing, and the loaded
//! set is validated as a [`SkillCatalog`]. No writes, no registry.

use std::path::{Path, PathBuf};

use siralos_core::skills::{MAX_SKILL_CATALOG, SkillCatalog, SkillDefinition};

/// Maximum number of skill files in one workspace catalog.
pub const MAX_SKILL_FILES: usize = MAX_SKILL_CATALOG;

/// A typed skill-catalog loading failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoadFailure {
    /// Bounded truthful message.
    pub message: String,
}

fn failure(message: impl Into<String>) -> SkillLoadFailure {
    SkillLoadFailure { message: message.into() }
}

/// The loading outcome: a validated catalog, or a typed absent state
/// when the workspace declares no skills directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillCatalogLoad {
    /// The workspace declares no `.siralos/skills` directory.
    Absent,
    /// The validated, sorted catalog.
    Catalog(SkillCatalog),
}

/// Load and validate the workspace skill catalog.
///
/// # Errors
///
/// Returns [`SkillLoadFailure`] for unreadable listings, non-regular
/// files, oversized content, invalid UTF-8, malformed names, and count
/// overflow.
pub fn load_workspace_skills(
    root: &Path,
) -> Result<SkillCatalogLoad, SkillLoadFailure> {
    let dir = root.join(".siralos").join("skills");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillCatalogLoad::Absent);
        }
        Err(error) => {
            return Err(failure(format!(
                ".siralos/skills is unreadable: {error}"
            )));
        }
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            failure(format!(".siralos/skills is unreadable: {error}"))
        })?;
        paths.push(entry.path());
    }
    paths.sort();
    let mut skills = Vec::new();
    for path in paths {
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        else {
            return Err(failure(
                "A skill file name must be valid UTF-8.".to_owned(),
            ));
        };
        let Some(raw_name) = path.file_name().and_then(|name| name.to_str())
        else {
            return Err(failure(
                "A skill file name must be valid UTF-8.".to_owned(),
            ));
        };
        if !raw_name.ends_with(".md") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            failure(format!("skill file {name:?} is unreadable: {error}"))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(failure(format!(
                "skill file {name:?} must be a regular file; refusing symlink or special file"
            )));
        }
        if skills.len() >= MAX_SKILL_FILES {
            return Err(failure(format!(
                "The skill catalog exceeds the {MAX_SKILL_FILES}-skill bound."
            )));
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            failure(format!("skill file {name:?} is unreadable: {error}"))
        })?;
        if bytes.len() > siralos_core::skills::MAX_SKILL_CONTENT_BYTES {
            return Err(failure(format!(
                "skill file {name:?} exceeds the {}-byte bound.",
                siralos_core::skills::MAX_SKILL_CONTENT_BYTES
            )));
        }
        let content = String::from_utf8(bytes).map_err(|_| {
            failure(format!("skill file {name:?} is not valid UTF-8."))
        })?;
        skills.push(
            SkillDefinition::new(name, &content)
                .map_err(|error| failure(error.message))?,
        );
    }
    let catalog =
        SkillCatalog::new(skills).map_err(|error| failure(error.message))?;
    Ok(SkillCatalogLoad::Catalog(catalog))
}

#[cfg(test)]
mod skills_loader_tests {
    use super::{SkillCatalogLoad, load_workspace_skills};

    fn workspace() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("siralos-skills-tests-{nonce}"));
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn skills_dir(root: &std::path::Path) -> std::path::PathBuf {
        let dir = root.join(".siralos").join("skills");
        std::fs::create_dir_all(&dir).expect("skills dir");
        dir
    }

    #[test]
    fn absent_then_sorted_load_with_bound_digests() {
        let root = workspace();
        assert_eq!(
            load_workspace_skills(&root).expect("load"),
            SkillCatalogLoad::Absent,
        );
        let dir = skills_dir(&root);
        std::fs::write(dir.join("zeta.md"), "guidance for zeta")
            .expect("write");
        std::fs::write(dir.join("alpha.md"), "guidance for alpha")
            .expect("write");
        std::fs::write(dir.join("notes.txt"), "ignored").expect("write");
        let SkillCatalogLoad::Catalog(catalog) =
            load_workspace_skills(&root).expect("load")
        else {
            panic!("expected a catalog");
        };
        assert_eq!(catalog.skills.len(), 2);
        assert_eq!(catalog.skills[0].name, "alpha");
        assert_eq!(catalog.skills[1].name, "zeta");
        let direct = siralos_core::skills::SkillDefinition::new(
            "alpha",
            "guidance for alpha",
        )
        .expect("skill");
        assert_eq!(catalog.skills[0].digest, direct.digest);
    }

    #[test]
    fn oversize_content_is_typed_invalid() {
        let root = workspace();
        let content =
            "x".repeat(siralos_core::skills::MAX_SKILL_CONTENT_BYTES + 1);
        std::fs::write(skills_dir(&root).join("big.md"), content)
            .expect("write");
        let error = load_workspace_skills(&root).expect_err("refused");
        assert!(error.message.contains("exceeds the"));
    }
}
