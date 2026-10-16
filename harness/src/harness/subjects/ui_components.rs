//! UI-components subject (ADR 0039): declarative terminal UI panels.
//!
//! Each scenario seeds a fixture workspace (files, an optional
//! `.siralos/ui.toml` document, optional module manifests) and then
//! runs authoring/render ops in order through the PRODUCTION paths:
//! the `siralos-cli` UI document flow for parse/add/edit/remove/list,
//! the `WriteTool` adapter for the protected-path refusal, the core
//! projector plus the production panel snapshot draw for renders,
//! and the lifecycle/authority-gated merge for module specs. The
//! record pins stable fields only: op statuses, production
//! messages, merged list text, tab titles, and the 80x24 frame
//! rows. File identities pin `verifyPaths` outcomes.

use crate::harness::*;

use siralos_core::ui::{UI_VIEWPORT_HEIGHT, UI_VIEWPORT_WIDTH};

/// Build one `UnvalidatedSpec` from a scenario spec object. Missing
/// required strings are scenario bugs; production validation of
/// values stays inside `UiComponentSpec::parse`.
fn scenario_raw_spec(
    entry: &Value,
) -> Result<siralos_core::ui::UnvalidatedSpec, HarnessError> {
    use siralos_core::ui::UnvalidatedSpec;
    let string_field = |name: &str| {
        entry.get(name).and_then(Value::as_str).map(str::to_owned).ok_or_else(
            || {
                HarnessError::corpus(format!(
                    "ui-components spec {name} must be a string"
                ))
            },
        )
    };
    let enabled = match entry.get("enabled") {
        None => true,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => {
            return Err(HarnessError::corpus(
                "ui-components spec enabled must be a boolean".to_owned(),
            ));
        }
    };
    let size_hint = |name: &str, fallback: i64| match entry.get(name) {
        None => Ok(fallback),
        Some(value) => value.as_i64().ok_or_else(|| {
            HarnessError::corpus(format!(
                "ui-components spec {name} must be an integer"
            ))
        }),
    };
    let mut params = Vec::new();
    if let Some(table) = entry.get("params") {
        let table = table.as_object().ok_or_else(|| {
            HarnessError::corpus(
                "ui-components spec params must be an object".to_owned(),
            )
        })?;
        for (key, value) in table {
            let text = value.as_str().ok_or_else(|| {
                HarnessError::corpus(
                    "ui-components spec params must be strings".to_owned(),
                )
            })?;
            params.push((key.clone(), text.to_owned()));
        }
    }
    Ok(UnvalidatedSpec {
        id: string_field("id")?,
        title: string_field("title")?,
        feed: string_field("feed")?,
        enabled,
        placement: string_field("placement")?,
        max_rows: size_hint(
            "max_rows",
            siralos_core::ui::DEFAULT_SPEC_MAX_ROWS,
        )?,
        max_cols: size_hint(
            "max_cols",
            siralos_core::ui::DEFAULT_SPEC_MAX_COLS,
        )?,
        params,
    })
}

/// Render one validated spec deterministically for records.
fn summarize_spec(spec: &siralos_core::ui::UiComponentSpec) -> Value {
    json!({
        "id": spec.id(),
        "title": spec.title(),
        "feed": spec.feed().as_str(),
        "enabled": spec.enabled(),
        "placement": spec.placement().to_string(),
        "max_rows": spec.max_rows(),
        "max_cols": spec.max_cols(),
        "params": spec.params(),
    })
}

/// Seed the UI document plus module manifests from scenario input.
/// Module fixtures mirror `load_manifest` production inputs: the
/// manifest text under `<folder>/module-manifest.toml` inside the
/// fixture workspace.
fn seed_ui_fixtures(
    root: &std::path::Path,
    input: &Value,
) -> Result<
    Vec<(
        String,
        siralos_adapters::module::manifest::ModuleManifest,
        siralos_core::module::lifecycle::LifecycleState,
    )>,
    HarnessError,
