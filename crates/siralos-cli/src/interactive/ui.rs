//! Workspace UI component documents (ADR 0039 authoring flow).
//!
//! User specs live in workspace `.siralos/ui.toml`, which model
//! tools cannot touch (protected behavioral configuration). The
//! Host reads and writes it through the observed-byte +
//! atomic-writer path: observe the exact revision, consume the
//! one-shot authority once, re-verify bytes plus identity before
//! parsing or replacing, stage beside the target, verify the
//! staged bytes through the spec parser, and commit
//! digest-bound. Rendering helpers below return raw text; every
//! dispatch site sanitizes before display.

use std::path::Path;

use siralos_adapters::atomic::stage_atomic;
use siralos_adapters::module::manifest::{ModuleRecord, load_module_records};
use siralos_adapters::observed_write::{
    ObservedRevision, ObservedWriteToken, observe_file_for_write,
};
use siralos_adapters::ui_feeds::{ContextSummary, FeedContext, resolve_feed};
use siralos_adapters::workspace::fs::MUTATION_TEMP_PREFIX;
use siralos_core::projection::LastProjection;
use siralos_core::tool::PermissionPolicy;
use siralos_core::tool::registry::RegisteredToolInfo;
use siralos_core::ui::{
    SourcedSpec, SpecSource, UiComponentSpec, UiSpecError, UnvalidatedSpec,
};

/// Feed inputs assembled by dispatch sites (stdio renders
/// directly; the worker assembles the same bundle). Read-only
/// handles only; resolution never retains them.
pub struct UiFeedInputs<'a> {
    /// Canonical workspace root.
    pub root: &'a Path,
    /// Registry definitions snapshot in registration order.
    pub tools: &'a [RegisteredToolInfo],
    /// Effective permission policy.
    pub policy: &'a PermissionPolicy,
    /// Installed module records backing the `modules.status`
    /// feed (ADR 0039 §2: install records, never live state).
    pub modules: &'a [ModuleRecord],
    /// Latest projection snapshot, when computed.
    pub last_projection: Option<&'a LastProjection>,
    /// Caller state directory for the checkpoint store.
    pub checkpoint_state_dir: Option<&'a Path>,
}

/// Load the installed module records for feed resolution. An
/// unreadable record set resolves as empty (no installed
/// modules), matching the empty install state.
#[must_use]
pub fn load_feed_module_records(root: &Path) -> Vec<ModuleRecord> {
    load_module_records(root).unwrap_or_default()
}

/// Resolve one spec's feed to sanitized display lines. Sanitizing
/// here keeps every render path honest: the projector and the
/// terminal both receive text that already survived the boundary.
pub fn resolve_spec_content(
    inputs: &UiFeedInputs<'_>,
    spec: &UiComponentSpec,
) -> Result<Vec<String>, String> {
    let summary;
    let context = match inputs.last_projection {
        Some(last) => {
            let request = &last.request;
            let mode = request.mode.as_str().to_owned();
            let mut stable_bytes = 0usize;
            let mut contextual_bytes = 0usize;
            let mut volatile_bytes = 0usize;
            for segment in request.context_projection.stable_segments.iter() {
                stable_bytes = stable_bytes.saturating_add(segment.bytes);
            }
            for segment in
                request.context_projection.contextual_segments.iter()
            {
                contextual_bytes =
                    contextual_bytes.saturating_add(segment.bytes);
            }
            for segment in request.context_projection.volatile_segments.iter()
            {
                volatile_bytes = volatile_bytes.saturating_add(segment.bytes);
            }
            summary = ContextSummary {
                mode,
                stable_bytes,
                contextual_bytes,
                volatile_bytes,
                estimated_tokens: request.estimated_tokens,
                working_maximum: request.pressure.working_maximum,
                pressure_state: request.pressure.state.as_str().to_owned(),
                pressure_percent: (request.pressure.ratio * 100.0).round()
                    as u64,
                tools_available: request.tool_projection.counts.available,
                tools_gated: request.tool_projection.counts.gated,
                tools_hidden: request.tool_projection.counts.hidden,
            };
            Some(&summary)
        }
        None => None,
    };
    let context = FeedContext {
        workspace_root: inputs.root,
        tools: inputs.tools,
        policy: inputs.policy,
        modules: inputs.modules,
        context,
        checkpoint_state_dir: inputs.checkpoint_state_dir,
    };
    resolve_feed(&context, spec.feed(), spec.params())
        .map(|lines| {
            lines
                .iter()
                .map(|line| crate::sanitize::sanitize_for_display(line))
                .collect()
        })
        .map_err(|error| error.message())
}

