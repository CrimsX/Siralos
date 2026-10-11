//! Module command rendering and the host setup those commands require
//! (source validation, rejection codes, activation gates).

use super::{
    ActivationRequest, BTreeMap, HostAuthority, LifecycleState, ModuleHost,
    ModuleHostBounds, ModuleManifest, ModuleRecord, Path, PathValidationError,
    RuntimeCheckResult, decide_module_activation, format_module_added,
    format_modules, is_model_protected_workspace_path, load_manifest,
    load_module_records, resolve_workspace_path, sanitize_for_display,
    validate_relative_path, verify_component,
};

/// Render the `/modules` empty-state or installed view.
pub(super) fn render_modules(workspace_root: &Path) -> String {
    match load_module_records(workspace_root) {
        Ok(records) => format_modules(&records),
        Err(failure) => {
            format!(
                "Modules unavailable: {failure} (code {})\n",
                failure.code()
            )
        }
    }
}

fn validate_source_path(path: &str) -> Result<(), &'static str> {
    validate_relative_path(path).map_err(|error| match error {
        PathValidationError::NullByte => "PATH_NULL_BYTE",
        PathValidationError::Empty => "PATH_EMPTY",
        PathValidationError::Absolute => "PATH_ABSOLUTE",
        PathValidationError::ParentTraversal => "PATH_PARENT_TRAVERSAL",
    })?;

    if is_model_protected_workspace_path(path) {
        Err("PATH_PROTECTED")
    } else {
        Ok(())
    }
}

/// Run one `/modules-add <folder>` flow: pick, verify, record.
pub(super) fn render_add_module(
    workspace_root: &Path,
    folder: &str,
    hosts: &mut BTreeMap<String, ModuleHost>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
) -> String {
    if let Err(code) = validate_source_path(folder) {
        return format!("Add Module failed: folder rejected (code {code})\n");
    }
    let resolved_folder = match resolve_workspace_path(workspace_root, folder)
    {
        Ok(resolved) => resolved,
        Err(rejection) => {
            return format!(
                "Add Module failed: folder rejected (code {})\n",
                rejection_code(&rejection)
            );
        }
    };
    if let Err(code) =
        validate_source_path(&resolved_folder.workspace_relative_path)
    {
        return format!("Add Module failed: folder rejected (code {code})\n");
    }
    let manifest =
        match load_manifest(workspace_root, &resolved_folder.absolute_path) {
            Ok(manifest) => manifest,
            Err(failure) => {
                return format!(
                    "Add Module failed: module manifest rejected (code {})\n",
                    failure.code()
                );
            }
        };
    let id = manifest.package().id().as_str().to_owned();
    let digest = manifest.package().digest().as_str().to_owned();
    let record = ModuleRecord {
        id: id.clone(),
        path: resolved_folder.workspace_relative_path.clone(),
        digest: format!("sha256:{digest}"),
    };

    // Preflight the persisted identity before constructing or installing a
    // ModuleHost.
    let current_records = match load_module_records(workspace_root) {
        Ok(records) => records,
        Err(failure) => {
            return format!(
                "Add Module failed: module records rejected (code {})\n",
                failure.code()
            );
        }
    };
    if current_records.iter().any(|existing| {
        existing.id == id
            && (existing.path != record.path
                || existing.digest != record.digest)
    }) {
        return "Add Module failed: module record conflict (code RECORD_CONFLICT)\n"
            .to_owned();
    }
    if manifest.component().is_none() && hosts.contains_key(&id) {
        return "Add Module failed: module record conflict (code RECORD_CONFLICT)\n"
            .to_owned();
    }

    // Keep the potentially installed host private until record_module's
    // digest-CAS commit succeeds.
    let pending_host = if let Some(component_path) = manifest.component() {
        let authority = match HostAuthority::parse(&[]) {
            Ok(authority) => authority,
            Err(failure) => {
                return format!(
                    "Add Module failed: module installation failed (code {})\n",
                    failure.code()
                );
            }
        };
        let mut host = ModuleHost::new(
            manifest.package().abi().clone(),
            authority,
            component_path.to_path_buf(),
            workspace_root.to_path_buf(),
            ModuleHostBounds::default(),
        );
        if let Err(failure) = host.install(manifest.package().clone()) {
            return format!(
                "Add Module failed: module installation failed (code {})\n",
                failure.code()
            );
        }
        Some(host)
    } else {
        None
    };

    if let Err(failure) =
        siralos_adapters::module::record_module(workspace_root, &record)
    {
        return format!(
            "Add Module failed: module record rejected (code {})\n",
            failure.code()
        );
    }
    if let Some(host) = pending_host {
        hosts.insert(id.clone(), host);
    }
    manifests.insert(id, manifest);
    format_module_added(&record)
}

