//! Conservative `res://` path resolution (R8).
//!
//! Mirrors `packages/core/src/godot/scene/resolution.ts`.

use super::limits::GODOT_SCENE_LIMITS;

/// Result of `res://` resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResPathResolution {
    /// Success with workspace-relative path.
    Ok(String),
    /// Failure with reason.
    Err(String),
}

impl ResPathResolution {
    /// True when Ok.
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Unwrap or panic.
    #[must_use]
    pub fn unwrap_path(self) -> String {
        match self {
            Self::Ok(p) => p,
            Self::Err(e) => panic!("{e}"),
        }
    }
}

/// Resolve `res://...` to a workspace-relative path, or report why not.
#[must_use]
pub fn resolve_res_path(reference: &str) -> ResPathResolution {
    if !reference.starts_with("res://") {
        return ResPathResolution::Err("Not a res:// reference.".to_owned());
    }
    let relative = &reference["res://".len()..];
    if relative.is_empty() {
        return ResPathResolution::Err("Empty res:// reference.".to_owned());
    }
    if relative.len() > GODOT_SCENE_LIMITS.max_document_bytes {
        return ResPathResolution::Err(
            "Reference exceeds the path length bound.".to_owned(),
        );
    }
    if relative.contains('\0')
        || relative.contains('\\')
        || relative.contains(':')
    {
        return ResPathResolution::Err(
            "Reference contains an unsupported path form.".to_owned(),
        );
    }
    let segments: Vec<&str> = relative.split('/').collect();
    if segments.iter().any(|s| *s == ".." || *s == "." || s.is_empty()) {
        return ResPathResolution::Err(
            "Reference is not a contained relative path.".to_owned(),
        );
    }
    ResPathResolution::Ok(relative.to_owned())
}

/// True when the string is a well-formed `uid://...` identity.
#[must_use]
pub fn is_godot_uid(value: &str) -> bool {
    if !value.to_ascii_lowercase().starts_with("uid://") {
        return false;
    }
    let suffix = &value[6..];
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{ResPathResolution, is_godot_uid, resolve_res_path};

    #[test]
    fn res_path_ok() {
        assert_eq!(
            resolve_res_path("res://scenes/player.tscn"),
            ResPathResolution::Ok("scenes/player.tscn".to_owned())
        );
    }

    #[test]
    fn res_path_rejects_traversal() {
        assert!(matches!(
            resolve_res_path("res://../escape.tscn"),
            ResPathResolution::Err(_)
        ));
    }

    #[test]
    fn uid_detection() {
        assert!(is_godot_uid("uid://abc123"));
        assert!(!is_godot_uid("res://foo"));
    }
}
