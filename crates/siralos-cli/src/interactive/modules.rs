//! Declarative module application: read `[modules.<id>]` declarations,
//! resolve their manifests, and reconcile the Host's in-memory lifecycle
//! state. There is no persistent install registry beyond `siralos.toml`;
//! removing a declaration and rebuilding is the removal operation.

use std::collections::BTreeSet;

use super::{
    ActivationRequest, BTreeMap, CapabilityGrant, HostAuthority,
    LifecycleState, ModuleFailure, ModuleHost, ModuleHostBounds,
    ModuleLifecycle, ModuleManifest, ModuleRecord, Path, RuntimeCheckResult,
    format_modules, load_module_declarations, resolve_module_declaration,
    sanitize_for_display,
};

/// One declarative apply pass. Lines are user-facing success/change
/// observations; failures are startup notices or `/rebuild` diagnostics.
#[derive(Debug, Default)]
pub(super) struct DeclaredModuleOutcome {
    /// Resolved records, including modules whose apply step failed. Lock
    /// verification uses this list so a failed activation still leaves a
    /// truthful identity to compare.
    pub(super) records: Vec<ModuleRecord>,
    /// Declarations successfully resolved this pass.
    pub(super) declared: usize,
    /// Modules currently applied (unchanged or newly successful).
    pub(super) applied: usize,
    /// Human-readable changes, deterministic by module id.
    pub(super) lines: Vec<String>,
    /// Truthful failures, deterministic by module id.
    pub(super) failures: Vec<String>,
}

impl DeclaredModuleOutcome {
    /// Render the `/rebuild` report. Startup only consumes `failures`
    /// through the existing startup-notice path.
    #[must_use]
    pub(super) fn render(&self) -> String {
        if self.declared == 0
            && self.failures.is_empty()
            && self.lines.is_empty()
        {
            return "No modules declared.\n".to_owned();
        }
        let mut out = String::new();
        if self.lines.is_empty() && self.failures.is_empty() {
            out.push_str("No module changes.\n");
        }
        for line in &self.lines {
            out.push_str(&sanitize_for_display(line));
            out.push('\n');
        }
        for failure in &self.failures {
            out.push_str(&sanitize_for_display(failure));
            out.push('\n');
        }
        out.push_str(&format!(
            "{} declared, {} applied, {} failed\n",
            self.declared,
            self.applied,
            self.failures.len()
        ));
        out
    }
}

/// Render the `/modules` declaration view. Runtime state is deliberately
/// not claimed here: the next rebuild is the source of truth.
pub(super) fn render_modules(workspace_root: &Path) -> String {
    match load_module_declarations(workspace_root) {
        Ok(loaded) => {
            let mut rendered = format_modules(&loaded.declarations);
            for failure in &loaded.failures {
                rendered.push_str(&format!(
                    "  {} (declaration rejected, code {})\n",
                    sanitize_for_display(&failure.id),
                    failure.failure.code()
                ));
            }
            rendered
        }
        Err(failure) => {
            format!(
                "Modules unavailable: {failure} (code {})\n",
                failure.code()
            )
        }
    }
}