pub(super) fn ensure_host<'a>(
    workspace_root: &Path,
    id: &str,
    hosts: &'a mut BTreeMap<String, ModuleHost>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
) -> Result<&'a mut ModuleHost, String> {
    // Always re-read the current record before considering an in-memory host.
    let records = load_module_records(workspace_root).map_err(|failure| {
        format!("module record rejected (code {})", failure.code())
    })?;
    let record =
        records.iter().find(|record| record.id == id).ok_or_else(|| {
            "module record conflict (code RECORD_CONFLICT)".to_owned()
        })?;
    validate_source_path(&record.path).map_err(|_| {
        "module record conflict (code RECORD_CONFLICT)".to_owned()
    })?;
    let folder = resolve_workspace_path(workspace_root, &record.path)
        .map_err(|rejection| {
            format!(
                "module folder rejected (code {})",
                rejection_code(&rejection)
            )
        })?;
    validate_source_path(&folder.workspace_relative_path).map_err(|_| {
        "module record conflict (code RECORD_CONFLICT)".to_owned()
    })?;
    let manifest = load_manifest(workspace_root, &folder.absolute_path)
        .map_err(|failure| {
            format!("module manifest rejected (code {})", failure.code())
        })?;
    let id_matches = manifest.package().id().as_str() == id;
    let digest_matches = record.digest.strip_prefix("sha256:")
        == Some(manifest.package().digest().as_str());
    if !id_matches || !digest_matches {
        return Err("module record conflict (code RECORD_CONFLICT)".to_owned());
    }

    if let Some(host) = hosts.get(id) {
        if manifest.component().is_none() {
            return Err(
                "manifest does not name a component; cannot enable without bytes"
                    .to_owned(),
            );
        }
        if host.installed_package() != Some(manifest.package())
            || manifests.get(id) != Some(&manifest)
        {
            return Err(
                "module record conflict (code RECORD_CONFLICT)".to_owned()
            );
        }
        if verify_component(&manifest).is_err() {
            return Err(
                "module record conflict (code RECORD_CONFLICT)".to_owned()
            );
        }
        return hosts.get_mut(id).ok_or_else(|| {
            "module record conflict (code RECORD_CONFLICT)".to_owned()
        });
    }

    let component = match manifest.component() {
        Some(component) => component.to_path_buf(),
        None => {
            return Err(
                "manifest does not name a component; cannot enable without bytes"
                    .to_owned(),
            );
        }
    };
    let authority = HostAuthority::parse(&[]).map_err(|failure| {
        format!("module installation rejected (code {})", failure.code())
    })?;
    let mut pending_host = ModuleHost::new(
        manifest.package().abi().clone(),
        authority,
        component,
        workspace_root.to_path_buf(),
        ModuleHostBounds::default(),
    );
    if let Err(failure) = pending_host.install(manifest.package().clone()) {
        return Err(format!(
            "module installation rejected (code {})",
            failure.code()
        ));
    }
    manifests.insert(id.to_owned(), manifest);
    hosts.insert(id.to_owned(), pending_host);
    Ok(hosts.get_mut(id).expect("just inserted"))
}

