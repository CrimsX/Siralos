//! Static project profile and inventories (R8 Godot Stage-2 parity).
//!
//! Every value is derived from untrusted project files without executing
//! anything; results are non-authoritative.

/// Autoload entry extracted from `project.godot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotAutoloadSummary {
    /// Autoload name.
    pub name: String,
    /// Raw resource target (`res://...` or `*res://...` for singletons).
    pub target: String,
    /// Whether this autoload is a singleton.
    pub is_singleton: bool,
}

/// Editor plugin descriptor extracted from a bounded `plugin.cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotPluginSummary {
    /// Workspace-relative plugin directory, e.g. `addons/example`.
    pub path: String,
    /// Plugin name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Author.
    pub author: String,
    /// Version.
    pub version: String,
    /// Workspace-relative script path.
    pub script_path: String,
    /// Language of the plugin script.
    pub language: GodotPluginLanguage,
    /// Whether the project declares the plugin enabled.
    pub enabled: bool,
    /// Heuristic: likely an import plugin.
    pub import_plugin_heuristic: bool,
}

/// Language of a plugin script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotPluginLanguage {
    /// GDScript.
    Gdscript,
    /// C# / .NET.
    Dotnet,
    /// Unknown language.
    Unknown,
}

/// GDExtension descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotGDExtensionSummary {
    /// Workspace-relative descriptor path.
    pub path: String,
    /// Compatibility minimum, if any.
    pub compatibility_minimum: Option<String>,
    /// Library target paths.
    pub library_targets: Vec<String>,
    /// Whether any referenced library file exists.
    pub library_files_exist: bool,
    /// Whether any target path escapes through a symlink.
    pub escapes_through_symlinks: bool,
}

/// Why a bounded scan stopped early.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotScanTruncationReason {
    /// No truncation.
    None,
    /// File limit.
    FileLimit,
    /// Directory limit.
    DirectoryLimit,
    /// Entry limit.
    EntryLimit,
    /// Depth limit.
    DepthLimit,
    /// Surfaced limit.
    SurfacedLimit,
    /// Plugin limit.
    PluginLimit,
    /// Descriptor limit.
    DescriptorLimit,
    /// Inventory limit.
    InventoryLimit,
    /// Bytes limit.
    BytesLimit,
    /// Timeout.
    Timeout,
    /// Cancelled.
    Cancelled,
}

/// Statically identified project components that may execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotExecutableContentInventory {
    /// Workspace-relative paths of `.gd` files with `@tool`.
    pub tool_scripts: Vec<String>,
    /// Editor plugins.
    pub editor_plugins: Vec<GodotPluginSummary>,
    /// Heuristic import plugins.
    pub import_plugins: Vec<String>,
    /// GDExtension descriptors.
    pub gdextension_descriptors: Vec<GodotGDExtensionSummary>,
    /// Count of autoloads.
    pub autoload_count: u32,
    /// DotNet project files.
    pub dotnet_project_files: Vec<String>,
    /// True when a bound was hit.
    pub scan_truncated: bool,
    /// Exact truncation reason.
    pub scan_truncation_reason: GodotScanTruncationReason,
}

/// Create an empty inventory with no truncation.
pub fn create_empty_godot_executable_content_inventory()
-> GodotExecutableContentInventory {
    GodotExecutableContentInventory {
        tool_scripts: Vec::new(),
        editor_plugins: Vec::new(),
        import_plugins: Vec::new(),
        gdextension_descriptors: Vec::new(),
        autoload_count: 0,
        dotnet_project_files: Vec::new(),
        scan_truncated: false,
        scan_truncation_reason: GodotScanTruncationReason::None,
    }
}

/// Language profile of a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotLanguageProfile {
    /// GDScript only.
    Gdscript,
    /// C# / .NET only.
    Dotnet,
    /// Mixed.
    Mixed,
    /// Unknown.
    Unknown,
}

/// Static project profile (non-authoritative).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotProjectProfile {
    /// Whether `project.godot` was detected.
    pub detected: bool,
    /// SHA-256 of `project.godot`, if present.
    pub project_file_sha256: Option<String>,
    /// `config_version` value, if parsed.
    pub config_version: Option<u32>,
    /// Project `name` from `project.godot`.
    pub name: Option<String>,
    /// `application/config/version` value.
    pub application_version: Option<String>,
    /// All `config/features` feature tokens.
    pub declared_features: Vec<String>,
    /// Parsed `config/features` engine version, if any.
    pub declared_engine_version: Option<GodotDeclaredVersion>,
    /// Main scene res path.
    pub main_scene: Option<String>,
    /// Whether the main scene file exists.
    pub main_scene_exists: Option<bool>,
    /// Whether the main scene is a symlink.
    pub main_scene_is_symlink: bool,
    /// Rendering methods.
    pub rendering_methods: Vec<String>,
    /// Language profile.
    pub language_profile: GodotLanguageProfile,
    /// Autoloads.
    pub autoloads: Vec<GodotAutoloadSummary>,
    /// Enabled editor plugin paths.
    pub enabled_editor_plugins: Vec<String>,
    /// Executable-content inventory.
    pub executable_content: GodotExecutableContentInventory,
    /// Warnings.
    pub warnings: Vec<String>,
}

/// Parsed `config/features` engine version token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotDeclaredVersion {
    /// Major.
    pub major: u64,
    /// Minor.
    pub minor: u64,
    /// Patch, if present.
    pub patch: Option<u64>,
    /// Raw token.
    pub raw: String,
}

/// Create an empty project profile (no `project.godot`).
pub fn create_empty_godot_project_profile() -> GodotProjectProfile {
    GodotProjectProfile {
        detected: false,
        project_file_sha256: None,
        config_version: None,
        name: None,
        application_version: None,
        declared_features: Vec::new(),
        declared_engine_version: None,
        main_scene: None,
        main_scene_exists: None,
        main_scene_is_symlink: false,
        rendering_methods: Vec::new(),
        language_profile: GodotLanguageProfile::Unknown,
        autoloads: Vec::new(),
        enabled_editor_plugins: Vec::new(),
        executable_content: create_empty_godot_executable_content_inventory(),
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GodotLanguageProfile, create_empty_godot_project_profile};

    #[test]
    fn empty_profile_has_no_project() {
        let p = create_empty_godot_project_profile();
        assert!(!p.detected);
        assert_eq!(p.language_profile, GodotLanguageProfile::Unknown);
        assert!(p.declared_features.is_empty());
    }
}