/// UI document file name under `.siralos/`.
pub const UI_TOML_FILE_NAME: &str = "ui.toml";
/// Protected directory holding the UI document.
pub const UI_TOML_DIR_NAME: &str = ".siralos";
/// Maximum UI document size in bytes.
pub const MAX_UI_DOCUMENT_BYTES: usize = 64 * 1024;
/// Maximum components per UI document.
pub const MAX_UI_COMPONENTS: usize = 32;

/// One UI document read failure. The `message` phrases are
/// stable so display text and the differential harness can pin
/// them; control flow and tests match on `kind`, never on the
/// human-readable string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiFileError {
    /// Machine-branchable failure class.
    pub kind: UiFileErrorKind,
    /// Deterministic, human-readable reason.
    pub message: String,
}

/// Machine-branchable class of a [`UiFileError`]. Variants group
/// failures by caller behavior (retry after reload, fix the spec,
/// pick another id), not by prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiFileErrorKind {
    /// A spec failed validation; the source error is preserved.
    InvalidSpec(UiSpecError),
    /// The document bytes are missing, unreadable, oversized, or
    /// not valid UTF-8.
    Unreadable,
    /// The document parses but has the wrong shape, or does not
    /// parse as TOML at all.
    Malformed,
    /// An id already exists.
    AlreadyExists,
    /// No component carries the addressed id.
    MissingComponent,
    /// An edit renames the addressed id.
    RenameRefused,
    /// No write authority exists or it is spent or stale.
    /// Callers reload and retry. The variants distinguish what
    /// failed so callers branch on kinds, never on prose.
    ObservationRefused,
    /// The one-shot authority was already consumed.
    OneShotConsumed,
    /// Bytes or identity moved under the observation.
    StaleConcurrent,
    /// Staging the replacement failed (bound, directory,
    /// stage, or staged-byte verification).
    StagingFailed,
    /// The digest-bound commit lost its race.
    CommitFailed,
}

fn error(kind: UiFileErrorKind, message: impl Into<String>) -> UiFileError {
    UiFileError { kind, message: message.into() }
}

/// One UI document: validated user specs in file order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiDocument {
    /// Validated specs in document order.
    pub specs: Vec<UiComponentSpec>,
}

fn ui_path(root: &Path) -> std::path::PathBuf {
    root.join(UI_TOML_DIR_NAME).join(UI_TOML_FILE_NAME)
}

fn parse_document(text: &str) -> Result<Vec<UnvalidatedSpec>, UiFileError> {
    // TOML extraction stays per-surface: this reads the workspace
    // `components` envelope with `toml_edit` (order-preserving
    // merge on save), while manifests read the `ui.components`
    // envelope with the `toml` crate in adapters. Only the
    // validation rules are shared, in `UiComponentSpec::parse`.
    let document: toml_edit::DocumentMut = text.parse().map_err(|_| {
        error(UiFileErrorKind::Malformed, "ui.toml does not parse")
    })?;
    let Some(components) = document.get("components") else {
        return Ok(Vec::new());
    };
    let tables = components.as_array_of_tables().ok_or_else(|| {
        error(
            UiFileErrorKind::Malformed,
            "ui.toml components must be an array of tables",
        )
    })?;
    if tables.len() > MAX_UI_COMPONENTS {
        return Err(error(
            UiFileErrorKind::Malformed,
            "ui.toml carries too many components",
        ));
    }
    let mut specs = Vec::new();
    for table in tables {
        specs.push(parse_entry(table)?);
    }
    Ok(specs)
}

fn table_string(
    table: &toml_edit::Table,
    key: &str,
    required: bool,
) -> Result<Option<String>, UiFileError> {
    match table.get(key) {
        None => {
            if required {
                Err(error(
                    UiFileErrorKind::Malformed,
                    format!("ui.toml component is missing {key:?}"),
                ))
            } else {
                Ok(None)
            }
        }
        Some(item) => match item.as_str() {
            Some(text) => Ok(Some(text.to_owned())),
            None => Err(error(
                UiFileErrorKind::Malformed,
                format!("ui.toml component {key:?} must be a string"),
            )),
        },
    }
}

