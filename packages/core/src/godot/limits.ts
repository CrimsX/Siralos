/**
 * Immutable Godot milestone limits.
 *
 * Provider input cannot raise these limits and user configuration cannot
 * disable them. Truncation is always explicit; every bound is enforced by
 * the adapter during discovery, probing, and scanning.
 */
export const GODOT_LIMITS = {
  /** Maximum discovery candidates retained after validation. */
  maxCandidates: 16,
  /** Maximum accepted Godot executable size (512 MiB). */
  maxExecutableBytes: 512 * 1024 * 1024,
  /** Bounded `--version` output capture (64 KiB). */
  maxVersionOutputBytes: 64 * 1024,
  /** Bounded `--help` output capture (2 MiB). */
  maxHelpOutputBytes: 2 * 1024 * 1024,
  /** Bounded extension API dump file size (128 MiB). */
  maxApiDumpBytes: 128 * 1024 * 1024,
  /** Bounded project.godot size (4 MiB). */
  maxProjectFileBytes: 4 * 1024 * 1024,
  /** Bounded editor plugin descriptor size (256 KiB). */
  maxPluginDescriptorBytes: 256 * 1024,
  /** Bounded GDExtension descriptor size (1 MiB). */
  maxGDExtensionDescriptorBytes: 1024 * 1024,
  /** Maximum project files scanned by language and content inventory. */
  maxProjectFilesScanned: 50_000,
  /** Maximum project directories visited during static traversal (root counts). */
  maxProjectDirectoriesVisited: 10_000,
  /** Maximum project directory depth during static traversal (root counts). */
  maxProjectScanDepth: 64,
  /** Maximum readdir entries examined (excluded and non-regular entries count). */
  maxProjectEntriesExamined: 200_000,
  /** Maximum files surfaced in a static scan result. */
  maxProjectFilesSurfaced: 20_000,
  /** Maximum editor plugin directories enumerated in addons/. */
  maxProjectPluginDirectories: 256,
  /** Maximum plugin.cfg + .gdextension descriptors parsed per inspection. */
  maxProjectDescriptorsParsed: 512,
  /** Maximum executable-content inventory output items (scripts + plugins + descriptors + autoloads). */
  maxProjectInventoryItems: 4096,
  /** Maximum total raw file bytes read during content inventory (128 MiB). */
  maxProjectTotalReadBytes: 128 * 1024 * 1024,
  /** Maximum source bytes inspected by content inventory (64 MiB). */
  maxSourceBytesInspected: 64 * 1024 * 1024,
  /** Maximum tool-script head bytes scanned for `@tool` markers. */
  maxToolScriptHeadBytes: 32 * 1024,
  /** Maximum enabled editor plugin entries declared in project.godot. */
  maxProjectPlugins: 256,
  /** Maximum autoload declarations retained per project. */
  maxProjectAutoloads: 256,
  /** Maximum GDExtension library targets assessed per descriptor. */
  maxGDExtensionTargetsPerDescriptor: 512,
  /** Maximum UTF-8 byte length of a project-provided res:// path reference. */
  maxResReferencePathBytes: 1024,
  /** Maximum UTF-8 byte length of a descriptor field value. */
  maxProjectDescriptorValueBytes: 16 * 1024,
  /** Maximum configured installation entries. */
  maxConfiguredInstallations: 16,
  /** Maximum installation id length. */
  maxInstallationIdLength: 64,
  /** Version probe timeout. */
  versionProbeTimeoutMs: 10_000,
  /** Help probe timeout. */
  helpProbeTimeoutMs: 15_000,
  /** Extension API dump probe timeout. */
  apiDumpTimeoutMs: 120_000,
  /** Static project scan timeout (checked during traversal). */
  staticProjectScanTimeoutMs: 30_000,
} as const;
