//! Workspace profile configuration persistence: bounded reads,
//! observed-byte-consumption writes, model switching, and removal.

use super::{
    BoundedFileRead, EffectiveRunPolicy, LockModuleIdentity, LockVerification,
    LockVerificationDecision, Path, SessionProvider, StoredLockDigest,
    WorkspaceProfileLoad, WorkspaceProfileWriteToken,
    create_effective_policy_evidence, create_workspace_lock,
    decide_lock_verification, display_field_redacted, io, load_module_records,
    load_workspace_profile_write_token, read_complete_file_bounded,
    verify_workspace_lock,
};

/// Stage 5.9 (decision 55): verify the on-disk `siralos.lock` against
/// the recomputed current lock. The current lock is recomputed from the
/// applied profile's effective-policy identity and the installed module
/// records; the on-disk lock is read through the unchanged 5.4 adapter.
/// The lock never gates authority: every outcome is advisory and the
/// session proceeds on live Host state.
///
/// Decision 114 Q5 — authority-only lock (documented as intentional):
/// the session lock covers AUTHORITY identity (effective policy, modules);
/// provider/model/endpoint are routing configuration set by the workspace
/// owner and intentionally do not drift the lock.
pub(super) fn verify_session_lock(
    workspace_root: &Path,
    effective: &EffectiveRunPolicy,
) -> LockVerificationDecision {
    let lock_profile_digest: Option<String> =
        if effective.applied_profile.is_some() {
            create_effective_policy_evidence(effective)
                .ok()
                .map(|evidence| evidence.effective_digest)
        } else {
            None
        };
    let lock_identities: Vec<LockModuleIdentity> =
        match load_module_records(workspace_root) {
            Ok(records) => records
                .iter()
                .map(|record| LockModuleIdentity {
                    id: record.id.clone(),
                    path: record.path.clone(),
                    digest: record
                        .digest
                        .strip_prefix("sha256:")
                        .unwrap_or(&record.digest)
                        .to_owned(),
                })
                .collect(),
            Err(_) => {
                // The recomputation itself is compromised: the stored
                // lock cannot be held to account against a current state
                // the session cannot see.
                return decide_lock_verification(
                    StoredLockDigest::Untrusted(
                        "the workspace module records could not be read"
                            .to_owned(),
                    ),
                    "",
                );
            }
        };
    match create_workspace_lock(
        lock_profile_digest.as_deref(),
        &lock_identities,
    ) {
        Ok(current_lock) => {
            let stored =
                match verify_workspace_lock(workspace_root, &current_lock) {
                    Ok(LockVerification::Missing) => StoredLockDigest::Missing,
                    Ok(LockVerification::Current) => {
                        StoredLockDigest::Trusted(
                            current_lock.lock_digest.clone(),
                        )
                    }
                    Ok(LockVerification::Stale { actual, .. }) => {
                        StoredLockDigest::Trusted(actual)
                    }
                    Err(failure) => {
                        StoredLockDigest::Untrusted(failure.message)
                    }
                };
            decide_lock_verification(stored, &current_lock.lock_digest)
        }
        Err(error) => decide_lock_verification(
            StoredLockDigest::Untrusted(error.message),
            "",
        ),
    }
}

#[cfg(test)]
static SCRATCH_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
pub(super) fn unique_scratch_name(prefix: &str) -> String {
    use std::sync::atomic::Ordering;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{nonce:x}-{sequence}", std::process::id())
}

fn profile_snapshot_unchanged(
    path: &Path,
    observed: &WorkspaceProfileWriteToken,
) -> Result<bool, String> {
    let bytes = match read_profile_bytes_bounded(
        path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(reason) => return Err(reason),
    };
    Ok(observed.matches_path(path, bytes.as_deref()))
}

pub(super) fn read_profile_bytes_bounded(
    path: &Path,
    maximum: usize,
) -> Result<Vec<u8>, String> {
    match read_complete_file_bounded(path, maximum) {
        BoundedFileRead::Complete(bytes) => Ok(bytes),
        BoundedFileRead::TooLarge => {
            Err("profile document exceeds the byte bound".to_owned())
        }
        BoundedFileRead::NotReadable => {
            // The bounded reader collapses "cannot be lstat'ed" into one arm,
            // and a MISSING profile is an ordinary state here: the first
            // `/provider` add creates the file, and a removal of an absent
            // profile is a documented no-op. Distinguish absence by probing
            // the metadata directly; every other cause stays a refusal.
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Err("profile target is absent".to_owned())
                }
                _ => Err("profile target must be a regular file".to_owned()),
            }
        }
        BoundedFileRead::IoError(error)
            if error.kind() == io::ErrorKind::NotFound =>
        {
            Err("profile target is absent".to_owned())
        }
        BoundedFileRead::IoError(_) => {
            Err("profile target could not be read".to_owned())
        }
    }
}