fn parse_entry(
    table: &toml_edit::Table,
) -> Result<UnvalidatedSpec, UiFileError> {
    for key in table.iter().map(|(key, _)| key) {
        match key {
            "id" | "title" | "feed" | "enabled" | "placement" | "max_rows"
            | "max_cols" | "params" => {}
            _ => {
                return Err(error(
                    UiFileErrorKind::Malformed,
                    format!("ui.toml component has unknown key {key:?}"),
                ));
            }
        }
    }
    let id = table_string(table, "id", true)?.expect("required");
    let title = table_string(table, "title", true)?.expect("required");
    let feed = table_string(table, "feed", true)?.expect("required");
    let enabled = match table.get("enabled") {
        None => true,
        Some(item) => item.as_bool().ok_or_else(|| {
            error(
                UiFileErrorKind::Malformed,
                "ui.toml component \"enabled\" must be a boolean",
            )
        })?,
    };
    let placement = table_string(table, "placement", true)?.expect("required");
    let max_rows = table
        .get("max_rows")
        .and_then(toml_edit::Item::as_integer)
        .unwrap_or(siralos_core::ui::DEFAULT_SPEC_MAX_ROWS);
    let max_cols = table
        .get("max_cols")
        .and_then(toml_edit::Item::as_integer)
        .unwrap_or(siralos_core::ui::DEFAULT_SPEC_MAX_COLS);
    if table.get("max_rows").is_some_and(|item| !item.is_integer())
        || table.get("max_cols").is_some_and(|item| !item.is_integer())
    {
        return Err(error(
            UiFileErrorKind::Malformed,
            "ui.toml component size hints must be integers",
        ));
    }
    let mut params = Vec::new();
    if let Some(item) = table.get("params") {
        let table = item.as_table_like().ok_or_else(|| {
            error(
                UiFileErrorKind::Malformed,
                "ui.toml component params must be a table",
            )
        })?;
        for (key, value) in table.iter() {
            let text = value.as_str().ok_or_else(|| {
                error(
                    UiFileErrorKind::Malformed,
                    "ui.toml component params must be strings",
                )
            })?;
            params.push((key.to_owned(), text.to_owned()));
        }
    }
    let raw = UnvalidatedSpec {
        id,
        title,
        feed,
        enabled,
        placement,
        max_rows,
        max_cols,
        params,
    };
    UiComponentSpec::parse(&raw).map_err(|parse_error| {
        error(
            UiFileErrorKind::InvalidSpec(parse_error),
            format!("ui.toml component invalid: {}", parse_error.message()),
        )
    })?;
    Ok(raw)
}

/// Load and validate the workspace UI document. A missing file is
/// an empty document; anything unreadable, oversized, non-UTF-8,
/// unparsable, or invalid is a typed refusal.
pub fn load_ui_document(root: &Path) -> Result<UiDocument, UiFileError> {
    let path = ui_path(root);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UiDocument::default());
        }
        Err(_) => {
            return Err(error(
                UiFileErrorKind::Unreadable,
                "ui.toml cannot be read",
            ));
        }
    };
    if bytes.len() > MAX_UI_DOCUMENT_BYTES {
        return Err(error(
            UiFileErrorKind::Unreadable,
            "ui.toml exceeds the byte bound",
        ));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        error(UiFileErrorKind::Unreadable, "ui.toml is not valid UTF-8")
    })?;
    if text.trim().is_empty() {
        return Ok(UiDocument::default());
    }
    let raws = parse_document(text)?;
    let mut specs = Vec::new();
    for raw in &raws {
        specs.push(UiComponentSpec::parse(raw).map_err(
            |parse_error: UiSpecError| {
                error(
                    UiFileErrorKind::InvalidSpec(parse_error),
                    format!(
                        "ui.toml component invalid: {}",
                        parse_error.message()
                    ),
                )
            },
        )?);
    }
    Ok(UiDocument { specs })
}

/// Serialize one document deterministically: fixed field order,
/// document order preserved. Built fresh every save so output is
/// byte-stable for identical specs.
fn serialize_document(specs: &[UiComponentSpec]) -> String {
    let mut document = toml_edit::DocumentMut::new();
    let mut array = toml_edit::ArrayOfTables::new();
    for spec in specs {
        let mut table = toml_edit::Table::new();
        table.insert("id", toml_edit::value(spec.id()));
        table.insert("title", toml_edit::value(spec.title()));
        table.insert("feed", toml_edit::value(spec.feed().as_str()));
        table.insert("enabled", toml_edit::value(spec.enabled()));
        table.insert(
            "placement",
            toml_edit::value(spec.placement().to_string()),
        );
        table.insert("max_rows", toml_edit::value(spec.max_rows() as i64));
        table.insert("max_cols", toml_edit::value(spec.max_cols() as i64));
        let mut params = toml_edit::Table::new();
        for (key, value) in spec.params() {
            params.insert(key, toml_edit::value(value));
        }
        if !params.is_empty() {
            table.insert("params", toml_edit::Item::Table(params));
        }
        array.push(table);
    }
    document.insert("components", toml_edit::Item::ArrayOfTables(array));
    document.to_string()
}

