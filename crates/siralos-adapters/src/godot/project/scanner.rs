//! Bounded static scanner for untrusted project.godot content (R8).
//!
//! Mirrors packages/adapters/src/godot/project/project-scanner.ts.
//! Only supported value forms are interpreted; unsupported forms are
//! preserved as raw and reported. No evaluation, execution, or resolution.

use siralos_godot::godot::diagnostics::{DiagnosticSeverity, SafeDiagnostic};
use siralos_godot::godot::scene::{
    GodotVariantValue, is_balanced_text, parse_godot_variant,
};
use siralos_godot::godot::{
    GODOT_LIMITS, GodotAutoloadSummary, GodotInputAction,
};

const MAX_SECTIONS: usize = 128;
const MAX_PROPERTIES_PER_SECTION: usize = 4096;
const MAX_LINE_LENGTH: usize = 64 * 1024;
const MAX_WARNINGS: usize = 50;
const MAX_VALUE_CONTINUATION_LINES: usize = 64;

/// Bounded structural record of one project.godot property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedProjectProperty {
    /// Section name, empty for global.
    pub section: String,
    /// Property key.
    pub key: String,
    /// Raw bounded value text exactly as scanned.
    pub raw_value: String,
    /// One-based line number of the property.
    pub line_number: usize,
}

/// Kind of a scanned value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannedValueKind {
    /// Quoted string.
    String,
    /// Integer literal.
    Integer,
    /// Boolean literal.
    Boolean,
    /// PackedStringArray literal.
    PackedStringArray,
    /// uid:// literal.
    Uid,
    /// Unsupported raw form.
    Raw,
}

/// Interpreted scanned value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedValue {
    /// Value kind.
    pub kind: ScannedValueKind,
    /// Interpreted data when kind is known.
    pub value: Option<ScannedValueData>,
    /// Exact raw text.
    pub raw: String,
}

/// Interpreted data for a scanned value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedValueData {
    /// String value.
    String(String),
    /// Integer value.
    Integer(i64),
    /// Boolean value.
    Boolean(bool),
    /// Packed string array value.
    PackedStringArray(Vec<String>),
    /// Uid value.
    Uid(String),
}

/// Result of scanning one project.godot file.
#[derive(Debug, Clone)]
pub struct GodotProjectScanResult {
    /// All scanned properties in file order.
    pub properties: Vec<ScannedProjectProperty>,
    /// config_version when parsed as integer.
    pub config_version: Option<i64>,
    /// Project name from application/config/name.
    pub name: Option<String>,
    /// Application version from application/config/version.
    pub application_version: Option<String>,
    /// Declared feature tokens from application/config/features.
    pub declared_features: Vec<String>,
    /// Main scene res:// path.
    pub main_scene: Option<String>,
    /// Rendering methods.
    pub rendering_methods: Vec<String>,
    /// Dotnet assembly name.
    pub dotnet_assembly_name: Option<String>,
    /// Autoloads declared under [autoload].
    pub autoloads: Vec<GodotAutoloadSummary>,
    /// Enabled editor plugins from editor_plugins/enabled.
    pub enabled_plugins: Vec<String>,
    /// Input actions declared under [input].
    pub input_actions: Vec<GodotInputAction>,
    /// Bounded warnings.
    pub warnings: Vec<SafeDiagnostic>,
    /// True when a bounded scan limit was reached.
    pub truncated: bool,
}