> {
    use siralos_adapters::module::manifest::{
        MODULE_MANIFEST_FILE_NAME, load_manifest,
    };
    if let Some(document) = input.get("uiDocument").and_then(Value::as_str) {
        let directory = root.join(".siralos");
        std::fs::create_dir_all(&directory).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot seed ui document directory: {error}"),
            )
        })?;
        std::fs::write(directory.join("ui.toml"), document).map_err(
            |error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!("cannot seed ui document: {error}"),
                )
            },
        )?;
    }
    let mut modules = Vec::new();
    if let Ok(entries) = scenario_array(input, "modules") {
        for entry in entries {
            let folder = scenario_string(entry, "folder")?;
            let manifest_text = entry
                .get("manifest")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "ui-components module manifest must be a string"
                            .to_owned(),
                    )
                })?;
            let state_text = entry
                .get("state")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "ui-components module state must be a string"
                            .to_owned(),
                    )
                })?;
            let directory = root.join(&folder);
            std::fs::create_dir_all(&directory).map_err(|error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!("cannot seed module directory: {error}"),
                )
            })?;
            std::fs::write(
                directory.join(MODULE_MANIFEST_FILE_NAME),
                manifest_text,
            )
            .map_err(|error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!("cannot seed module manifest: {error}"),
                )
            })?;
            let manifest = load_manifest(root, std::path::Path::new(&folder))
                .map_err(|failure| {
                    HarnessError::corpus(format!(
                        "ui-components module manifest refused: {}",
                        failure.code()
                    ))
                })?;
            use siralos_core::module::lifecycle::LifecycleState;
            let state = match state_text {
                "absent" => LifecycleState::Absent,
                "installed" => LifecycleState::Installed,
                "enabled" => LifecycleState::Enabled,
                "active" => LifecycleState::Active,
                _ => {
                    return Err(HarnessError::corpus(
                        "ui-components module state must be a lifecycle state"
                            .to_owned(),
                    ));
                }
            };
            modules.push((folder.to_owned(), manifest, state));
        }
    }
    Ok(modules)
}

/// Merge user specs with module-declared specs through the
/// production gate (lifecycle plus feed authority, user ids win).
/// Mirrors the session `panels_report` assembly without the live
/// session: harness modules are never Active, so the install-time
/// requested capabilities are the authority ceiling, exactly as in
/// production for enabled-but-inactive modules.
fn merged_specs(
    root: &std::path::Path,
    modules: &[(
        String,
        siralos_adapters::module::manifest::ModuleManifest,
        siralos_core::module::lifecycle::LifecycleState,
    )],
) -> Result<Vec<siralos_core::ui::SourcedSpec>, HarnessError> {
    use siralos_cli::interactive::ui::{
        ModuleUiSource, load_ui_document, module_ui_specs,
    };
    use siralos_core::ui::merge_specs;
    let user = load_ui_document(root)
        .map(|document| document.specs)
        .unwrap_or_default();
    let mut sources = Vec::new();
    for (folder, manifest, state) in modules {
        sources.push(ModuleUiSource {
            id: folder.clone(),
            manifest,
            state: *state,
            grant: None,
        });
    }
    Ok(merge_specs(user, module_ui_specs(&sources)))
}

/// Run one `parse` op: stage the document text as the workspace UI
/// document, then load it through the production loader.
fn parse_op(
    root: &std::path::Path,
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::load_ui_document;
    let document =
        entry.get("document").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus(
                "ui-components parse document must be a string".to_owned(),
            )
        })?;
    let directory = root.join(".siralos");
    std::fs::create_dir_all(&directory).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot seed parse document directory: {error}"),
        )
    })?;
    std::fs::write(directory.join("ui.toml"), document).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot seed parse document: {error}"),
        )
    })?;
    match load_ui_document(root) {
        Ok(loaded) => Ok(json!({
            "op": "parse",
            "status": "ok",
            "specs": loaded
                .specs
                .iter()
                .map(summarize_spec)
                .collect::<Vec<_>>(),
        })),
        Err(error) => Ok(json!({
            "op": "parse",
            "status": "refused",
            "message": error.message,
        })),
    }
}

/// Observe a fresh one-shot write authority for authoring ops.
/// Observation failures are recorded refusals, never harness
/// errors: they are the production outcome under test.
fn observe_for_authoring(
    root: &std::path::Path,
) -> Result<
    Result<siralos_adapters::observed_write::ObservedWriteToken, String>,
    HarnessError,
> {
    use siralos_cli::interactive::ui::observe_ui_document;
    Ok(match observe_ui_document(root) {
        Err(error) => Err(error.message),
        Ok((_, None)) => Err("write authority unavailable".to_owned()),
        Ok((_, Some(token))) => Ok(token),
    })
}

