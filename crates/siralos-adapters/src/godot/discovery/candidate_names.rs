//! Fixed Godot executable names searched on PATH (R8-6b, TS oracle
//! `packages/adapters/src/godot/discovery/candidate-names.ts`).

/// Fixed Godot executable names for the given platform.
///
/// `platform` is the Node-style platform string: `"win32"` selects
/// the Windows spelling with `.exe`; every other value uses the
/// POSIX spelling.
#[must_use]
pub fn godot_candidate_names(platform: &str) -> Vec<String> {
    if platform == "win32" {
        vec![
            "godot.exe".to_owned(),
            "godot4.exe".to_owned(),
            "godot-mono.exe".to_owned(),
            "godot4-mono.exe".to_owned(),
        ]
    } else {
        vec![
            "godot".to_owned(),
            "godot4".to_owned(),
            "godot-mono".to_owned(),
            "godot4-mono".to_owned(),
        ]
    }
}

/// PATH entry separator for the platform.
#[must_use]
pub fn path_list_separator(platform: &str) -> char {
    if platform == "win32" { ';' } else { ':' }
}

#[cfg(test)]
mod tests {
    use super::{godot_candidate_names, path_list_separator};

    #[test]
    fn candidate_names_win32_end_with_exe() {
        let names = godot_candidate_names("win32");
        assert_eq!(names.len(), 4);
        assert!(names.iter().all(|name| name.ends_with(".exe")));
    }

    #[test]
    fn candidate_names_posix_have_no_extension() {
        let names = godot_candidate_names("linux");
        assert_eq!(names.len(), 4);
        assert!(names.iter().all(|name| !name.ends_with(".exe")));
    }

    #[test]
    fn separator_matches_platform() {
        assert_eq!(path_list_separator("win32"), ';');
        assert_eq!(path_list_separator("linux"), ':');
        assert_eq!(path_list_separator("darwin"), ':');
    }
}