/// Conservative, bounded scanner for untrusted project.godot content.
#[must_use]
pub fn scan_project_file(content: &str) -> GodotProjectScanResult {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut properties: Vec<ScannedProjectProperty> = Vec::new();
    let mut warnings: Vec<SafeDiagnostic> = Vec::new();
    let mut section = String::new();
    let mut section_count: usize = 0;
    let mut property_count: usize = 0;
    let mut truncated = false;
    let mut index: usize = 0;
    while index < lines.len() {
        let line_raw = lines[index];
        let line = line_raw.strip_suffix('\r').unwrap_or(line_raw);
        if line.len() > MAX_LINE_LENGTH {
            truncated = true;
            index += 1;
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || is_comment_line(trimmed) {
            index += 1;
            continue;
        }
        if trimmed.starts_with('[') && trimmed.contains(']') {
            if section_count >= MAX_SECTIONS {
                truncated = true;
                index += 1;
                continue;
            }
            section_count += 1;
            property_count = 0;
            let closing = trimmed.find(']').unwrap_or(trimmed.len() - 1);
            section = trimmed[1..closing].trim().to_owned();
            index += 1;
            continue;
        }
        let Some(equals_index) = trimmed.find('=') else {
            add_scan_warning(
                &mut warnings,
                format!(
                    "Unrecognized project setting without a value at line {}.",
                    index + 1
                ),
            );
            index += 1;
            continue;
        };
        if property_count >= MAX_PROPERTIES_PER_SECTION {
            truncated = true;
            index += 1;
            continue;
        }
        property_count += 1;
        let key_raw = trimmed[..equals_index].trim();
        let key = unquote_key(key_raw);
        let section_snapshot = section.clone();
        let key_snapshot = key.clone();
        let mut raw_value = trimmed[equals_index + 1..].trim().to_owned();
        let mut continuation: usize = 0;
        while !is_balanced_text(&raw_value)
            && continuation < MAX_VALUE_CONTINUATION_LINES
        {
            index += 1;
            continuation += 1;
            if index >= lines.len() {
                break;
            }
            let next_raw = lines[index];
            let next_line = next_raw.strip_suffix('\r').unwrap_or(next_raw);
            let next_trimmed = next_line.trim();
            if next_trimmed.is_empty() || is_comment_line(next_trimmed) {
                continue;
            }
            raw_value = format!("{raw_value}\n{next_trimmed}");
        }
        if !is_balanced_text(&raw_value) {
            add_scan_warning(
                &mut warnings,
                format!(
                    "The value of {section_snapshot}.{key_snapshot} at line {} is unbalanced and was truncated at the continuation bound.",
                    index + 1
                ),
            );
        }
        let line_number = index + 1;
        properties.push(ScannedProjectProperty {
            section: section_snapshot,
            key: key_snapshot,
            raw_value,
            line_number,
        });
        index += 1;
    }

    let config_version =
        read_integer(&properties, "", "config_version", &mut warnings);
    let name =
        read_string(&properties, "application", "config/name", &mut warnings);
    let application_version = read_string(
        &properties,
        "application",
        "config/version",
        &mut warnings,
    );
    let declared_features =
        read_string_array(&properties, "application", "config/features");
    let main_scene = read_string(
        &properties,
        "application",
        "run/main_scene",
        &mut warnings,
    );
    let rendering_methods = {
        let a = read_string(
            &properties,
            "rendering",
            "renderer/rendering_method",
            &mut warnings,
        );
        let b = read_string(
            &properties,
            "rendering",
            "renderer/rendering_method.mobile",
            &mut warnings,
        );
        let mut out = Vec::new();
        if let Some(v) = a {
            out.push(v);
        }
        if let Some(v) = b {
            if !out.contains(&v) {
                out.push(v);
            }
        }
        out
    };
    let dotnet_assembly_name = read_string(
        &properties,
        "dotnet",
        "project/assembly_name",
        &mut warnings,
    );
    let autoloads = read_autoloads(&properties, &mut warnings);
    let enabled_plugins = read_enabled_plugins(&properties, &mut warnings);
    let input_actions = read_input_actions(&properties, &mut warnings);

    GodotProjectScanResult {
        properties,
        config_version,
        name,
        application_version,
        declared_features,
        main_scene,
        rendering_methods,
        dotnet_assembly_name,
        autoloads,
        enabled_plugins,
        input_actions,
        warnings,
        truncated,
    }
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with(';') || trimmed.starts_with('#')
}

fn unquote_key(key: &str) -> String {
    if key.len() >= 2 && key.starts_with('\"') && key.ends_with('\"') {
        key[1..key.len() - 1].to_owned()
    } else {
        key.to_owned()
    }
}

fn find_property<'a>(
    properties: &'a [ScannedProjectProperty],
    section: &str,
    key: &str,
) -> Option<&'a ScannedProjectProperty> {
    properties.iter().find(|p| p.section == section && p.key == key)
}