/// Write the `[profile]` section after observing the current profile revision.
///
/// This convenience boundary is safe for callers that do not already hold a
/// snapshot: it obtains one immediately before delegating to the
/// snapshot-bound writer. Callers that read a profile to build a mutation MUST
/// use [`write_profile_config_at`] with that exact observation instead.
pub fn write_profile_config(
    workspace_root: &Path,
    provider: &str,
    model: &str,
    credential_env: Option<&str>,
    endpoint: Option<&str>,
    protocol: Option<&str>,
    model_display_name: Option<&str>,
) -> Result<(), String> {
    let observed = load_workspace_profile_write_token(workspace_root)
        .ok_or_else(|| {
            "profile snapshot is unavailable; reload before retrying"
                .to_owned()
        })?;
    write_profile_config_at(
        workspace_root,
        &observed,
        provider,
        model,
        credential_env,
        endpoint,
        protocol,
        model_display_name,
    )
}

/// Write the `[profile]` section atomically with format-preserving merge
/// (C2) — the fifth atomic writer (per decision 114 Q4). The credential is
/// stored verbatim as given (`env:NAME`, `key:VALUE`, or a bare legacy env
/// name). The written bytes are verified through the bounded adapter parser
/// before the rename; symlinked/non-regular targets are refused per the
/// manifest pattern; the temp is deleted on validation failure.
///
/// `observed` is the exact profile revision used to construct this mutation.
/// The writer re-reads the target, refuses any drift before parsing, and uses
/// the same revision for the final compare-and-swap commit.
#[allow(clippy::too_many_arguments)]
pub fn write_profile_config_at(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
    provider: &str,
    model: &str,
    credential_env: Option<&str>,
    endpoint: Option<&str>,
    protocol: Option<&str>,
    model_display_name: Option<&str>,
) -> Result<(), String> {
    observed.consume()?;
    write_profile_config_at_after_consumption(
        workspace_root,
        observed,
        provider,
        model,
        credential_env,
        endpoint,
        protocol,
        model_display_name,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_profile_config_at_after_consumption(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
    provider: &str,
    model: &str,
    credential_env: Option<&str>,
    endpoint: Option<&str>,
    protocol: Option<&str>,
    model_display_name: Option<&str>,
) -> Result<(), String> {
    // Re-validate at the write boundary (defense in depth).
    if !siralos_core::composition::is_provider_id(provider) {
        return Err("A provider must match [a-z0-9_-]{1,64}.".to_owned());
    }
    if !siralos_core::composition::is_model_id(model) {
        return Err(
            "A model must match [a-zA-Z0-9._/:@-]{1,256} with no NUL."
                .to_owned(),
        );
    }
    if let Some(cred) = credential_env {
        // Verbatim credential: accept env:NAME, key:VALUE, or bare legacy env name. Validation mirrors ProfileRecord.
        if let Some(name) = cred.strip_prefix("env:") {
            if !siralos_core::composition::is_credential_env_name(name) {
                return Err(
                    "A credential env name must match [A-Z0-9_]{1,64} after \"env:\"."
                        .to_owned(),
                );
            }
            if cred.len()
                > siralos_core::composition::MAX_PROFILE_CREDENTIAL_BYTES
            {
                return Err(format!(
                    "The credential exceeds the {}-byte bound.",
                    siralos_core::composition::MAX_PROFILE_CREDENTIAL_BYTES
                ));
            }
        } else if let Some(inner) = cred.strip_prefix("key:") {
            if inner.is_empty() {
                return Err(
                    "A credential key: value must be non-empty.".to_owned()
                );
            }
            if inner.contains('\0') {
                return Err("A credential must not contain NUL.".to_owned());
            }
            if inner.len()
                > siralos_core::composition::MAX_PROFILE_CREDENTIAL_KEY_BYTES
            {
                return Err(format!(
                    "The credential key value exceeds the {}-byte bound.",
                    siralos_core::composition::MAX_PROFILE_CREDENTIAL_KEY_BYTES
                ));
            }
        } else {
            // Bare legacy compat — treat as env name.
            if !siralos_core::composition::is_credential_env_name(cred) {
                return Err(
                    "A credential env name must match [A-Z0-9_]{1,64} after \"env:\"."
                        .to_owned(),
                );
            }
        }
        if cred.contains('\0') {
            return Err("A credential must not contain NUL.".to_owned());
        }
    }
    if let Some(proto) = protocol {
        if proto != "openai-completions"
            && proto != "openai-responses"
            && proto != "anthropic-messages"
        {
            return Err(
                "The protocol must be \"openai-completions\", \"openai-responses\", or \"anthropic-messages\"."
                    .to_owned(),
            );
        }
    }
    if let Some(display) = model_display_name {
        if !display.is_empty() {
            if display.len()
                > siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
            {
                return Err(format!(
                    "The model display name exceeds the {}-byte bound.",
                    siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
                ));
            }
            if display.contains('\0') {
                return Err(
                    "A model display name must not contain NUL.".to_owned()
                );
            }
            if !siralos_core::composition::is_printable(display) {
                return Err(
                    "A model display name must be printable.".to_owned()
                );
            }
        }
    }
    if let Some(ep) = endpoint {
        // One sequential clause check with an early return each: the previous
        // shape wrapped these in an outer `if` that re-evaluated the same five
        // conditions to choose a message, and its last arm was an unconditional
        // `return Err` — so any later edit that weakened the outer condition
        // would have reported "must not contain spaces" for a valid endpoint.
        if ep.is_empty()
            || ep.len() > siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
        {
            return Err(format!(
                "The endpoint exceeds the {}-byte bound or is empty.",
                siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
            ));
        }
        if ep.contains('\0') {
            return Err("An endpoint must not contain NUL.".to_owned());
        }
        if !siralos_core::composition::has_http_scheme(ep) {
            return Err(
                "An endpoint must start with \"https://\" or \"http://\"."
                    .to_owned(),
            );
        }
        if ep.contains(' ') {
            return Err("An endpoint must not contain spaces.".to_owned());
        }
        if !siralos_core::composition::is_valid_http_endpoint(ep) {
            return Err(
                "An endpoint must have a valid HTTP(S) authority without userinfo, fragment, or controls."
                    .to_owned(),
            );
        }
    }
    let path = workspace_root
        .join(siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME);
    // Read the current bytes once, then bind the merge to the caller's exact
    // observation before parsing or serializing anything. The final atomic
    // commit below repeats the same identity check for the stage-to-swap
    // window.
    let current_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(reason) => return Err(reason),
    };
    if !observed.matches_path(&path, current_bytes.as_deref()) {
        return Err(
            "siralos.toml changed concurrently; reload before retrying"
                .to_owned(),
        );
    }
    let existing: Option<String> = current_bytes
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| "siralos.toml is not valid UTF-8".to_owned())
        })
        .transpose()?;
    // Format-preserving parse via toml_edit.
    let mut doc: toml_edit::DocumentMut = if let Some(ref text) = existing {
        if text.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            text.parse::<toml_edit::DocumentMut>()
                .map_err(|_| "siralos.toml does not parse".to_owned())?
        }
    } else {
        toml_edit::DocumentMut::new()
    };
    // Ensure [profile] is a table with a name when absent (name is required
    // for the profile to parse). Fail closed on a non-table [profile]:
    // never silently rewrite a shape-violating document.
    match doc.get("profile") {
        Some(item) if item.is_table() || item.is_inline_table() => {}
        Some(_) => {
            return Err("The [profile] entry must be a table.".to_owned());
        }
        None => {
            doc["profile"] = toml_edit::table();
        }
    }
    // Only set when absent — preserve an existing name byte-for-byte.
    // Navigated defensively: a fresh document has no [profile] yet, and the
    // chained immutable index panics on missing intermediates.
    let needs_name = doc
        .get("profile")
        .and_then(|item| item.as_table_like())
        .map(|table| table.get("name").is_none())
        .unwrap_or(true);
    if needs_name {
        if let Some(profile_item) = doc.get_mut("profile") {
            if let Some(table) = profile_item.as_table_like_mut() {
                table.insert("name", toml_edit::value("default"));
            }
        }
    }
    // Merge profile fields.
    doc["profile"]["provider"] = toml_edit::value(provider);
    doc["profile"]["model"] = toml_edit::value(model);
    // Credential: verbatim — written as given (env:X stays env:X; key:X written as key:X).
    if let Some(cred) = credential_env {
        doc["profile"]["credential"] = toml_edit::value(cred);
    } else if let Some(profile_item) = doc.get_mut("profile") {
        if let Some(table) = profile_item.as_table_like_mut() {
            table.remove("credential");
        }
    }
    if let Some(ep) = endpoint {
        doc["profile"]["endpoint"] = toml_edit::value(ep);
    } else {
        // Remove endpoint key if present (optional).
        if let Some(profile_item) = doc.get_mut("profile") {
            if let Some(table) = profile_item.as_table_like_mut() {
                table.remove("endpoint");
            }
        }
    }
    // Protocol: written only when not default (openai-completions omitted).
    if let Some(proto) = protocol {
        if proto != "openai-completions" {
            doc["profile"]["protocol"] = toml_edit::value(proto);
        } else if let Some(profile_item) = doc.get_mut("profile") {
            if let Some(table) = profile_item.as_table_like_mut() {
                table.remove("protocol");
            }
        }
    } else if let Some(profile_item) = doc.get_mut("profile") {
        if let Some(table) = profile_item.as_table_like_mut() {
            table.remove("protocol");
        }
    }
    // Model display name: written only when non-empty.
    if let Some(display) = model_display_name {
        if !display.is_empty() {
            doc["profile"]["model_display_name"] = toml_edit::value(display);
        } else if let Some(profile_item) = doc.get_mut("profile") {
            if let Some(table) = profile_item.as_table_like_mut() {
                table.remove("model_display_name");
            }
        }
    } else if let Some(profile_item) = doc.get_mut("profile") {
        if let Some(table) = profile_item.as_table_like_mut() {
            table.remove("model_display_name");
        }
    }
    let serialized = doc.to_string();
    if serialized.len()
        > siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES
    {
        return Err("siralos.toml exceeds the byte bound".to_owned());
    }
    // Stage through the shared atomic writer: an exclusive temporary file in the
    // target's own directory, RAII cleanup on every failure, and a commit that
    // refuses a symlinked or special-file target.
    let staged = siralos_adapters::atomic::stage_atomic(
        workspace_root,
        siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME,
        &format!(
            "{}siralos-toml",
            siralos_adapters::workspace::fs::MUTATION_TEMP_PREFIX
        ),
        serialized.as_bytes(),
        Some(0o600),
    )
    .map_err(|_| "profile could not be staged".to_owned())?;
    // Verify the exact staged bytes through the same parser used at startup.
    // No second filesystem copy is made: a literal credential must never be
    // written to an unrelated temporary directory merely to validate it.
    let verify_bytes = read_profile_bytes_bounded(
        staged.path(),
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    )?;
    match siralos_adapters::profile_config::parse_workspace_profile_bytes(
        &verify_bytes,
    ) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            if record.validate().is_err() {
                return Err(
                    "written profile failed validation (details hidden)"
                        .to_owned(),
                );
            }
            // Ensure the applied record carries every written value (verbatim
            // credential; comparison is local and never rendered).
            let expected_credential = credential_env.map(str::to_owned);
            let expected_protocol = protocol.unwrap_or("openai-completions");
            let expected_display = model_display_name
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if record.provider.as_deref() != Some(provider)
                || record.model.as_deref() != Some(model)
                || record.credential.as_deref()
                    != expected_credential.as_deref()
                || record.endpoint.as_deref() != endpoint
                || record.protocol.as_str() != expected_protocol
                || record.model_display_name != expected_display
            {
                return Err(
                    "written profile did not apply the requested fields"
                        .to_owned(),
                );
            }
        }
        siralos_adapters::profile_config::WorkspaceProfileLoad::Invalid {
            ..
        } => return Err("written profile invalid (details hidden)".to_owned()),
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {
            return Err("written profile did not apply the requested fields"
                .to_owned());
        }
    }
    // Revalidate the exact filesystem revision once more after staging and
    // validating the candidate. A content-only check would accept an A→B→A
    // replacement; the token's identity evidence and one-shot authority make
    // that sequence fail closed before the pathname swap.
    let final_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(_reason) => {
            return Err(
                "profile could not be revalidated; reload before retrying"
                    .to_owned(),
            );
        }
    };
    if !observed.matches_path(&path, final_bytes.as_deref()) {
        return Err(
            "siralos.toml changed concurrently; reload before retrying"
                .to_owned(),
        );
    }
    let observed_digest =
        final_bytes.as_deref().map(siralos_core::identity::sha256_hex);
    let commit_result = if let Some(digest) = observed_digest.as_deref() {
        staged.commit_if_digest(digest)
    } else {
        staged.commit_if_absent()
    };
    commit_result.map_err(|error| match error {
        siralos_adapters::atomic::AtomicWriteFailure::TargetChanged { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::ParentChanged {
            ..
        } => "siralos.toml changed concurrently; reload before retrying"
            .to_owned(),
        siralos_adapters::atomic::AtomicWriteFailure::TargetIsNotARegularFile {
            ..
        } => "siralos.toml must be a regular file; refusing symlink or special file"
            .to_owned(),
        siralos_adapters::atomic::AtomicWriteFailure::TargetUnreadable { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::ReplaceFailed { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::Staged { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::StagedIdentityUnverifiable {
            ..
        } => "profile commit failed (details hidden)".to_owned(),
    })?;
    Ok(())
}

/// Validate a candidate live model id with the core predicate
/// (`siralos_core::composition::is_model_id`: 1..=256 bytes, no NUL, the
/// `is_model_id_char` set).
/// Refuses with the existing honest write-boundary message.
fn validate_live_model_id(id: &str) -> Result<(), String> {
    if !siralos_core::composition::is_model_id(id) {
        return Err(
            "A model must match [a-zA-Z0-9._/:@-]{1,256} with no NUL."
                .to_owned(),
        );
    }
    Ok(())
}

/// Persist a live `/model` switch after obtaining a fresh profile
/// observation. Callers that retain a revision across an approval boundary
/// should use [`persist_switched_model_at`].
pub fn persist_switched_model(
    workspace_root: &Path,
    applied_provider: Option<&str>,
    new_model: &str,
) -> Result<String, String> {
    validate_live_model_id(new_model)
        .map_err(|reason| format!("{reason}\n"))?;
    let observed = load_workspace_profile_write_token(workspace_root)
        .ok_or_else(|| {
            "model switch refused: profile snapshot is unavailable; reload before switching\n"
                .to_owned()
        })?;
    persist_switched_model_at(
        workspace_root,
        &observed,
        applied_provider,
        new_model,
    )
}

/// Persist a live `/model` switch against the exact profile revision the
/// caller observed. Only the model changes; the previous display name is
/// cleared. The bounded writer is reused for validation, preservation,
/// staging, and final compare-and-swap.
pub fn persist_switched_model_at(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
    applied_provider: Option<&str>,
    new_model: &str,
) -> Result<String, String> {
    observed.consume()?;
    validate_live_model_id(new_model)
        .map_err(|reason| format!("{reason}\n"))?;
    if applied_provider.is_none_or(|provider| provider.is_empty()) {
        return Err(
            "no provider configured — cannot switch model without an applied [profile]\n"
                .to_owned(),
        );
    }
    let path = workspace_root
        .join(siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME);
    let current_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(_reason) => {
            return Err(
                "model switch refused: profile could not be revalidated\n"
                    .to_owned(),
            );
        }
    };
    if !observed.matches_path(&path, current_bytes.as_deref()) {
        return Err(
            "model switch refused: profile changed concurrently; reload before switching\n"
                .to_owned(),
        );
    }
    let record = match current_bytes
        .as_deref()
        .map(siralos_adapters::profile_config::parse_workspace_profile_bytes)
    {
        Some(WorkspaceProfileLoad::Record(record)) => record,
        _ => {
            return Err(
                "no provider configured — cannot switch model without an applied [profile]\n"
                    .to_owned(),
            );
        }
    };
    if record.validate().is_err() {
        return Err(
            "model switch refused: profile is invalid (details hidden)\n"
                .to_owned(),
        );
    }
    if record.provider.as_deref() != applied_provider {
        return Err(
            "model switch refused: profile provider changed; reload before switching\n"
                .to_owned(),
        );
    }
    let provider = match record.provider.as_deref() {
        Some(provider) if !provider.is_empty() => provider.to_owned(),
        _ => {
            return Err(
                "no provider configured — cannot switch model without an applied [profile]\n"
                    .to_owned(),
            );
        }
    };
    write_profile_config_at_after_consumption(
        workspace_root,
        observed,
        &provider,
        new_model,
        record.credential.as_deref(),
        record.endpoint.as_deref(),
        Some(record.protocol.as_str()),
        None,
    )
    .map_err(|reason| format!("model switch failed: {reason}\n"))?;
    let safe_model =
        display_field_redacted(Some(new_model), record.credential.as_deref());
    Ok(format!(
        "model switched to {safe_model} — model display name cleared\n"
    ))
}

/// Perform the full live switch: validate + persist through
/// [`persist_switched_model`], then update the live provider cell (so the
/// NEXT provider request uses the new id) and the session display holders
/// in place. The provider cell is only touched after the persist succeeds,
/// so a refused switch changes neither disk nor the live session. Returns
/// the user-facing message from [`persist_switched_model`].
pub(super) fn apply_model_switch(
    workspace_root: &Path,
    live_provider: &SessionProvider,
    applied_provider: Option<&str>,
    applied_model: &mut Option<String>,
    applied_model_display_name: &mut Option<String>,
    new_model: &str,
) -> Result<String, String> {
    if !live_provider.supports_live_model() {
        return Err(
            "model switch unavailable for this provider mode; restart required\n"
                .to_owned(),
        );
    }
    let message =
        persist_switched_model(workspace_root, applied_provider, new_model)?;
    if !live_provider.set_live_model(new_model) {
        return Err(
            "model switch was not applied; disk changed but live route did not\n"
                .to_owned(),
        );
    }
    *applied_model = Some(new_model.to_owned());
    *applied_model_display_name = None;
    Ok(message)
}

/// Remove provider-owned fields after obtaining a fresh profile observation.
///
/// Callers that already observed the profile must use
/// [`remove_profile_config_at`] so a concurrent edit cannot be silently
/// removed along with the provider.
pub fn remove_profile_config(workspace_root: &Path) -> Result<(), String> {
    // Name the rule BEFORE taking a snapshot: a symlinked or non-regular
    // target is refused for what it IS. Reporting it as an "unavailable
    // snapshot" would send the user looking for a reload that cannot help.
    let path = workspace_root
        .join(siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("profile target must be a regular file".to_owned());
        }
        Ok(_) => {}
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err("profile target could not be read".to_owned());
        }
        Err(_) => {}
    }
    let observed = load_workspace_profile_write_token(workspace_root)
        .ok_or_else(|| {
            "profile snapshot is unavailable; reload before retrying"
                .to_owned()
        })?;
    remove_profile_config_at(workspace_root, &observed)
}

