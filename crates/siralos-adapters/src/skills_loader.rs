//! The workspace skill catalog adapter (Stage 5.6, decision 52).
//!
//! Read-only loading of declarative skill files from
//! `.siralos/skills/*.md`: each file is one skill (the file stem is the
//! name, the body is the guidance content). The listing is
//! deterministic (sorted by file name), bounded during directory
//! iteration, and every file is opened without following a link and read
//! as a bounded complete stream before parsing. The loaded set is
//! validated as a [`SkillCatalog`]. No writes, no registry.

use std::fs::{Metadata, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use siralos_core::skills::{MAX_SKILL_CATALOG, SkillCatalog, SkillDefinition};

/// Maximum number of directory entries considered in one workspace
/// catalog. The same bound is also the maximum number of skills.
pub const MAX_SKILL_FILES: usize = MAX_SKILL_CATALOG;

const MAX_SKILL_CONTENT_BYTES: usize =
    siralos_core::skills::MAX_SKILL_CONTENT_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillFileRead {
    Complete(Vec<u8>),
    TooLarge,
    Unreadable,
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

fn has_markdown_extension(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.as_encoded_bytes().len() >= 3
            && name.as_encoded_bytes()[name.as_encoded_bytes().len() - 3..]
                .eq_ignore_ascii_case(b".md")
    })
}

fn read_skill_file(path: &Path) -> SkillFileRead {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Refuse a link and keep a substituted FIFO from blocking the
        // adapter before its opened-handle metadata can reject it.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open a reparse point itself; the handle metadata check below
        // rejects it instead of following it to another file.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(_) => return SkillFileRead::Unreadable,
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return SkillFileRead::Unreadable,
    };
    if is_reparse_point(&metadata)
        || metadata.file_type().is_symlink()
        || !metadata.is_file()
    {
        return SkillFileRead::Unreadable;
    }
    if metadata.len() > MAX_SKILL_CONTENT_BYTES as u64 {
        return SkillFileRead::TooLarge;
    }

    let mut bytes = Vec::new();
    let read_limit = (MAX_SKILL_CONTENT_BYTES as u64).saturating_add(1);
    if file.by_ref().take(read_limit).read_to_end(&mut bytes).is_err() {
        return SkillFileRead::Unreadable;
    }
    if bytes.len() > MAX_SKILL_CONTENT_BYTES {
        SkillFileRead::TooLarge
    } else {
        SkillFileRead::Complete(bytes)
    }
}

/// A typed skill-catalog loading failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoadFailure {
    /// Bounded, path-free truthful message.
    pub message: String,
}

fn failure(message: impl Into<String>) -> SkillLoadFailure {
    SkillLoadFailure { message: message.into() }
}