/// Observe the UI document revision for a later guarded write.
/// Returns the current validated document plus the one-shot write
/// authority (or the honest reason no authority exists).
pub fn observe_ui_document(
    root: &Path,
) -> Result<(UiDocument, Option<ObservedWriteToken>), UiFileError> {
    let relative =
        std::path::PathBuf::from(UI_TOML_DIR_NAME).join(UI_TOML_FILE_NAME);
    match observe_file_for_write(root, &relative, MAX_UI_DOCUMENT_BYTES) {
        ObservedRevision::Unbindable { reason } => Err(error(
            UiFileErrorKind::ObservationRefused,
            format!("ui.toml cannot authorize a write: {reason}"),
        )),
        ObservedRevision::Bound { bytes, token } => {
            let document = if bytes.is_empty() {
                UiDocument::default()
            } else {
                load_ui_document(root)?
            };
            Ok((document, Some(token)))
        }
    }
}

/// Save one document under a consumed one-shot authority: consume,
/// re-verify bytes plus identity, serialize, stage beside the
/// target inside `.siralos/`, verify the staged bytes through the
/// spec parser, and commit digest-bound (digest when the target
/// existed, absence-checked when it did not). Any mismatch
/// refuses stale.
pub fn save_ui_document(
    root: &Path,
    document: &UiDocument,
    observed: &ObservedWriteToken,
) -> Result<(), UiFileError> {
    if !observed.consume() {
        return Err(error(
            UiFileErrorKind::OneShotConsumed,
            "ui.toml write authority is one-shot; reload before retrying",
        ));
    }
    let path = ui_path(root);
    let current = std::fs::read(&path).ok();
    // One check covers bytes and identity: `matches_path` refuses
    // on a byte mismatch before comparing identity, and matches an
    // absent target only against absent authority — no absence
    // guard beside it.
    if !observed.matches_path(&path, current.as_deref()) {
        return Err(error(
            UiFileErrorKind::StaleConcurrent,
            "ui.toml changed concurrently; reload first",
        ));
    }
    let serialized = serialize_document(&document.specs);
    if serialized.len() > MAX_UI_DOCUMENT_BYTES {
        return Err(error(
            UiFileErrorKind::StagingFailed,
            "ui.toml exceeds the byte bound",
        ));
    }
    let directory = root.join(UI_TOML_DIR_NAME);
    std::fs::create_dir_all(&directory).map_err(|_| {
        error(
            UiFileErrorKind::StagingFailed,
            "ui.toml directory cannot be created",
        )
    })?;
    let staged = stage_atomic(
        &directory,
        UI_TOML_FILE_NAME,
        &format!("{MUTATION_TEMP_PREFIX}ui-toml"),
        serialized.as_bytes(),
        Some(0o600),
    )
    .map_err(|_| {
        error(UiFileErrorKind::StagingFailed, "ui.toml could not be staged")
    })?;
    let staged_bytes = std::fs::read(staged.path()).map_err(|_| {
        error(
            UiFileErrorKind::StagingFailed,
            "ui.toml staged bytes cannot be verified",
        )
    })?;
    let staged_text = std::str::from_utf8(&staged_bytes).map_err(|_| {
        error(
            UiFileErrorKind::StagingFailed,
            "ui.toml staged bytes are not UTF-8",
        )
    })?;
    parse_document(staged_text).map_err(|_| {
        error(
            UiFileErrorKind::StagingFailed,
            "ui.toml staged document failed verification",
        )
    })?;
    if let Some(previous) = current {
        let digest = siralos_core::identity::sha256_hex(&previous);
        staged.commit_if_digest(&digest).map_err(|_| {
            error(
                UiFileErrorKind::CommitFailed,
                "ui.toml changed during commit; reload first",
            )
        })?;
    } else {
        staged.commit_if_absent().map_err(|_| {
            error(
                UiFileErrorKind::CommitFailed,
                "ui.toml changed during commit; reload first",
            )
        })?;
    }
    Ok(())
}

/// Load workspace user specs with their source tags. Module
/// specs join in P5; this seam already returns the merged shape
/// so render paths do not change.
pub fn load_sourced_user_specs(
    root: &Path,
) -> Result<Vec<SourcedSpec>, UiFileError> {
    Ok(siralos_core::ui::merge_specs(
        load_ui_document(root)?.specs,
        Vec::new(),
    ))
}