/// Apply the desired `[modules.<id>]` set to the session's live maps.
///
/// Resolution happens before any mutation, so a malformed replacement can
/// never tear down a working module. A failed apply keeps the previous
/// working state for that id. Modules whose declaration failed to resolve
/// are "blocked": their previous state is retained rather than removed.
pub(super) fn apply_declared_modules(
    workspace_root: &Path,
    hosts: &mut BTreeMap<String, ModuleHost>,
    hostless: &mut BTreeMap<String, ModuleLifecycle>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
) -> DeclaredModuleOutcome {
    let mut outcome = DeclaredModuleOutcome::default();
    let loaded = match load_module_declarations(workspace_root) {
        Ok(loaded) => loaded,
        Err(failure) => {
            outcome.failures.push(format!(
                "module declarations rejected: {failure} (code {})",
                failure.code()
            ));
            return outcome;
        }
    };
    // "Declared" is every entry the document named, including entries whose
    // shape or resolution failed; "applied" is the subset live now.
    outcome.declared = loaded.declarations.len() + loaded.failures.len();

    // A shape failure is a failed declaration, not a deletion: retain any
    // previous working state for that id exactly as a resolution failure does.
    let mut blocked = BTreeSet::new();
    for failure in loaded.failures {
        blocked.insert(failure.id.clone());
        outcome.failures.push(format!(
            "module {} declaration rejected: {} (code {})",
            sanitize_for_display(&failure.id),
            failure.failure,
            failure.failure.code()
        ));
    }

    let mut desired = BTreeMap::new();
    for declaration in loaded.declarations {
        match resolve_module_declaration(workspace_root, &declaration) {
            Ok(resolved) => {
                outcome.records.push(resolved.record.clone());
                desired.insert(declaration.id, resolved);
            }
            Err(failure) => {
                blocked.insert(declaration.id.clone());
                outcome.failures.push(format!(
                    "module {} not resolved: {} (code {})",
                    sanitize_for_display(&declaration.id),
                    failure,
                    failure.code()
                ));
            }
        }
    }

    // Remove declarations that disappeared. A blocked declaration retains
    // its previous working state so an intermediate edit cannot remove it.
    let current_ids: BTreeSet<String> = hosts
        .keys()
        .chain(hostless.keys())
        .chain(manifests.keys())
        .cloned()
        .collect();
    for id in current_ids {
        if desired.contains_key(&id) || blocked.contains(&id) {
            continue;
        }
        match remove_current(&id, hosts, hostless, manifests) {
            Ok(()) => outcome.lines.push(format!("removed {id}")),
            Err(failure) => outcome.failures.push(format!(
                "module {} could not be removed (code {})",
                sanitize_for_display(&id),
                failure.code()
            )),
        }
    }

    // BTreeMap iteration is the deterministic id order.
    for (id, resolved) in desired {
        match apply_one(
            &id,
            &resolved,
            workspace_root,
            hosts,
            hostless,
            manifests,
        ) {
            Ok(changed) => {
                outcome.applied += 1;
                if let Some(line) = changed {
                    outcome.lines.push(line);
                }
            }
            Err(failure) => outcome.failures.push(format!(
                "module {} not applied (code {})",
                sanitize_for_display(&id),
                failure.code()
            )),
        }
    }

    outcome
}

/// Apply one resolved declaration, returning an optional change line.
fn apply_one(
    id: &str,
    resolved: &super::ResolvedModuleDeclaration,
    workspace_root: &Path,
    hosts: &mut BTreeMap<String, ModuleHost>,
    hostless: &mut BTreeMap<String, ModuleLifecycle>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
) -> Result<Option<String>, ModuleFailure> {
    let manifest = &resolved.manifest;
    let package = manifest.package().clone();
    let is_component = manifest.component().is_some();

    let unchanged = if is_component {
        hosts.get(id).is_some_and(|host| {
            host.installed_package() == Some(&package)
                && manifests.get(id) == Some(manifest)
        })
    } else {
        hostless.get(id).is_some_and(|lifecycle| {
            lifecycle.installed_package() == Some(&package)
                && manifests.get(id) == Some(manifest)
        })
    };

    if unchanged {
        if is_component {
            let host = hosts.get_mut(id).expect("unchanged host");
            ensure_host_active(host, manifest)?;
        } else {
            let lifecycle = hostless.get_mut(id).expect("unchanged lifecycle");
            ensure_spec_enabled(lifecycle)?;
        }
        return Ok(None);
    }

    // Stage the replacement completely before touching the current slot.
    // A component's activation is staged too, so a guest bind failure
    // leaves the previous module untouched.
    let staged = if is_component {
        let mut host = build_host(manifest, workspace_root)?;
        ensure_host_active(&mut host, manifest)?;
        Staged::Host(Box::new(host))
    } else {
        let mut lifecycle = ModuleLifecycle::new();
        lifecycle.install(package.clone())?;
        ensure_spec_enabled(&mut lifecycle)?;
        Staged::Spec(Box::new(lifecycle))
    };

    // Tear down the old slot only after the replacement is ready. If that
    // fails, deactivate the staged host before dropping it so a failed swap
    // cannot leave an unreachable active guest session.
    if let Err(failure) = remove_current(id, hosts, hostless, manifests) {
        if let Staged::Host(mut host) = staged {
            if host.state() == LifecycleState::Active {
                let _ = host.deactivate();
            }
        }
        return Err(failure);
    }

    manifests.insert(id.to_owned(), manifest.clone());
    match staged {
        Staged::Host(host) => {
            hosts.insert(id.to_owned(), *host);
        }
        Staged::Spec(lifecycle) => {
            hostless.insert(id.to_owned(), *lifecycle);
        }
    }
    Ok(Some(format!(
        "applied {} ({})",
        id,
        if is_component { "component" } else { "spec-only" }
    )))
}

