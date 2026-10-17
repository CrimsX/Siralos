//! The per-execution settings document (`settings`): the offline
//! network posture plus the deny-root-re-allow filesystem grants
//! the supervisor receives, scoped to one absolute workdir.

use std::path::Path;

/// Build the exact per-execution settings document: offline network
/// (`allowedDomains: []`, `deniedDomains: ["*"]` deny-all,
/// `strictAllowlist`) plus deny-root-re-allow filesystem grants
/// around the confined workdir (readable and writable; everything
/// else unreadable and unwritable). `None` when the workdir is not
/// absolute — settings paths resolve against the supervisor's cwd,
/// so a relative grant would mis-scope, and refusing is the only
/// safe mapping.
#[must_use]
pub fn build_srt_settings(workdir: &Path) -> Option<serde_json::Value> {
    if !workdir.is_absolute() {
        return None;
    }
    let root = workdir.to_string_lossy().into_owned();
    Some(serde_json::json!({
        "network": {
            "allowedDomains": [],
            "deniedDomains": ["*"],
            "strictAllowlist": true,
        },
        "filesystem": {
            "denyRead": ["/"],
            "allowRead": [root],
            "allowWrite": [root],
            "denyWrite": ["/"],
        },
    }))
}