fn collect_bounded_paths<I>(
    entries: I,
) -> Result<Vec<PathBuf>, SkillLoadFailure>
where
    I: Iterator<Item = Result<PathBuf, SkillLoadFailure>>,
{
    let mut paths: Vec<PathBuf> = Vec::with_capacity(MAX_SKILL_FILES);
    for entry in entries.take(MAX_SKILL_FILES.saturating_add(1)) {
        let entry = entry?;
        if paths.len() >= MAX_SKILL_FILES {
            return Err(directory_entry_overflow_failure());
        }
        paths.push(entry);
    }
    Ok(paths)
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
    let siralos_dir = root.join(".siralos");
    let siralos_metadata = match std::fs::symlink_metadata(&siralos_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillCatalogLoad::Absent);
        }
        Err(_) => return Err(failure("skill catalog is unreadable")),
    };
    if is_reparse_point(&siralos_metadata)
        || siralos_metadata.file_type().is_symlink()
        || !siralos_metadata.is_dir()
    {
        return Err(failure(
            "skill catalog directory must be a real directory",
        ));
    }

    let dir = siralos_dir.join("skills");
    let skills_metadata = match std::fs::symlink_metadata(&dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillCatalogLoad::Absent);
        }
        Err(_) => return Err(failure("skill catalog is unreadable")),
    };
    if is_reparse_point(&skills_metadata)
        || skills_metadata.file_type().is_symlink()
        || !skills_metadata.is_dir()
    {
        return Err(failure(
            "skill catalog directory must be a real directory",
        ));
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillCatalogLoad::Absent);
        }
        Err(_) => return Err(failure("skill catalog is unreadable")),
    };
    let mut paths = collect_bounded_paths(entries.map(|entry| {
        entry
            .map(|entry| entry.path())
            .map_err(|_| failure("skill catalog is unreadable"))
    }))?;
    paths.sort();

    let mut skills = Vec::with_capacity(MAX_SKILL_FILES);
    for path in paths {
        if !has_markdown_extension(&path) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
        else {
            return Err(failure("a skill file name must be valid UTF-8"));
        };
        if !is_conservative_skill_name(name) {
            return Err(failure(
                "skill names must use only letters, digits, `_`, or `-`",
            ));
        }
        if is_protected_material_stem(name) {
            return Err(failure(
                "credential and policy material cannot be loaded as skills",
            ));
        }
        if skills.len() >= MAX_SKILL_FILES {
            return Err(catalog_overflow_failure());
        }

        let bytes = match read_skill_file(&path) {
            SkillFileRead::Complete(bytes) => bytes,
            SkillFileRead::TooLarge => {
                return Err(failure(format!(
                    "skill file exceeds the {MAX_SKILL_CONTENT_BYTES}-byte bound."
                )));
            }
            SkillFileRead::Unreadable => {
                return Err(failure(
                    "skill file is unreadable or not a regular file",
                ));
            }
        };
        let content = String::from_utf8(bytes)
            .map_err(|_| failure("skill file is not valid UTF-8"))?;
        if content.is_empty() {
            return Err(failure("skill content must not be empty"));
        }
        skills.push(
            SkillDefinition::new(name, &content)
                .map_err(|_| failure("skill definition is invalid"))?,
        );
    }
    let catalog = SkillCatalog::new(skills)
        .map_err(|_| failure("skill catalog is invalid"))?;
    Ok(SkillCatalogLoad::Catalog(catalog))
}

fn catalog_overflow_failure() -> SkillLoadFailure {
    failure(format!(
        "The skill catalog exceeds the {MAX_SKILL_FILES}-skill bound."
    ))
}

fn directory_entry_overflow_failure() -> SkillLoadFailure {
    failure(format!(
        "The skill directory exceeds the {MAX_SKILL_FILES}-entry bound."
    ))
}

fn is_conservative_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
        })
}

/// Stems that name credential or policy material rather than a skill.
///
/// The skills directory is projected into model guidance, so a workspace that
/// drops `credentials.md` or `AGENTS.md` beside its skills must not have that
/// file silently become prompt content. The comparison is case-insensitive so
/// the rule cannot be bypassed with `Credentials.MD`.
fn is_protected_material_stem(name: &str) -> bool {
    const PROTECTED: &[&str] = &[
        // Behavioral configuration, protected from model-facing mutation.
        "agents",
        // Credential material. The list names the SHAPE, not the topic: a skill
        // called `agent` or `env` is legitimate guidance and must load, so bare
        // topic words are deliberately absent.
        "credentials",
        "credential",
        "secrets",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        "id_ecdsa_sk",
        "private_key",
        "privatekey",
        "known_hosts",
        "authorized_keys",
        "kubeconfig",
        "gitconfig",
    ];
    let lowered = name.to_ascii_lowercase();
    PROTECTED.iter().any(|stem| *stem == lowered)
}

#[cfg(test)]
mod skills_loader_tests {
    use super::{
        MAX_SKILL_FILES, SkillCatalogLoad, collect_bounded_paths,
        load_workspace_skills,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_WORKSPACE_NONCE: AtomicUsize = AtomicUsize::new(0);

    fn workspace() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = NEXT_WORKSPACE_NONCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "siralos-skills-tests-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn skills_dir(root: &std::path::Path) -> std::path::PathBuf {
        let dir = root.join(".siralos").join("skills");
        std::fs::create_dir_all(&dir).expect("skills dir");
        dir
    }

