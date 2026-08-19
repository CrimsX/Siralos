//! Command capabilities advertised by a Godot executable's `--help` output.
//!
//! Presence means advertised support, not operationally verified support.
//!
//! The two states stay distinct (see `verified_capabilities` on the engine
//! profile).

/// Capabilities advertised via `--help`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCommandCapabilities {
    /// `--editor`.
    pub editor: bool,
    /// `--project-manager`.
    pub project_manager: bool,
    /// `--recovery-mode`.
    pub recovery_mode: bool,
    /// `--headless`.
    pub headless: bool,
    /// `--path`.
    pub project_path: bool,
    /// `--scene`.
    pub scene: bool,
    /// `--script`.
    pub script: bool,
    /// `--check-only`.
    pub check_only: bool,
    /// `--import`.
    pub import: bool,
    /// `--quit`.
    pub quit: bool,
    /// `--quit-after`.
    pub quit_after: bool,
    /// `--lsp-port`.
    pub lsp: bool,
    /// `--dap-port`.
    pub dap: bool,
    /// `--debug-server`.
    pub debug_server: bool,
    /// `--build-solutions`.
    pub build_solutions: bool,
    /// `--dump-extension-api`.
    pub extension_api_dump: bool,
    /// `--dump-extension-api-with-docs`.
    pub extension_api_with_docs_dump: bool,
    /// `--validate-extension-api`.
    pub extension_api_validation: bool,
    /// `--doctool`.
    pub doc_tool: bool,
    /// `--write-movie`.
    pub movie_writing: bool,
}

/// Create an empty capability set with all flags cleared.
pub fn empty_godot_command_capabilities() -> GodotCommandCapabilities {
    GodotCommandCapabilities {
        editor: false,
        project_manager: false,
        recovery_mode: false,
        headless: false,
        project_path: false,
        scene: false,
        script: false,
        check_only: false,
        import: false,
        quit: false,
        quit_after: false,
        lsp: false,
        dap: false,
        debug_server: false,
        build_solutions: false,
        extension_api_dump: false,
        extension_api_with_docs_dump: false,
        extension_api_validation: false,
        doc_tool: false,
        movie_writing: false,
    }
}

#[cfg(test)]
mod tests {
    use super::empty_godot_command_capabilities;

    #[test]
    fn empty_capabilities_are_all_false() {
        let caps = empty_godot_command_capabilities();
        assert!(!caps.editor);
        assert!(!caps.extension_api_dump);
        assert!(!caps.movie_writing);
    }
}