fn interpret_value(raw: &str) -> ScannedValue {
    if raw.starts_with('\"') {
        if let Some(parsed) = parse_quoted_string(raw, false) {
            return ScannedValue {
                kind: ScannedValueKind::String,
                value: Some(ScannedValueData::String(parsed)),
                raw: raw.to_owned(),
            };
        }
        return ScannedValue {
            kind: ScannedValueKind::Raw,
            value: None,
            raw: raw.to_owned(),
        };
    }
    if raw.starts_with("PackedStringArray(") && raw.ends_with(')') {
        let inner = raw["PackedStringArray(".len()..raw.len() - 1].trim();
        if let Some(items) = parse_comma_separated_strings(inner) {
            return ScannedValue {
                kind: ScannedValueKind::PackedStringArray,
                value: Some(ScannedValueData::PackedStringArray(items)),
                raw: raw.to_owned(),
            };
        }
        return ScannedValue {
            kind: ScannedValueKind::Raw,
            value: None,
            raw: raw.to_owned(),
        };
    }
    if is_integer_text(raw) {
        if let Ok(n) = raw.parse::<i64>() {
            return ScannedValue {
                kind: ScannedValueKind::Integer,
                value: Some(ScannedValueData::Integer(n)),
                raw: raw.to_owned(),
            };
        }
    }
    if raw == "true" {
        return ScannedValue {
            kind: ScannedValueKind::Boolean,
            value: Some(ScannedValueData::Boolean(true)),
            raw: raw.to_owned(),
        };
    }
    if raw == "false" {
        return ScannedValue {
            kind: ScannedValueKind::Boolean,
            value: Some(ScannedValueData::Boolean(false)),
            raw: raw.to_owned(),
        };
    }
    if is_uid_text(raw) {
        return ScannedValue {
            kind: ScannedValueKind::Uid,
            value: Some(ScannedValueData::Uid(raw.to_owned())),
            raw: raw.to_owned(),
        };
    }
    ScannedValue {
        kind: ScannedValueKind::Raw,
        value: None,
        raw: raw.to_owned(),
    }
}

fn is_integer_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes[0] == b'+' || bytes[0] == b'-' {
        if bytes.len() == 1 {
            return false;
        }
        i = 1;
    }
    bytes[i..].iter().all(|b| b.is_ascii_digit())
}

fn is_uid_text(text: &str) -> bool {
    if !text.starts_with("uid://") {
        return false;
    }
    let rest = &text["uid://".len()..];
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

fn parse_quoted_string(raw: &str, allow_trailing: bool) -> Option<String> {
    if !raw.starts_with('\"') {
        return None;
    }
    let mut value = String::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut index: usize = 1;
    let mut closed = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\"' {
            closed = true;
            index += 1;
            break;
        }
        if ch == '\\' {
            let next = chars.get(index + 1).copied();
            match next {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('\"') | Some('\\') | Some('\'') => {
                    value.push(next.unwrap())
                }
                None => break,
                Some(other) => {
                    value.push('\\');
                    value.push(other);
                }
            }
            index += 2;
            continue;
        }
        value.push(ch);
        index += 1;
    }
    if !closed {
        return None;
    }
    if !allow_trailing {
        let remainder: String = chars[index..].iter().collect();
        if !remainder.trim().is_empty() {
            return None;
        }
    }
    Some(value)
}

fn parse_comma_separated_strings(text: &str) -> Option<Vec<String>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    let mut items: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index: usize = 0;
    while index < chars.len() {
        while index < chars.len()
            && (chars[index] == ',' || chars[index].is_whitespace())
        {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let byte_index: usize =
            chars[..index].iter().map(|c| c.len_utf8()).sum();
        let segment = &text[byte_index..];
        let parsed = parse_quoted_string_allow_trailing(segment)?;
        items.push(parsed.value);
        let consumed_chars = parsed.consumed;
        let consumed_bytes: usize =
            segment.chars().take(consumed_chars).map(|c| c.len_utf8()).sum();
        let char_consumed_bytes = consumed_bytes;
        let raw_consumed_chars =
            segment[..char_consumed_bytes].chars().count();
        index += raw_consumed_chars;
    }
    Some(items)
}

struct QuotedParse {
    value: String,
    consumed: usize,
}

fn parse_quoted_string_allow_trailing(raw: &str) -> Option<QuotedParse> {
    if !raw.starts_with('\"') {
        return None;
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut value = String::new();
    let mut index: usize = 1;
    let mut closed = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\"' {
            closed = true;
            index += 1;
            break;
        }
        if ch == '\\' {
            let next = chars.get(index + 1).copied();
            match next {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('\"') | Some('\\') | Some('\'') => {
                    value.push(next.unwrap())
                }
                None => break,
                Some(other) => {
                    value.push('\\');
                    value.push(other);
                }
            }
            index += 2;
            continue;
        }
        value.push(ch);
        index += 1;
    }
    if !closed {
        return None;
    }
    Some(QuotedParse { value, consumed: index })
}