    #[test]
    fn legitimate_topic_names_still_load_as_guidance() {
        // The protected-stem rule names the SHAPE of credential material, not
        // a topic: a skill about agents or environments is ordinary guidance.
        for name in ["agent.md", "env.md", "secret-rotation.md", "notes.md"] {
            let root = workspace();
            let dir = skills_dir(&root);
            std::fs::write(dir.join(name), "guidance").expect("write");
            let SkillCatalogLoad::Catalog(catalog) =
                load_workspace_skills(&root).expect("load")
            else {
                panic!("expected a catalog for {name}");
            };
            assert_eq!(catalog.skills().len(), 1, "{name}");
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn credential_and_policy_material_is_not_loaded_as_guidance() {
        for name in [
            "AGENTS.md",
            "credentials.md",
            "Credentials.MD",
            "id_rsa.md",
            "private_key.md",
            "authorized_keys.md",
        ] {
            let root = workspace();
            let dir = skills_dir(&root);
            std::fs::write(dir.join(name), "SECRET MATERIAL").expect("write");
            std::fs::write(dir.join("real-skill.md"), "guidance")
                .expect("write");
            let error = load_workspace_skills(&root)
                .expect_err("protected material must be refused");
            assert!(
                error.message.contains("cannot be loaded as skills"),
                "{name}: {}",
                error.message
            );
            let _ = std::fs::remove_dir_all(root);
        }
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
        assert_eq!(catalog.skills().len(), 2);
        assert_eq!(catalog.skills()[0].name(), "alpha");
        assert_eq!(catalog.skills()[1].name(), "zeta");
        let direct = siralos_core::skills::SkillDefinition::new(
            "alpha",
            "guidance for alpha",
        )
        .expect("skill");
        assert_eq!(catalog.skills()[0].digest(), direct.digest());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_collection_consumes_only_cap_plus_one_entries() {
        let consumed = std::cell::Cell::new(0usize);
        let entries = std::iter::from_fn(|| {
            let index = consumed.get() + 1;
            consumed.set(index);
            if index <= MAX_SKILL_FILES + 1 {
                Some(Ok(std::path::PathBuf::from(format!("{index}.md"))))
            } else {
                panic!("bounded collection consumed too many entries")
            }
        });
        let error = collect_bounded_paths(entries)
            .expect_err("the sentinel entry must exceed the bound");
        assert_eq!(consumed.get(), MAX_SKILL_FILES + 1);
        assert_eq!(
            error.message,
            format!(
                "The skill directory exceeds the {MAX_SKILL_FILES}-entry bound."
            )
        );
    }

    #[test]
    fn exactly_max_directory_entries_are_accepted() {
        let root = workspace();
        let dir = skills_dir(&root);
        for index in 0..MAX_SKILL_FILES {
            std::fs::write(
                dir.join(format!("skill-{index:02}.md")),
                "guidance",
            )
            .expect("write");
        }
        let SkillCatalogLoad::Catalog(catalog) =
            load_workspace_skills(&root).expect("load")
        else {
            panic!("expected a catalog");
        };
        assert_eq!(catalog.skills().len(), MAX_SKILL_FILES);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn exactly_max_content_is_accepted() {
        let root = workspace();
        let content =
            "x".repeat(siralos_core::skills::MAX_SKILL_CONTENT_BYTES);
        std::fs::write(skills_dir(&root).join("exact.md"), content)
            .expect("write");
        let SkillCatalogLoad::Catalog(catalog) =
            load_workspace_skills(&root).expect("load")
        else {
            panic!("expected a catalog");
        };
        assert_eq!(catalog.skills().len(), 1);
        assert_eq!(
            catalog.skills()[0].content().len(),
            siralos_core::skills::MAX_SKILL_CONTENT_BYTES
        );
        let _ = std::fs::remove_dir_all(root);
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
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn directory_entry_budget_is_enforced_during_iteration() {
        let root = workspace();
        let dir = skills_dir(&root);
        for index in 0..=MAX_SKILL_FILES {
            std::fs::write(
                dir.join(format!("ignored-{index:02}.txt")),
                "ignored",
            )
            .expect("write");
        }
        let error = load_workspace_skills(&root)
            .expect_err("directory entry budget must be bounded");
        assert_eq!(
            error.message,
            format!(
                "The skill directory exceeds the {MAX_SKILL_FILES}-entry bound."
            )
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn diagnostics_do_not_expose_entry_or_root_paths() {
        let base = workspace();
        let root = base.join("private-workspace");
        let dir = skills_dir(&root);
        let name = "private-skill-name";
        std::fs::write(
            dir.join(format!("{name}.md")),
            "x".repeat(siralos_core::skills::MAX_SKILL_CONTENT_BYTES + 1),
        )
        .expect("write");
        let error = load_workspace_skills(&root).expect_err("refused");
        assert!(error.message.contains("exceeds the"));
        assert!(!error.message.contains(name));
        assert!(!error.message.contains(root.to_string_lossy().as_ref()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn invalid_utf8_non_markdown_entry_is_ignored() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = workspace();
        let dir = skills_dir(&root);
        let mut ignored_name = OsString::from("notes");
        ignored_name.push(OsString::from_vec(vec![0xff]));
        ignored_name.push(".txt");
        std::fs::write(dir.join(ignored_name), "ignored").expect("write");
        std::fs::write(dir.join("alpha.md"), "guidance").expect("write");

        let SkillCatalogLoad::Catalog(catalog) =
            load_workspace_skills(&root).expect("load")
        else {
            panic!("expected a catalog");
        };
        assert_eq!(catalog.skills().len(), 1);
        assert_eq!(catalog.skills()[0].name(), "alpha");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn linked_siralos_directory_is_refused_without_path_diagnostics() {
        use std::os::unix::fs::symlink;

        let root = workspace();
        let outside = workspace();
        let outside_skills = outside.join("skills");
        std::fs::create_dir_all(&outside_skills).expect("outside skills");
        std::fs::write(outside_skills.join("foreign.md"), "guidance")
            .expect("write");
        let link = root.join(".siralos");
        symlink(&outside, &link).expect("symlink");

        let error = load_workspace_skills(&root).expect_err("refused");
        assert_eq!(
            error.message,
            "skill catalog directory must be a real directory"
        );
        assert!(!error.message.contains(root.to_string_lossy().as_ref()));
        assert!(!error.message.contains(outside.to_string_lossy().as_ref()));
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn linked_skills_directory_is_refused_without_path_diagnostics() {
        use std::os::unix::fs::symlink;

        let root = workspace();
        let outside = workspace();
        let outside_skills = outside.join("skills");
        std::fs::create_dir_all(&outside_skills).expect("outside skills");
        std::fs::write(outside_skills.join("foreign.md"), "guidance")
            .expect("write");
        let siralos = root.join(".siralos");
        std::fs::create_dir_all(&siralos).expect("siralos directory");
        let link = siralos.join("skills");
        symlink(&outside_skills, &link).expect("symlink");

        let error = load_workspace_skills(&root).expect_err("refused");
        assert_eq!(
            error.message,
            "skill catalog directory must be a real directory"
        );
        assert!(!error.message.contains(root.to_string_lossy().as_ref()));
        assert!(!error.message.contains(outside.to_string_lossy().as_ref()));
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_skill_file_is_refused_without_path_diagnostics() {
        use std::os::unix::fs::symlink;

        let root = workspace();
        let dir = skills_dir(&root);
        let target = dir.join("target.md");
        let link = dir.join("private-link.md");
        std::fs::write(&target, "guidance").expect("write");
        symlink(&target, &link).expect("symlink");
        let error = load_workspace_skills(&root).expect_err("refused");
        assert!(!error.message.contains("private-link"));
        assert!(!error.message.contains(root.to_string_lossy().as_ref()));
        let _ = std::fs::remove_dir_all(root);
    }
}