/// One module's contribution input: its manifest plus live
/// lifecycle position. The grant is the Active grant when the
/// module is active; otherwise authority falls back to the
/// install-time requested set (already ceiling-checked).
pub struct ModuleUiSource<'m, 'g> {
    /// Declaring module id.
    pub id: String,
    /// Loaded manifest carrying the declared specs.
    pub manifest: &'m siralos_adapters::module::manifest::ModuleManifest,
    /// Current lifecycle state.
    pub state: siralos_core::module::lifecycle::LifecycleState,
    /// Effective grant while Active, `None` otherwise.
    pub grant: Option<&'g siralos_core::module::CapabilityGrant>,
}

/// Collect module-declared specs gated by lifecycle and feed
/// authority (ADR 0039 §2/§5): installed-but-disabled modules
/// contribute nothing; a spec naming a feed outside the
/// module's authority is dropped, never rendered. Returns
/// `(module id, spec)` pairs in manifest order for
/// [`merge_specs`](siralos_core::ui::merge_specs).
#[must_use]
pub fn module_ui_specs(
    sources: &[ModuleUiSource<'_, '_>],
) -> Vec<(String, UiComponentSpec)> {
    use siralos_core::module::capability::CapabilityId;
    use siralos_core::module::lifecycle::LifecycleState;
    let mut collected = Vec::new();
    for source in sources {
        match source.state {
            LifecycleState::Enabled | LifecycleState::Active => {}
            LifecycleState::Absent | LifecycleState::Installed => continue,
        }
        for spec in source.manifest.ui_specs() {
            let allowed =
                match siralos_adapters::ui_feeds::feed_capability(spec.feed())
                {
                    None => true,
                    Some(capability) => {
                        let Ok(id) = CapabilityId::parse(capability) else {
                            continue;
                        };
                        match source.grant {
                            Some(grant) => grant.contains(&id),
                            None => source
                                .manifest
                                .package()
                                .requested_capabilities()
                                .contains(&id),
                        }
                    }
                };
            if allowed {
                collected.push((source.id.clone(), spec.clone()));
            }
        }
    }
    collected
}

/// Render the merged component list, mapping file failures to
/// display text. Raw text; dispatch sanitizes.
#[must_use]
pub fn render_ui_list_text(root: &Path) -> String {
    match load_sourced_user_specs(root) {
        Ok(sourced) => render_ui_list(&sourced),
        Err(error) => format!("ui list unavailable: {}\n", error.message),
    }
}

/// Remove one component under a fresh observation, mapping every
/// outcome to display text. Raw text; dispatch sanitizes.
#[must_use]
pub fn execute_ui_remove(root: &Path, id: &str) -> String {
    if id.is_empty() {
        return "usage: /ui-remove <id>\n".to_owned();
    }
    let token = match observe_ui_document(root) {
        Ok((_, Some(token))) => token,
        Ok((_, None)) => {
            return "ui remove refused: write authority unavailable\n"
                .to_owned();
        }
        Err(error) => {
            return format!("ui remove refused: {}\n", error.message);
        }
    };
    match remove_ui_spec(root, id, &token) {
        Ok(message) => format!("{message}\n"),
        Err(error) => format!("ui remove refused: {}\n", error.message),
    }
}

/// Render one component preview with live feed content, mapping
/// every outcome to display text. Raw text; dispatch sanitizes.
#[must_use]
pub fn render_ui_preview_text(inputs: &UiFeedInputs<'_>, id: &str) -> String {
    if id.is_empty() {
        return "usage: /ui-preview <id>\n".to_owned();
    }
    let specs = match load_sourced_user_specs(inputs.root) {
        Ok(sourced) => sourced,
        Err(error) => {
            return format!("ui preview unavailable: {}\n", error.message);
        }
    };
    let Some(item) = specs.iter().find(|each| each.spec.id() == id) else {
        return format!("no ui component {id:?}\n");
    };
    match resolve_spec_content(inputs, &item.spec) {
        Ok(content) => render_ui_preview(&item.spec, &content),
        Err(message) => format!("ui preview refused: {message}\n"),
    }
}