fn read_integer(
    properties: &[ScannedProjectProperty],
    section: &str,
    key: &str,
    warnings: &mut Vec<SafeDiagnostic>,
) -> Option<i64> {
    let prop = find_property(properties, section, key)?;
    let v = interpret_value(&prop.raw_value);
    if v.kind != ScannedValueKind::Integer {
        warn_unknown(warnings, prop);
        return None;
    }
    if let Some(ScannedValueData::Integer(n)) = v.value {
        Some(n)
    } else {
        None
    }
}

fn read_string(
    properties: &[ScannedProjectProperty],
    section: &str,
    key: &str,
    warnings: &mut Vec<SafeDiagnostic>,
) -> Option<String> {
    let prop = find_property(properties, section, key)?;
    let v = interpret_value(&prop.raw_value);
    if v.kind != ScannedValueKind::String {
        warn_unknown(warnings, prop);
        return None;
    }
    if let Some(ScannedValueData::String(s)) = v.value {
        Some(s)
    } else {
        None
    }
}

fn read_string_array(
    properties: &[ScannedProjectProperty],
    section: &str,
    key: &str,
) -> Vec<String> {
    let Some(prop) = find_property(properties, section, key) else {
        return Vec::new();
    };
    let v = interpret_value(&prop.raw_value);
    if v.kind != ScannedValueKind::PackedStringArray {
        return Vec::new();
    }
    if let Some(ScannedValueData::PackedStringArray(items)) = v.value {
        items
    } else {
        Vec::new()
    }
}

fn validate_project_relative_path(raw: &str, max_bytes: usize) -> bool {
    if raw.contains('\0') {
        return false;
    }
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return false;
    }
    if normalized.len() >= 2 {
        let bytes = normalized.as_bytes();
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes.get(2) == Some(&b'/')
        {
            return false;
        }
    }
    for seg in normalized.split('/') {
        if seg == ".." {
            return false;
        }
    }
    if raw.len() > max_bytes {
        return false;
    }
    true
}

fn add_scan_warning(warnings: &mut Vec<SafeDiagnostic>, message: String) {
    if warnings.len() < MAX_WARNINGS {
        warnings.push(SafeDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message,
        });
    }
}

fn warn_unknown(
    warnings: &mut Vec<SafeDiagnostic>,
    prop: &ScannedProjectProperty,
) {
    if warnings.len() >= MAX_WARNINGS {
        return;
    }
    warnings.push(SafeDiagnostic {
        severity: DiagnosticSeverity::Warning,
        message: format!(
            "The value of {}.{} (line {}) uses an unsupported form and was preserved as unknown.",
            prop.section, prop.key, prop.line_number
        ),
    });
}

fn read_autoloads(
    properties: &[ScannedProjectProperty],
    warnings: &mut Vec<SafeDiagnostic>,
) -> Vec<GodotAutoloadSummary> {
    let mut out = Vec::new();
    for prop in properties {
        if prop.section != "autoload" {
            continue;
        }
        if out.len() >= GODOT_LIMITS.max_project_autoloads {
            add_scan_warning(
                warnings,
                "The number of autoload declarations exceeded the bound (maxProjectAutoloads); the autoload list is partial.".to_owned(),
            );
            break;
        }
        let v = interpret_value(&prop.raw_value);
        if v.kind != ScannedValueKind::String {
            continue;
        }
        let target = if let Some(ScannedValueData::String(s)) = v.value {
            s
        } else {
            continue;
        };
        let reference = target.strip_prefix('*').unwrap_or(&target);
        if let Some(rel) = reference.strip_prefix("res://") {
            if !validate_project_relative_path(
                rel,
                GODOT_LIMITS.max_res_reference_path_bytes,
            ) {
                add_scan_warning(
                    warnings,
                    format!(
                        "The autoload {} declares a target ({}) that is not a contained project path.",
                        prop.key, target
                    ),
                );
            }
        } else if !reference.is_empty() {
            add_scan_warning(
                warnings,
                format!(
                    "The autoload {} declares a non-res:// target ({}) that could not be validated as a project path.",
                    prop.key, target
                ),
            );
        }
        out.push(GodotAutoloadSummary {
            name: prop.key.clone(),
            target: target.clone(),
            is_singleton: target.starts_with('*'),
        });
    }
    out
}