/// Run one `add` op through the production authoring path.
fn add_op(
    root: &std::path::Path,
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::add_ui_spec;
    let raw = scenario_raw_spec(entry.get("spec").ok_or_else(|| {
        HarnessError::corpus(
            "ui-components add spec must be an object".to_owned(),
        )
    })?)?;
    let token = match observe_for_authoring(root)? {
        Err(message) => {
            return Ok(json!({
                "op": "add",
                "status": "refused",
                "message": message,
            }));
        }
        Ok(token) => token,
    };
    match add_ui_spec(root, &raw, &token) {
        Ok(message) => Ok(json!({
            "op": "add",
            "status": "ok",
            "message": message,
        })),
        Err(error) => Ok(json!({
            "op": "add",
            "status": "refused",
            "message": error.message,
        })),
    }
}

/// Run one `edit` op through the production authoring path.
fn edit_op(
    root: &std::path::Path,
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::edit_ui_spec;
    let id = scenario_string(entry, "id")?;
    let raw = scenario_raw_spec(entry.get("spec").ok_or_else(|| {
        HarnessError::corpus(
            "ui-components edit spec must be an object".to_owned(),
        )
    })?)?;
    let token = match observe_for_authoring(root)? {
        Err(message) => {
            return Ok(json!({
                "op": "edit",
                "status": "refused",
                "message": message,
            }));
        }
        Ok(token) => token,
    };
    match edit_ui_spec(root, &id, &raw, &token) {
        Ok(message) => Ok(json!({
            "op": "edit",
            "status": "ok",
            "message": message,
        })),
        Err(error) => Ok(json!({
            "op": "edit",
            "status": "refused",
            "message": error.message,
        })),
    }
}

/// Run one `remove` op through the production authoring path.
fn remove_op(
    root: &std::path::Path,
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::remove_ui_spec;
    let id = scenario_string(entry, "id")?;
    let token = match observe_for_authoring(root)? {
        Err(message) => {
            return Ok(json!({
                "op": "remove",
                "status": "refused",
                "message": message,
            }));
        }
        Ok(token) => token,
    };
    match remove_ui_spec(root, &id, &token) {
        Ok(message) => Ok(json!({
            "op": "remove",
            "status": "ok",
            "message": message,
        })),
        Err(error) => Ok(json!({
            "op": "remove",
            "status": "refused",
            "message": error.message,
        })),
    }
}

/// Run one `list` op through the production list renderer.
fn list_op(root: &std::path::Path) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::render_ui_list_text;
    Ok(json!({
        "op": "list",
        "text": render_ui_list_text(root),
    }))
}

/// Run one `write` op: drive the `WriteTool` adapter directly so
/// the protected-path refusal for `.siralos/ui.toml` is pinned.
/// Mirrors the `default-tools` write arm, including checkpoint
/// stripping on success.
fn write_op(
    scenario_id: &str,
    root: &std::path::Path,
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_adapters::tool::WriteTool;
    use siralos_adapters::workspace::checkpoint::open_checkpoint_store;
    use siralos_core::tool::Tool;
    let store_root =
        std::env::temp_dir().join(format!("siralos-cand-ui-{scenario_id}"));
    let _ = std::fs::remove_dir_all(&store_root);
    let store = open_checkpoint_store(root, &store_root).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot open fixture checkpoint store: {error}"),
        )
    })?;
    let tool = WriteTool::new(root, store).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot build write tool: {error}"),
        )
    })?;
    let token = CancellationToken::new();
    let path = scenario_string(entry, "path")?;
    let content =
        entry.get("content").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus(
                "ui-components write content must be a string".to_owned(),
            )
        })?;
    let result = tool
        .execute(&json!({ "path": path, "content": content }), token.signal());
    let _ = std::fs::remove_dir_all(&store_root);
    Ok(match result {
        ToolExecutionResult::Success { output, .. } => {
            let mut detail = output.as_object().cloned().unwrap_or_default();
            detail.remove("checkpoint");
            json!({
                "op": "write",
                "tool": "write",
                "status": "success",
                "output": detail,
            })
        }
        other => json!({
            "op": "write",
            "tool": "write",
            "status": other.status_str(),
            "message": other.message(),
        }),
    })
}