/// Drain one completed spec form: build the raw spec from the
/// validated form data and add or edit under a fresh observation.
/// Returns display text (success or typed refusal). Raw text;
/// dispatch sanitizes.
#[must_use]
pub fn drain_ui_spec_form(
    root: &Path,
    editing: Option<String>,
    data: &crate::tui::UiSpecData,
) -> String {
    let id = editing.as_deref().unwrap_or(data.id.as_str());
    if id.is_empty() {
        return "ui form refused: missing component id\n".to_owned();
    }
    let raw = UnvalidatedSpec {
        id: id.to_owned(),
        title: data.title.clone(),
        feed: data.feed.clone(),
        placement: data.placement.clone(),
        max_rows: data.max_rows,
        max_cols: data.max_cols,
        enabled: data.enabled,
        params: data.params.clone(),
    };
    let observed = match observe_ui_document(root) {
        Ok((_, Some(token))) => token,
        Ok((_, None)) => {
            return "ui form refused: write authority unavailable\n"
                .to_owned();
        }
        Err(error) => {
            return format!("ui form refused: {}\n", error.message);
        }
    };
    let outcome = match editing {
        Some(_) => edit_ui_spec(root, id, &raw, &observed),
        None => add_ui_spec(root, &raw, &observed),
    };
    match outcome {
        Ok(message) => format!("{message}\n"),
        Err(error) => format!("ui form refused: {}\n", error.message),
    }
}

/// Render the merged component list. Raw text; dispatch sanitizes.
#[must_use]
pub fn render_ui_list(sourced: &[SourcedSpec]) -> String {
    if sourced.is_empty() {
        return "UI components:\n  (none — add one with /ui-add)\n".to_owned();
    }
    let mut out = String::from("UI components:\n");
    for item in sourced {
        let source = match &item.source {
            SpecSource::User => "user".to_owned(),
            SpecSource::Module { id } => format!("module:{id}"),
        };
        let state = if item.spec.enabled() { "enabled" } else { "disabled" };
        out.push_str(&format!(
            "  {} - {} ({}, {}, {source}, {state})\n",
            item.spec.id(),
            item.spec.title(),
            item.spec.feed().as_str(),
            item.spec.placement(),
        ));
    }
    out
}

