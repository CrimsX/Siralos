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

/// One recognized `--help` option and the capability it advertises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GodotKnownOption {
    /// Complete option token as it appears in `--help` output.
    pub option: &'static str,
    /// The capability the option advertises.
    pub capability: GodotCapabilityKey,
}

/// Identifies one field of [`GodotCommandCapabilities`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotCapabilityKey {
    /// `--editor`.
    Editor,
    /// `--project-manager`.
    ProjectManager,
    /// `--recovery-mode`.
    RecoveryMode,
    /// `--headless`.
    Headless,
    /// `--path`.
    ProjectPath,
    /// `--scene`.
    Scene,
    /// `--script`.
    Script,
    /// `--check-only`.
    CheckOnly,
    /// `--import`.
    Import,
    /// `--quit`.
    Quit,
    /// `--quit-after`.
    QuitAfter,
    /// `--lsp-port`.
    Lsp,
    /// `--dap-port`.
    Dap,
    /// `--debug-server`.
    DebugServer,
    /// `--build-solutions`.
    BuildSolutions,
    /// `--dump-extension-api`.
    ExtensionApiDump,
    /// `--dump-extension-api-with-docs`.
    ExtensionApiWithDocsDump,
    /// `--validate-extension-api`.
    ExtensionApiValidation,
    /// `--doctool`.
    DocTool,
    /// `--write-movie`.
    MovieWriting,
}

impl GodotCapabilityKey {
    /// Set the matching capability flag to `value`.
    pub fn apply(
        self,
        capabilities: &mut GodotCommandCapabilities,
        value: bool,
    ) {
        match self {
            Self::Editor => capabilities.editor = value,
            Self::ProjectManager => capabilities.project_manager = value,
            Self::RecoveryMode => capabilities.recovery_mode = value,
            Self::Headless => capabilities.headless = value,
            Self::ProjectPath => capabilities.project_path = value,
            Self::Scene => capabilities.scene = value,
            Self::Script => capabilities.script = value,
            Self::CheckOnly => capabilities.check_only = value,
            Self::Import => capabilities.import = value,
            Self::Quit => capabilities.quit = value,
            Self::QuitAfter => capabilities.quit_after = value,
            Self::Lsp => capabilities.lsp = value,
            Self::Dap => capabilities.dap = value,
            Self::DebugServer => capabilities.debug_server = value,
            Self::BuildSolutions => capabilities.build_solutions = value,
            Self::ExtensionApiDump => capabilities.extension_api_dump = value,
            Self::ExtensionApiWithDocsDump => {
                capabilities.extension_api_with_docs_dump = value;
            }
            Self::ExtensionApiValidation => {
                capabilities.extension_api_validation = value
            }
            Self::DocTool => capabilities.doc_tool = value,
            Self::MovieWriting => capabilities.movie_writing = value,
        }
    }
}

/// Immutable, bounded option set recognized by the help capability parser.
pub const GODOT_KNOWN_OPTIONS: &[GodotKnownOption] = &[
    GodotKnownOption {
        option: "--editor",
        capability: GodotCapabilityKey::Editor,
    },
    GodotKnownOption {
        option: "--project-manager",
        capability: GodotCapabilityKey::ProjectManager,
    },
    GodotKnownOption {
        option: "--recovery-mode",
        capability: GodotCapabilityKey::RecoveryMode,
    },
    GodotKnownOption {
        option: "--headless",
        capability: GodotCapabilityKey::Headless,
    },
    GodotKnownOption {
        option: "--path",
        capability: GodotCapabilityKey::ProjectPath,
    },
    GodotKnownOption {
        option: "--scene",
        capability: GodotCapabilityKey::Scene,
    },
    GodotKnownOption {
        option: "--script",
        capability: GodotCapabilityKey::Script,
    },
    GodotKnownOption {
        option: "--check-only",
        capability: GodotCapabilityKey::CheckOnly,
    },
    GodotKnownOption {
        option: "--import",
        capability: GodotCapabilityKey::Import,
    },
    GodotKnownOption {
        option: "--quit",
        capability: GodotCapabilityKey::Quit,
    },
    GodotKnownOption {
        option: "--quit-after",
        capability: GodotCapabilityKey::QuitAfter,
    },
    GodotKnownOption {
        option: "--lsp-port",
        capability: GodotCapabilityKey::Lsp,
    },
    GodotKnownOption {
        option: "--dap-port",
        capability: GodotCapabilityKey::Dap,
    },
    GodotKnownOption {
        option: "--debug-server",
        capability: GodotCapabilityKey::DebugServer,
    },
    GodotKnownOption {
        option: "--build-solutions",
        capability: GodotCapabilityKey::BuildSolutions,
    },
    GodotKnownOption {
        option: "--dump-extension-api",
        capability: GodotCapabilityKey::ExtensionApiDump,
    },
    GodotKnownOption {
        option: "--dump-extension-api-with-docs",
        capability: GodotCapabilityKey::ExtensionApiWithDocsDump,
    },
    GodotKnownOption {
        option: "--validate-extension-api",
        capability: GodotCapabilityKey::ExtensionApiValidation,
    },
    GodotKnownOption {
        option: "--doctool",
        capability: GodotCapabilityKey::DocTool,
    },
    GodotKnownOption {
        option: "--write-movie",
        capability: GodotCapabilityKey::MovieWriting,
    },
];

/// Option tokens that must never be passed to a Godot probe executable.
///
/// Fixed Siralos probes pass only `--version`, `--help`, or
/// `--dump-extension-api`; these project-affecting tokens are prohibited in
/// probe invocation code and used by the architecture guardrail.
pub const FORBIDDEN_GODOT_PROJECT_ARGUMENTS: &[&str] =
    &["--path", "--upwards", "--import", "--scene", "--script"];

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