fn read_enabled_plugins(
    properties: &[ScannedProjectProperty],
    warnings: &mut Vec<SafeDiagnostic>,
) -> Vec<String> {
    let raw = read_string_array(properties, "editor_plugins", "enabled");
    let mut enabled = Vec::new();
    for plugin in raw {
        if enabled.len() >= GODOT_LIMITS.max_project_plugins {
            add_scan_warning(
                warnings,
                "The number of enabled editor plugins exceeded the bound (maxProjectPlugins); the enabled-plugin list is partial.".to_owned(),
            );
            break;
        }
        let reference = if let Some(s) = plugin.strip_prefix("res://") {
            s
        } else {
            plugin.as_str()
        };
        if !validate_project_relative_path(
            reference,
            GODOT_LIMITS.max_res_reference_path_bytes,
        ) {
            add_scan_warning(
                warnings,
                format!(
                    "The enabled plugin entry ({}) is not a contained project path and cannot be matched to an addon.",
                    plugin
                ),
            );
            continue;
        }
        enabled.push(plugin);
    }
    enabled
}

fn read_input_actions(
    properties: &[ScannedProjectProperty],
    warnings: &mut Vec<SafeDiagnostic>,
) -> Vec<GodotInputAction> {
    let mut actions = Vec::new();
    for prop in properties {
        if prop.section != "input" {
            continue;
        }
        if actions.len() >= GODOT_LIMITS.max_project_input_actions {
            add_scan_warning(
                warnings,
                "The number of input actions exceeded the bound (maxProjectInputActions); the input-action list is partial.".to_owned(),
            );
            break;
        }
        actions.push(interpret_input_action(&prop.key, &prop.raw_value));
    }
    actions
}

fn interpret_input_action(name: &str, raw: &str) -> GodotInputAction {
    let parsed = parse_godot_variant(raw);
    if let GodotVariantValue::Dictionary(entries) = parsed.value {
        let mut deadzone: Option<f64> = None;
        let mut event_count: usize = 0;
        let mut event_types: Vec<String> = Vec::new();
        for entry in &entries {
            if let GodotVariantValue::String(k) = entry.key.as_ref() {
                if k == "deadzone" {
                    if let GodotVariantValue::Float(f) = entry.value.as_ref() {
                        deadzone = Some(*f);
                    }
                } else if k == "events" {
                    if let GodotVariantValue::Array(items) =
                        entry.value.as_ref()
                    {
                        for ev in items {
                            if event_count
                                >= GODOT_LIMITS.max_input_action_event_types
                            {
                                break;
                            }
                            if let GodotVariantValue::Opaque {
                                type_name,
                                ..
                            } = ev
                            {
                                if type_name.starts_with("InputEvent") {
                                    event_count += 1;
                                    if event_types.len()
                                        < GODOT_LIMITS
                                            .max_input_action_event_types
                                    {
                                        event_types.push(type_name.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return GodotInputAction {
            name: name.to_owned(),
            deadzone,
            event_count,
            event_types,
        };
    }
    let mut event_types: Vec<String> = Vec::new();
    let mut search_from: usize = 0;
    while let Some(pos) = raw[search_from..].find("Object(") {
        let abs = search_from + pos;
        let after = &raw[abs + "Object(".len()..];
        let trimmed = after.trim_start();
        let mut end = 0usize;
        for (i, c) in trimmed.chars().enumerate() {
            if c.is_ascii_alphanumeric() || c == '_' {
                end = i + 1;
            } else {
                break;
            }
        }
        if end > 0 {
            let type_name = &trimmed[..end];
            if type_name.starts_with("InputEvent")
                && !event_types.contains(&type_name.to_owned())
                && event_types.len()
                    < GODOT_LIMITS.max_input_action_event_types
            {
                event_types.push(type_name.to_owned());
            }
        }
        search_from = abs + "Object(".len();
        if event_types.len() >= GODOT_LIMITS.max_input_action_event_types {
            break;
        }
    }
    let count = event_types.len();
    GodotInputAction {
        name: name.to_owned(),
        deadzone: None,
        event_count: count,
        event_types,
    }
}