/// Remove provider-owned fields atomically (provider deletion) — the
/// sixth atomic writer, reusing the fifth's pattern beside
/// [`write_profile_config_at`]. Policy, module, context, and skills fields
/// inside `[profile]` are preserved. A symlinked or non-regular target is
/// refused with the write path's diagnostic; the temp is deleted on failure.
///
/// A missing file, an empty file, or a file with no `[profile]` table
/// holds no provider: succeed WITHOUT rewriting anything only when the
/// caller's token still describes that exact state.
pub fn remove_profile_config_at(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
) -> Result<(), String> {
    observed.consume()?;
    let path = workspace_root
        .join(siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME);
    // Read current bytes, then bind every subsequent decision to the exact
    // observation supplied by the caller. This catches drift that happened
    // while a confirmation modal was open, before the writer's own stage.
    let current_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(reason) => return Err(reason),
    };
    if !observed.matches_path(&path, current_bytes.as_deref()) {
        return Err(
            "siralos.toml changed concurrently; reload before retrying"
                .to_owned(),
        );
    }
    let Some(bytes) = current_bytes else {
        return Ok(());
    };
    let existing = String::from_utf8(bytes)
        .map_err(|_| "siralos.toml is not valid UTF-8".to_owned())?;
    // Format-preserving parse via toml_edit (write-path diagnostic).
    let mut doc: toml_edit::DocumentMut = if existing.trim().is_empty() {
        if !profile_snapshot_unchanged(&path, observed)? {
            return Err(
                "siralos.toml changed concurrently; reload before retrying"
                    .to_owned(),
            );
        }
        return Ok(());
    } else {
        existing
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| "siralos.toml does not parse".to_owned())?
    };
    if doc.get("profile").is_none() {
        if !profile_snapshot_unchanged(&path, observed)? {
            return Err(
                "siralos.toml changed concurrently; reload before retrying"
                    .to_owned(),
            );
        }
        return Ok(());
    }
    // Remove only provider-owned fields. Policy, module selection, context
    // narrowing, and skills are independent authority/state and must survive
    // provider removal.
    let provider_keys = [
        "provider",
        "model",
        "model_display_name",
        "endpoint",
        "protocol",
        "credential",
        "record-replay",
        "replay",
    ];
    let remove_profile = {
        let Some(item) = doc.get_mut("profile") else {
            if !profile_snapshot_unchanged(&path, observed)? {
                return Err(
                    "siralos.toml changed concurrently; reload before retrying"
                        .to_owned(),
                );
            }
            return Ok(());
        };
        let Some(table) = item.as_table_like_mut() else {
            return Err(
                "profile must be a TOML table; refusing provider removal"
                    .to_owned(),
            );
        };
        for key in provider_keys {
            table.remove(key);
        }
        // A table that now holds nothing but its own name is not a profile
        // any more -- it names a provider that no longer exists -- so it goes
        // with the provider. Anything else the user declared (policy, module
        // selection, context narrowing, skills) keeps the table alive.
        table.iter().all(|(key, _)| key == "name")
    };
    if remove_profile {
        doc.remove("profile");
    }
    let serialized = doc.to_string();
    if serialized.len()
        > siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES
    {
        return Err("siralos.toml exceeds the byte bound".to_owned());
    }
    // The write path's staging verbatim, through the shared atomic writer.
    let staged = siralos_adapters::atomic::stage_atomic(
        workspace_root,
        siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME,
        &format!(
            "{}siralos-toml",
            siralos_adapters::workspace::fs::MUTATION_TEMP_PREFIX
        ),
        serialized.as_bytes(),
        Some(0o600),
    )
    .map_err(|_| "profile could not be staged".to_owned())?;
    // Verify the exact staged bytes without copying secret-bearing profile
    // text into a second temporary directory.
    let verify_bytes = read_profile_bytes_bounded(
        staged.path(),
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    )?;
    match siralos_adapters::profile_config::parse_workspace_profile_bytes(
        &verify_bytes,
    ) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {}
        siralos_adapters::profile_config::WorkspaceProfileLoad::Invalid {
            diagnostic,
        } => {
            let _ = diagnostic;
            return Err(
                "removed config is invalid; refusing provider removal"
                    .to_owned(),
            );
        }
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            if record.validate().is_err() {
                return Err(
                    "remaining profile is invalid; refusing provider removal"
                        .to_owned(),
                );
            }
            if record.provider.is_some()
                || record.model.is_some()
                || record.model_display_name.is_some()
                || record.endpoint.is_some()
                || record.credential.is_some()
                || record.record_replay
                || record.replay
            {
                return Err(
                    "provider fields remain after removal; refusing to replace siralos.toml"
                        .to_owned(),
                );
            }
        }
    }
    let final_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(_reason) => {
            return Err(
                "profile could not be revalidated; reload before retrying"
                    .to_owned(),
            );
        }
    };
    if !observed.matches_path(&path, final_bytes.as_deref()) {
        return Err(
            "siralos.toml changed concurrently; reload before retrying"
                .to_owned(),
        );
    }
    let observed_digest =
        final_bytes.as_deref().map(siralos_core::identity::sha256_hex);
    let Some(observed_digest) = observed_digest else {
        return Err(
            "profile could not be revalidated; reload before retrying"
                .to_owned(),
        );
    };
    let commit_result = staged.commit_if_digest(&observed_digest);
    commit_result.map_err(|error| match error {
        siralos_adapters::atomic::AtomicWriteFailure::TargetChanged { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::ParentChanged {
            ..
        } => "siralos.toml changed concurrently; reload before retrying"
            .to_owned(),
        siralos_adapters::atomic::AtomicWriteFailure::TargetIsNotARegularFile {
            ..
        } => "siralos.toml must be a regular file; refusing symlink or special file"
            .to_owned(),
        siralos_adapters::atomic::AtomicWriteFailure::TargetUnreadable { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::ReplaceFailed { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::Staged { .. }
        | siralos_adapters::atomic::AtomicWriteFailure::StagedIdentityUnverifiable {
            ..
        } => "profile commit failed (details hidden)".to_owned(),
    })?;
    Ok(())
}