/// Run one `render` op: merge, project with live feed content, and
/// draw the production panels snapshot over the fixed viewport.
/// Assembly mirrors the session `panels_report` bundle.
fn render_op(
    root: &std::path::Path,
    modules: &[(
        String,
        siralos_adapters::module::manifest::ModuleManifest,
        siralos_core::module::lifecycle::LifecycleState,
    )],
    entry: &Value,
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::{
        UiFeedInputs, load_feed_module_records, resolve_spec_content,
    };
    use siralos_cli::tui::{UiPanels, render_panels_to_buffer};
    use siralos_core::tool::PermissionPolicy;
    use siralos_core::tool::registry::RegisteredToolInfo;
    use siralos_core::ui::{UiRegistry, project_components};
    let selected =
        entry.get("selected").and_then(Value::as_u64).ok_or_else(|| {
            HarnessError::corpus(
                "ui-components render selected must be a non-negative integer"
                    .to_owned(),
            )
        })? as usize;
    let merged = merged_specs(root, modules)?;
    let mut registry = UiRegistry::new();
    for item in &merged {
        if !item.spec.enabled() {
            continue;
        }
        if registry.insert(item.spec.clone()).is_err() {
            continue;
        }
    }
    let policy = PermissionPolicy::default();
    let tools: Vec<RegisteredToolInfo> = Vec::new();
    let records = load_feed_module_records(root);
    let inputs = UiFeedInputs {
        root,
        tools: &tools,
        policy: &policy,
        modules: &records,
        last_projection: None,
        checkpoint_state_dir: None,
    };
    // Resolve at the seam: no intermediate map, so a feed
    // refusal defaults to the empty slot right where the
    // projector asks for content.
    let projection = project_components(&registry, |spec| {
        resolve_spec_content(&inputs, spec).unwrap_or_default()
    });
    let panels = UiPanels { projection, selected }.clamped();
    let tabs = panels.tab_titles();
    if selected > tabs.len() - 1 {
        return Err(HarnessError::corpus(
            "ui-components render selected is out of range".to_owned(),
        ));
    }
    let buffer = render_panels_to_buffer(
        Some(&panels),
        None,
        "ui snapshot",
        UI_VIEWPORT_WIDTH,
        UI_VIEWPORT_HEIGHT,
    );
    let area = buffer.area;
    let mut frame: Vec<String> = Vec::with_capacity(area.height as usize);
    for row in 0..area.height {
        let mut line = String::new();
        for col in 0..area.width {
            if let Some(cell) = buffer.cell((col, row)) {
                line.push_str(cell.symbol());
            }
        }
        frame.push(line.trim_end().to_owned());
    }
    Ok(json!({
        "op": "render",
        "selected": selected,
        "tabs": tabs,
        "frame": frame,
    }))
}

/// Run one `merge` op: the merged component list with source
/// attribution through the production list renderer.
fn merge_op(
    root: &std::path::Path,
    modules: &[(
        String,
        siralos_adapters::module::manifest::ModuleManifest,
        siralos_core::module::lifecycle::LifecycleState,
    )],
) -> Result<Value, HarnessError> {
    use siralos_cli::interactive::ui::render_ui_list;
    Ok(json!({
        "op": "merge",
        "text": render_ui_list(&merged_specs(root, modules)?),
    }))
}

/// Run one ui-components scenario: fixture files plus the UI
/// document and module manifests, ops in order, verified file
/// identities.
pub(in crate::harness) fn ui_components_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    with_fixture_workspace(scenario_id, input, |root| {
        let modules = seed_ui_fixtures(root, input)?;
        let mut ops = Vec::new();
        for entry in scenario_array(input, "calls")? {
            let op = scenario_string(entry, "op")?;
            ops.push(match op.as_str() {
                "parse" => parse_op(root, entry)?,
                "add" => add_op(root, entry)?,
                "edit" => edit_op(root, entry)?,
                "remove" => remove_op(root, entry)?,
                "list" => list_op(root)?,
                "write" => write_op(scenario_id, root, entry)?,
                "render" => render_op(root, &modules, entry)?,
                "merge" => merge_op(root, &modules)?,
                other => {
                    return Err(HarnessError::corpus(format!(
                        "unsupported ui-components op {other}"
                    )));
                }
            });
        }
        let mut files = Vec::new();
        for path in scenario_array(input, "verifyPaths")? {
            let relative = path.as_str().ok_or_else(|| {
                HarnessError::corpus(
                    "verifyPaths entries must be strings".to_owned(),
                )
            })?;
            let sha256 = std::fs::read(root.join(relative))
                .ok()
                .map(|bytes| sha256_hex(&bytes));
            files.push(json!({
                "path": relative,
                "sha256": sha256,
            }));
        }
        Ok(json!({
            "ops": ops,
            "files": files,
        }))
    })
}