pub(super) fn render_enable_module(
    workspace_root: &Path,
    hosts: &mut BTreeMap<String, ModuleHost>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
    id: &str,
    profile_modules: Option<&[String]>,
) -> String {
    let id = id.trim();
    if id.is_empty() {
        return "Enable failed: module id is required (code PATH_EMPTY)\n"
            .to_owned();
    }
    let sanitized = sanitize_for_display(id);
    // A workspace profile may narrow enablement. Refuse an unselected id
    // before reconstructing the host or reading component bytes.
    if let Some(selected) = profile_modules
        && !selected.iter().any(|module| module == &sanitized)
    {
        return "Enable failed: module is outside the applied profile selection\n"
            .to_owned();
    }
    let host = match ensure_host(workspace_root, &sanitized, hosts, manifests)
    {
        Ok(host) => host,
        Err(reason) => return format!("Enable failed: {reason}\n"),
    };
    match host.enable() {
        Ok(()) => format!("Enabled {sanitized}.\n"),
        Err(failure) => {
            format!(
                "Enable failed: {} (code {})\n",
                failure.code(),
                failure.code()
            )
        }
    }
}

pub(super) fn render_activate_module(
    _workspace_root: &Path,
    hosts: &mut BTreeMap<String, ModuleHost>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
    id: &str,
    profile_modules: Option<&[String]>,
) -> String {
    let id = id.trim();
    if id.is_empty() {
        return "Activate failed: module id is required (code PATH_EMPTY)\n"
            .to_owned();
    }
    let sanitized = sanitize_for_display(id);
    // Gate against the actual Host-owned enabled set before loading or
    // installing a component. A profile may narrow this set, never
    // manufacture an enabled module.
    let enabled_ids: Vec<String> = hosts
        .iter()
        .filter_map(|(module_id, host)| {
            matches!(
                host.state(),
                LifecycleState::Enabled | LifecycleState::Active
            )
            .then_some(module_id.clone())
        })
        .collect();
    let gate =
        decide_module_activation(&enabled_ids, profile_modules, &sanitized);
    if let Some(reason) = &gate.reason {
        return format!("Activate failed: {reason}\n");
    }

    let manifest = match manifests.get(&sanitized) {
        Some(manifest) => manifest,
        None => {
            return format!(
                "Activate failed: manifest not loaded for {sanitized}\n"
            );
        }
    };
    let package = manifest.package().clone();
    let capabilities: Vec<String> = package
        .requested_capabilities()
        .iter()
        .map(|cap| cap.as_str().to_owned())
        .collect();
    let request = match ActivationRequest::parse(
        package.id().as_str(),
        package.digest().as_str(),
        package.abi().as_str(),
        &capabilities,
    ) {
        Ok(request) => request,
        Err(failure) => {
            return format!("Activate failed: {}\n", failure.code());
        }
    };
    let host = match hosts.get_mut(&sanitized) {
        Some(host) => host,
        None => {
            return format!(
                "Activate failed: module {sanitized} is not enabled by the Host\n"
            );
        }
    };
    match host.activate(request, RuntimeCheckResult::Ready) {
        Ok(_) => format!("Activated {sanitized}.\n"),
        Err(failure) => {
            format!("Activate failed: {}\n", failure.code())
        }
    }
}

/// Stable short code for a workspace path rejection.
fn rejection_code(
    rejection: &siralos_adapters::workspace::resolve::PathRejection,
) -> &'static str {
    use siralos_adapters::workspace::resolve::PathRejection as Rejection;
    match rejection {
        Rejection::NullByte => "PATH_NULL_BYTE",
        Rejection::Empty => "PATH_EMPTY",
        Rejection::Absolute => "PATH_ABSOLUTE",
        Rejection::InvalidCharacter => "PATH_INVALID_CHARACTER",
        Rejection::OutsideWorkspace => "PATH_OUTSIDE_WORKSPACE",
        Rejection::Unresolvable(_) => "PATH_UNRESOLVABLE",
        Rejection::LinkEscape => "PATH_LINK_ESCAPE",
    }
}