/// Render one spec preview with resolved feed lines. Raw text;
/// dispatch sanitizes.
#[must_use]
pub fn render_ui_preview(
    spec: &UiComponentSpec,
    content: &[String],
) -> String {
    let mut out = format!(
        "{} - {}\nfeed: {}\nplacement: {}\n",
        spec.id(),
        spec.title(),
        spec.feed().as_str(),
        spec.placement(),
    );
    if content.is_empty() {
        out.push_str("(no content)\n");
    } else {
        for line in content {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Add one validated spec: duplicate ids refuse typed.
pub fn add_ui_spec(
    root: &Path,
    raw: &UnvalidatedSpec,
    observed: &ObservedWriteToken,
) -> Result<String, UiFileError> {
    let spec = UiComponentSpec::parse(raw).map_err(|parse_error| {
        error(
            UiFileErrorKind::InvalidSpec(parse_error),
            format!("ui spec invalid: {}", parse_error.message()),
        )
    })?;
    let document = load_ui_document(root)?;
    if document.specs.iter().any(|each| each.id() == spec.id()) {
        return Err(error(
            UiFileErrorKind::AlreadyExists,
            format!("ui component {:?} already exists", spec.id()),
        ));
    }
    let mut specs = document.specs;
    specs.push(spec.clone());
    save_ui_document(root, &UiDocument { specs }, observed)?;
    Ok(format!("Added ui component {}.", spec.id()))
}

/// Replace the spec addressed by id. Id renames and missing ids
/// refuse typed.
pub fn edit_ui_spec(
    root: &Path,
    id: &str,
    raw: &UnvalidatedSpec,
    observed: &ObservedWriteToken,
) -> Result<String, UiFileError> {
    let spec = UiComponentSpec::parse(raw).map_err(|parse_error| {
        error(
            UiFileErrorKind::InvalidSpec(parse_error),
            format!("ui spec invalid: {}", parse_error.message()),
        )
    })?;
    if spec.id() != id {
        return Err(error(
            UiFileErrorKind::RenameRefused,
            "ui component id renames are refused",
        ));
    }
    let document = load_ui_document(root)?;
    let mut specs = document.specs;
    let Some(slot) = specs.iter_mut().find(|each| each.id() == id) else {
        return Err(error(
            UiFileErrorKind::MissingComponent,
            format!("no ui component {id:?}"),
        ));
    };
    *slot = spec;
    save_ui_document(root, &UiDocument { specs }, observed)?;
    Ok(format!("Edited ui component {id}."))
}

/// Remove the spec addressed by id. Missing ids refuse typed.
pub fn remove_ui_spec(
    root: &Path,
    id: &str,
    observed: &ObservedWriteToken,
) -> Result<String, UiFileError> {
    let document = load_ui_document(root)?;
    let mut specs = document.specs;
    let before = specs.len();
    specs.retain(|each| each.id() != id);
    if specs.len() == before {
        return Err(error(
            UiFileErrorKind::MissingComponent,
            format!("no ui component {id:?}"),
        ));
    }
    save_ui_document(root, &UiDocument { specs }, observed)?;
    Ok(format!("Removed ui component {id}."))
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_UI_COMPONENTS, UiDocument, UiFeedInputs, UiFileErrorKind,
        add_ui_spec, edit_ui_spec, load_feed_module_records, load_ui_document,
        observe_ui_document, remove_ui_spec, render_ui_list,
        render_ui_preview, resolve_spec_content, save_ui_document,
    };
    use siralos_core::ui::{UnvalidatedSpec, merge_specs};

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "siralos-ui-doc-{}-{}",
            std::process::id(),
            label
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn raw(id: &str) -> UnvalidatedSpec {
        UnvalidatedSpec {
            id: id.to_owned(),
            title: format!("{id} title"),
            feed: "tools.catalog".to_owned(),
            enabled: true,
            placement: "tab".to_owned(),
            max_rows: 20,
            max_cols: 60,
            params: Vec::new(),
        }
    }

    #[test]
    fn missing_document_loads_empty() {
        let root = fixture_root("missing");
        let document = load_ui_document(&root).expect("loads");
        assert!(document.specs.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_load_round_trip_is_byte_stable() {
        let root = fixture_root("roundtrip");
        let (observed_doc, token) =
            observe_ui_document(&root).expect("observes");
        assert!(observed_doc.specs.is_empty());
        let token = token.expect("bindable");
        let document = UiDocument {
            specs: vec![
                siralos_core::ui::UiComponentSpec::parse(&raw("b"))
                    .expect("parses"),
                siralos_core::ui::UiComponentSpec::parse(&raw("a"))
                    .expect("parses"),
            ],
        };
        save_ui_document(&root, &document, &token).expect("saves");
        let first = std::fs::read(root.join(".siralos").join("ui.toml"))
            .expect("read");
        let reloaded = load_ui_document(&root).expect("reloads");
        assert_eq!(reloaded, document);
        let (_, token) = observe_ui_document(&root).expect("re-observes");
        save_ui_document(&root, &reloaded, &token.expect("bindable"))
            .expect("re-saves");
        let second = std::fs::read(root.join(".siralos").join("ui.toml"))
            .expect("read");
        assert_eq!(first, second, "round trip is byte-stable");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn add_edit_remove_enforce_identity_and_staleness() {
        let root = fixture_root("crud");
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        let message = add_ui_spec(&root, &raw("one"), &token).expect("adds");
        assert_eq!(message, "Added ui component one.");
        // The one-shot authority is spent: a second mutation with
        // the same token refuses stale instead of writing.
        assert!(matches!(
            add_ui_spec(&root, &raw("two"), &token).unwrap_err().kind,
            UiFileErrorKind::OneShotConsumed
        ));
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        assert!(matches!(
            add_ui_spec(&root, &raw("one"), &token).unwrap_err().kind,
            UiFileErrorKind::AlreadyExists
        ));
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        let mut renamed = raw("two");
        renamed.id = "other".to_owned();
        assert!(matches!(
            edit_ui_spec(&root, "two", &renamed, &token).unwrap_err().kind,
            UiFileErrorKind::RenameRefused
        ));
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        assert!(matches!(
            edit_ui_spec(&root, "missing", &raw("missing"), &token)
                .unwrap_err()
                .kind,
            UiFileErrorKind::MissingComponent
        ));
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        edit_ui_spec(&root, "one", &raw("one"), &token).expect("edits");
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        assert!(matches!(
            remove_ui_spec(&root, "missing", &token).unwrap_err().kind,
            UiFileErrorKind::MissingComponent
        ));
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        remove_ui_spec(&root, "one", &token).expect("removes");
        assert!(load_ui_document(&root).expect("loads").specs.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_documents_refuse_typed() {
        let root = fixture_root("malformed");
        std::fs::create_dir_all(root.join(".siralos")).expect("dir");
        for (label, text) in [
            ("syntax", "[[components\n"),
            ("shape", "components = 42\n"),
            (
                "entry",
                "[[components]]\ntitle = \"no id\"\nfeed = \"tools.catalog\"\nplacement = \"tab\"\n",
            ),
            (
                "feed",
                "[[components]]\nid = \"x\"\ntitle = \"x\"\nfeed = \"nope\"\nplacement = \"tab\"\n",
            ),
            ("overflow", &"[[components]]\n".repeat(MAX_UI_COMPONENTS + 1)),
        ] {
            std::fs::write(root.join(".siralos").join("ui.toml"), text)
                .expect("seed");
            assert!(load_ui_document(&root).is_err(), "{label} must refuse");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_preserves_spec_error_kind() {
        use siralos_core::ui::UiSpecError;
        let root = fixture_root("speckind");
        std::fs::create_dir_all(root.join(".siralos")).expect("dir");
        std::fs::write(
            root.join(".siralos").join("ui.toml"),
            "[[components]]\nid = \"x\"\ntitle = \"x\"\nfeed = \"nope\"\nplacement = \"tab\"\n",
        )
        .expect("seed");
        assert!(matches!(
            load_ui_document(&root).unwrap_err().kind,
            UiFileErrorKind::InvalidSpec(UiSpecError::UnknownFeed)
        ));
        std::fs::write(
            root.join(".siralos").join("ui.toml"),
            "[[components]]\nid = \"x\"\ntitle = \"x\"\nfeed = \"tools.catalog\"\nplacement = \"cell:2,0\"\n",
        )
        .expect("seed");
        assert!(matches!(
            load_ui_document(&root).unwrap_err().kind,
            UiFileErrorKind::InvalidSpec(UiSpecError::InvalidPlacement)
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn modules_status_lists_declared_install_records() {
        let root = fixture_root("modstatus");
        std::fs::write(
            root.join("siralos.toml"),
            "[modules.demo]\npath = \"mods/demo\"\ndigest = \"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n",
        )
        .expect("seed");
        let records = load_feed_module_records(&root);
        assert_eq!(records.len(), 1, "{records:?}");
        let policy = siralos_core::tool::PermissionPolicy::default();
        let tools: Vec<siralos_core::tool::registry::RegisteredToolInfo> =
            Vec::new();
        let inputs = UiFeedInputs {
            root: &root,
            tools: &tools,
            policy: &policy,
            modules: &records,
            last_projection: None,
            checkpoint_state_dir: None,
        };
        let spec =
            siralos_core::ui::UiComponentSpec::parse(&UnvalidatedSpec {
                id: "mod-status".to_owned(),
                title: "Modules".to_owned(),
                feed: "modules.status".to_owned(),
                enabled: true,
                placement: "tab".to_owned(),
                max_rows: 20,
                max_cols: 60,
                params: Vec::new(),
            })
            .expect("parses");
        let lines = resolve_spec_content(&inputs, &spec).expect("resolves");
        assert!(lines.iter().any(|line| line.contains("demo")), "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.contains("none installed")),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn concurrent_modification_refuses_stale() {
        let root = fixture_root("stale");
        let (_, token) = observe_ui_document(&root).expect("observes");
        let token = token.expect("bindable");
        // An external write between observation and save breaks the
        // single identity check: bytes and identity refuse together.
        std::fs::create_dir_all(root.join(".siralos")).expect("dir");
        std::fs::write(
            root.join(".siralos").join("ui.toml"),
            "[[components]]\nid = \"other\"\ntitle = \"Other\"\nfeed = \"tools.catalog\"\nplacement = \"tab\"\n",
        )
        .expect("external write");
        assert!(matches!(
            add_ui_spec(&root, &raw("one"), &token).unwrap_err().kind,
            UiFileErrorKind::StaleConcurrent
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn render_shapes_are_stable() {
        use siralos_core::ui::SourcedSpec;
        let specs = vec![
            siralos_core::ui::UiComponentSpec::parse(&raw("b"))
                .expect("parses"),
        ];
        let sourced: Vec<SourcedSpec> =
            merge_specs(specs, Vec::new()).into_iter().collect();
        let listed = render_ui_list(&sourced);
        assert!(listed.contains('b'), "{listed:?}");
        assert!(listed.contains("tools.catalog"), "{listed:?}");
        assert!(listed.contains("user"), "{listed:?}");
        let empty = render_ui_list(&[]);
        assert!(empty.contains("(none"), "{empty:?}");
        let preview =
            render_ui_preview(&sourced[0].spec, &["line one".to_owned()]);
        assert!(preview.contains("line one"), "{preview:?}");
        let blank = render_ui_preview(&sourced[0].spec, &[]);
        assert!(blank.contains("(no content)"), "{blank:?}");
    }
}