enum Staged {
    Host(Box<ModuleHost>),
    Spec(Box<ModuleLifecycle>),
}

fn build_host(
    manifest: &ModuleManifest,
    workspace_root: &Path,
) -> Result<ModuleHost, ModuleFailure> {
    let Some(component_path) = manifest.component() else {
        return Err(ModuleFailure::InvalidInput {
            reason: "manifest does not name a component".to_owned(),
        });
    };
    let authority = HostAuthority::parse(&[])?;
    let mut host = ModuleHost::new(
        manifest.package().abi().clone(),
        authority,
        component_path.to_path_buf(),
        workspace_root.to_path_buf(),
        ModuleHostBounds::default(),
    );
    host.install(manifest.package().clone())?;
    Ok(host)
}

fn ensure_host_active(
    host: &mut ModuleHost,
    manifest: &ModuleManifest,
) -> Result<(), ModuleFailure> {
    match host.state() {
        LifecycleState::Installed => host.enable()?,
        LifecycleState::Enabled => {}
        LifecycleState::Active => return Ok(()),
        LifecycleState::Absent => return Err(ModuleFailure::NotInstalled),
    }
    let package = manifest.package();
    let capabilities: Vec<String> = package
        .requested_capabilities()
        .iter()
        .map(|cap| cap.as_str().to_owned())
        .collect();
    let request = ActivationRequest::parse(
        package.id().as_str(),
        package.digest().as_str(),
        package.abi().as_str(),
        &capabilities,
    )?;
    host.activate(request, RuntimeCheckResult::Ready)?;
    Ok(())
}

fn ensure_spec_enabled(
    lifecycle: &mut ModuleLifecycle,
) -> Result<(), ModuleFailure> {
    match lifecycle.state() {
        LifecycleState::Installed => lifecycle.enable(),
        LifecycleState::Enabled | LifecycleState::Active => Ok(()),
        LifecycleState::Absent => Err(ModuleFailure::NotInstalled),
    }
}

/// Remove one current id from every map. Refuses to drop an active host
/// until deactivation succeeds; on any failure the old state is restored.
fn remove_current(
    id: &str,
    hosts: &mut BTreeMap<String, ModuleHost>,
    hostless: &mut BTreeMap<String, ModuleLifecycle>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
) -> Result<(), ModuleFailure> {
    if let Some(mut host) = hosts.remove(id) {
        if host.state() == LifecycleState::Active {
            if let Err(failure) = host.deactivate() {
                hosts.insert(id.to_owned(), host);
                return Err(failure);
            }
        }
        if let Err(failure) = host.uninstall() {
            hosts.insert(id.to_owned(), host);
            return Err(failure);
        }
        manifests.remove(id);
        return Ok(());
    }
    if let Some(mut lifecycle) = hostless.remove(id) {
        if lifecycle.state() == LifecycleState::Active {
            if let Err(failure) = lifecycle.deactivate() {
                hostless.insert(id.to_owned(), lifecycle);
                return Err(failure);
            }
        }
        if let Err(failure) = lifecycle.uninstall() {
            hostless.insert(id.to_owned(), lifecycle);
            return Err(failure);
        }
    }
    manifests.remove(id);
    Ok(())
}

/// The lifecycle state and Active grant the panels view reports for one
/// module id: component hosts first, spec-only lifecycles second, and
/// `Absent` when neither holds the id (`module_ui_specs` drops both).
#[must_use]
pub(super) fn module_ui_state<'h>(
    id: &str,
    hosts: &'h BTreeMap<String, ModuleHost>,
    hostless: &BTreeMap<String, ModuleLifecycle>,
) -> (LifecycleState, Option<&'h CapabilityGrant>) {
    if let Some(host) = hosts.get(id) {
        return (host.state(), host.active().map(|active| active.grant()));
    }
    if let Some(lifecycle) = hostless.get(id) {
        return (lifecycle.state(), None);
    }
    (LifecycleState::Absent, None)
}
