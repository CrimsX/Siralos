//! Conservative normalization of LSP payloads into the provider-neutral
//! models.
//!
//! Mirror URIs map to workspace-relative paths; out-of-mirror URIs are
//! rejected or represented conservatively; every field is bounded;
//! control characters are sanitized; markup is data (never executed or
//! rendered); and malformed items are skipped safely. LSP line/character
//! positions are 0-based and converted to the 1-based Siralos convention
//! explicitly at this boundary. The generic payload normalization lives in
//! the core language module (Stage 3R R5); this adapter supplies the
//! mirror URI mapping and the Godot source label.

use serde_json::Value;

use siralos_core::language::position::{RawPosition, RawRange};
use siralos_core::language::truncate_utf8_bytes;
use siralos_core::language::{
    LANGUAGE_LIMITS, LanguageLimits, NormalizedDiagnosticPayload,
    RawDefinitionEntry, RawDiagnostic, RawDiagnosticCode,
    normalize_definition_locations, normalize_diagnostic_payload,
    sanitize_control_characters, to_one_based_range,
};
use siralos_godot::godot::{
    GODOT_LIMITS, GdScriptCompletionItem, GdScriptCompletionResult,
    GdScriptDefinitionLocation, GdScriptHoverResult, GdScriptHoverSection,
    GdScriptSeverity, GdScriptSourceRange, GodotGdScriptDiagnostic,
};

use super::file_uri::mirror_uri_to_workspace_relative;

/// Context binding one normalization to a mirror root and fallback path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspNormalizationContext<'a> {
    /// Absolute mirror project root.
    pub mirror_root_path: &'a str,
    /// Workspace-relative path of the queried document.
    pub path: &'a str,
}

/// Normalized `textDocument/publishDiagnostics` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPublishDiagnostics {
    /// Workspace-relative document path.
    pub path: String,
    /// Bounded diagnostics in payload order.
    pub diagnostics: Vec<GodotGdScriptDiagnostic>,
    /// True when the per-document count bound was applied.
    pub truncated: bool,
}

/// Normalize `textDocument/publishDiagnostics`. Out-of-mirror URIs are
/// rejected (`None`); malformed payloads are rejected; entries with empty
/// messages are skipped conservatively.
pub fn normalize_publish_diagnostics(
    uri: &str,
    raw_diagnostics: &Value,
    context: LspNormalizationContext<'_>,
) -> Option<NormalizedPublishDiagnostics> {
    let path =
        mirror_uri_to_workspace_relative(uri, context.mirror_root_path)?;
    let limits = LanguageLimits {
        max_diagnostics_per_set: GODOT_LIMITS.lsp_max_diagnostics_per_document,
        ..LANGUAGE_LIMITS
    };
    let raws = parse_raw_diagnostics(raw_diagnostics);
    let normalized: NormalizedDiagnosticPayload = normalize_diagnostic_payload(
        &raws,
        "godot-lsp",
        &path,
        Some(context.mirror_root_path),
        &limits,
    );
    Some(NormalizedPublishDiagnostics {
        path: normalized.path,
        diagnostics: normalized
            .diagnostics
            .iter()
            .map(|diagnostic| GodotGdScriptDiagnostic {
                source: siralos_godot::godot::GdScriptDiagnosticSource::Lsp,
                severity: match diagnostic.severity {
                    siralos_core::language::DiagnosticSeverity::Error => {
                        GdScriptSeverity::Error
                    }
                    siralos_core::language::DiagnosticSeverity::Warning => {
                        GdScriptSeverity::Warning
                    }
                    siralos_core::language::DiagnosticSeverity::Info => {
                        GdScriptSeverity::Info
                    }
                    siralos_core::language::DiagnosticSeverity::Unknown => {
                        GdScriptSeverity::Unknown
                    }
                },
                path: diagnostic.path.clone(),
                line: diagnostic.line.map(|line| line as u32),
                column: diagnostic.column.map(|column| column as u32),
                code: diagnostic.code.clone(),
                message: diagnostic.message.clone(),
                raw_category: diagnostic.raw_category.clone(),
            })
            .collect(),
        truncated: normalized.truncated,
    })
}

fn parse_raw_diagnostics(raw: &Value) -> Vec<RawDiagnostic> {
    let Some(entries) = raw.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let record = entry.as_object()?;
            Some(RawDiagnostic {
                range: parse_raw_range(record.get("range")),
                severity: record.get("severity").and_then(Value::as_i64),
                code: record.get("code").and_then(|code| match code {
                    Value::String(text) => {
                        Some(RawDiagnosticCode::Text(text.clone()))
                    }
                    Value::Number(number) => {
                        number.as_i64().map(RawDiagnosticCode::Number)
                    }
                    _ => None,
                }),
                message: record
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source: record
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn parse_raw_range(value: Option<&Value>) -> Option<RawRange> {
    let record = value?.as_object()?;
    Some(RawRange {
        start: parse_raw_position(record.get("start"))?,
        end: parse_raw_position(record.get("end"))?,
    })
}

fn parse_raw_position(value: Option<&Value>) -> Option<RawPosition> {
    let record = value?.as_object()?;
    Some(RawPosition {
        line: record.get("line").and_then(Value::as_i64),
        // LSP calls the 0-based offset `character`.
        column: record.get("character").and_then(Value::as_i64),
    })
}

/// Normalize one hover response; markup is data, never executed or
/// rendered.
pub fn normalize_hover(
    uri: &str,
    hover: &Value,
    context: LspNormalizationContext<'_>,
) -> Option<GdScriptHoverResult> {
    let path =
        mirror_uri_to_workspace_relative(uri, context.mirror_root_path)?;
    if hover.is_null() {
        return None;
    }
    let contents = extract_hover_contents(hover, context);
    let range = hover
        .as_object()
        .and_then(|record| parse_raw_range(record.get("range")))
        .and_then(to_one_based_range)
        .map(map_language_range);
    Some(GdScriptHoverResult { path, range, contents })
}

fn extract_hover_contents(
    hover: &Value,
    context: LspNormalizationContext<'_>,
) -> Vec<GdScriptHoverSection> {
    if let Some(record) = hover.as_object() {
        return sections_from_contents(
            record.get("contents").unwrap_or(&Value::Null),
            context,
        );
    }
    sections_from_contents(hover, context)
}

fn sections_from_contents(
    value: &Value,
    context: LspNormalizationContext<'_>,
) -> Vec<GdScriptHoverSection> {
    let mut sections: Vec<GdScriptHoverSection> = Vec::new();
    let mut total_bytes: usize = 0;
    let max_bytes = GODOT_LIMITS.lsp_max_hover_bytes;
    fn push_section(
        sections: &mut Vec<GdScriptHoverSection>,
        total_bytes: &mut usize,
        max_bytes: usize,
        kind: &str,
        text: &str,
        mirror_root_path: &str,
    ) {
        let sanitized = sanitize_control_characters(text)
            .as_str()
            .split(mirror_root_path)
            .collect::<Vec<_>>()
            .join("<mirror>");
        let remaining = max_bytes.saturating_sub(*total_bytes);
        let bounded = truncate_utf8_bytes(&sanitized, remaining);
        *total_bytes += bounded.len();
        sections.push(GdScriptHoverSection {
            kind: kind.to_owned(),
            text: bounded,
        });
    }
    fn visit(
        entry: &Value,
        sections: &mut Vec<GdScriptHoverSection>,
        total_bytes: &mut usize,
        max_bytes: usize,
        mirror_root_path: &str,
    ) {
        if *total_bytes >= max_bytes {
            return;
        }
        if let Some(text) = entry.as_str() {
            push_section(
                sections,
                total_bytes,
                max_bytes,
                "plaintext",
                text,
                mirror_root_path,
            );
            return;
        }
        if let Some(items) = entry.as_array() {
            for item in items {
                visit(
                    item,
                    sections,
                    total_bytes,
                    max_bytes,
                    mirror_root_path,
                );
            }
            return;
        }
        if let Some(record) = entry.as_object() {
            let kind = record.get("kind").and_then(Value::as_str);
            let language = record.get("language").and_then(Value::as_str);
            let section_value = record.get("value").and_then(Value::as_str);
            if let (Some(kind), Some(section_value)) = (kind, section_value) {
                push_section(
                    sections,
                    total_bytes,
                    max_bytes,
                    if kind == "markdown" { "markdown" } else { "plaintext" },
                    section_value,
                    mirror_root_path,
                );
                return;
            }
            if let (Some(_language), Some(section_value)) =
                (language, section_value)
            {
                push_section(
                    sections,
                    total_bytes,
                    max_bytes,
                    "plaintext",
                    section_value,
                    mirror_root_path,
                );
            }
        }
    }
    visit(
        value,
        &mut sections,
        &mut total_bytes,
        max_bytes,
        context.mirror_root_path,
    );
    sections
}

/// Normalize a completion response; `additionalTextEdits` and `command`
/// attachments are deliberately dropped because completion never mutates
/// files or executes commands.
pub fn normalize_completion(
    uri: &str,
    completion: &Value,
    context: LspNormalizationContext<'_>,
) -> GdScriptCompletionResult {
    let path = mirror_uri_to_workspace_relative(uri, context.mirror_root_path)
        .unwrap_or_else(|| context.path.to_owned());
    let raw_items = match completion {
        Value::Array(items) => Some(items),
        Value::Object(record) => record.get("items").and_then(Value::as_array),
        _ => None,
    };
    let mut items: Vec<GdScriptCompletionItem> = Vec::new();
    let mut truncated = false;
    if let Some(raw_items) = raw_items {
        for entry in raw_items {
            if items.len() >= GODOT_LIMITS.lsp_max_completion_items {
                truncated = true;
                break;
            }
            if let Some(item) = normalize_completion_item(entry, context) {
                items.push(item);
            }
        }
    }
    GdScriptCompletionResult { path, items, truncated }
}

fn normalize_completion_item(
    entry: &Value,
    context: LspNormalizationContext<'_>,
) -> Option<GdScriptCompletionItem> {
    let record = entry.as_object()?;
    let label = record
        .get("label")
        .and_then(Value::as_str)
        .map(sanitize_control_characters)?;
    if label.is_empty() {
        return None;
    }
    let kind = record
        .get("kind")
        .and_then(Value::as_number)
        .map(|number| number.to_string());
    let detail = bounded_detail(record.get("detail"), context);
    let documentation =
        bounded_documentation(record.get("documentation"), context);
    let insert_text =
        record.get("insertText").and_then(Value::as_str).and_then(|text| {
            bounded_detail(Some(&Value::String(text.to_owned())), context)
        });
    Some(GdScriptCompletionItem {
        label: truncate_utf8_bytes(&label, GODOT_LIMITS.lsp_max_hover_bytes),
        kind,
        detail,
        documentation,
        insert_text,
    })
}

fn bounded_detail(
    value: Option<&Value>,
    context: LspNormalizationContext<'_>,
) -> Option<String> {
    let text = value?.as_str()?;
    Some(truncate_utf8_bytes(
        &mask_mirror(
            sanitize_control_characters(text).as_str(),
            context.mirror_root_path,
        ),
        GODOT_LIMITS.lsp_max_hover_bytes,
    ))
}

fn bounded_documentation(
    value: Option<&Value>,
    context: LspNormalizationContext<'_>,
) -> Option<String> {
    match value? {
        Value::String(text) => Some(truncate_utf8_bytes(
            &mask_mirror(
                sanitize_control_characters(text).as_str(),
                context.mirror_root_path,
            ),
            GODOT_LIMITS.lsp_max_hover_bytes,
        )),
        Value::Object(record) => {
            let inner = record.get("value")?.as_str()?;
            Some(truncate_utf8_bytes(
                &mask_mirror(
                    sanitize_control_characters(inner).as_str(),
                    context.mirror_root_path,
                ),
                GODOT_LIMITS.lsp_max_hover_bytes,
            ))
        }
        _ => None,
    }
}

fn mask_mirror(text: &str, mirror_root_path: &str) -> String {
    text.split(mirror_root_path).collect::<Vec<_>>().join("<mirror>")
}

/// Normalize a definition response; the generic location normalization
/// stays in the core language module and the mirror URI mapping stays at
/// this adapter boundary.
pub fn normalize_definition(
    uri: &str,
    locations: &Value,
    context: LspNormalizationContext<'_>,
) -> siralos_godot::godot::GdScriptDefinitionResult {
    let path = mirror_uri_to_workspace_relative(uri, context.mirror_root_path)
        .unwrap_or_else(|| context.path.to_owned());
    let entries: Vec<RawDefinitionEntry> = match locations {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let record = item.as_object();
                RawDefinitionEntry {
                    uri: record
                        .and_then(|record| record.get("uri"))
                        .or_else(|| {
                            record.and_then(|record| record.get("targetUri"))
                        })
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    range: record
                        .and_then(|record| record.get("range"))
                        .or_else(|| {
                            record.and_then(|record| {
                                record.get("targetSelectionRange")
                            })
                        })
                        .and_then(|value| parse_raw_range(Some(value))),
                }
            })
            .collect(),
        Value::Object(_) => vec![RawDefinitionEntry {
            uri: locations
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_owned),
            range: locations
                .get("range")
                .and_then(|value| parse_raw_range(Some(value))),
        }],
        _ => Vec::new(),
    };
    let normalized = normalize_definition_locations(
        &entries,
        &path,
        |target| {
            mirror_uri_to_workspace_relative(target, context.mirror_root_path)
        },
        siralos_core::language::DefinitionLimits {
            max_locations: GODOT_LIMITS.lsp_max_definition_locations,
        },
    );
    siralos_godot::godot::GdScriptDefinitionResult {
        path: normalized.path,
        locations: normalized
            .locations
            .iter()
            .map(|location| GdScriptDefinitionLocation {
                path: location.path.clone(),
                range: map_language_range(location.range),
                external: location.external,
            })
            .collect(),
        truncated: normalized.truncated,
    }
}

fn map_language_range(
    range: siralos_core::language::LanguageRange,
) -> GdScriptSourceRange {
    let to_position = |position: siralos_core::language::LanguagePosition| {
        siralos_godot::godot::GdScriptPosition {
            line: u32::try_from(position.line).unwrap_or(u32::MAX),
            column: u32::try_from(position.column).unwrap_or(u32::MAX),
        }
    };
    GdScriptSourceRange {
        start: to_position(range.start),
        end: to_position(range.end),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LspNormalizationContext, normalize_completion, normalize_definition,
        normalize_hover, normalize_publish_diagnostics,
    };
    use serde_json::json;
    use siralos_godot::godot::GdScriptSeverity;

    const MIRROR: &str = if cfg!(windows) { "C:\\mirror" } else { "/mirror" };

    fn context() -> LspNormalizationContext<'static> {
        LspNormalizationContext {
            mirror_root_path: MIRROR,
            path: "src/player.gd",
        }
    }

    #[test]
    fn publish_diagnostics_maps_severity_and_positions() {
        let payload = json!([
            {
                "range": {"start": {"line": 33, "character": 16}, "end": {"line": 33, "character": 25}},
                "severity": 1,
                "code": "identifier",
                "source": "gdscript",
                "message": "Identifier \"velocityy\" not declared."
            },
            {"severity": 2, "message": "A warning without a range."},
            "malformed entry skipped",
            {"severity": 9, "message": ""}
        ]);
        let result = normalize_publish_diagnostics(
            &super::super::path_to_file_uri(&format!(
                "{MIRROR}/src/player.gd"
            )),
            &payload,
            context(),
        )
        .expect("in-mirror uri");
        assert_eq!(result.path, "src/player.gd");
        assert_eq!(result.diagnostics.len(), 2);
        assert_eq!(result.diagnostics[0].severity, GdScriptSeverity::Error);
        assert_eq!(result.diagnostics[0].line, Some(34));
        assert_eq!(result.diagnostics[0].column, Some(17));
        assert_eq!(
            result.diagnostics[0].raw_category.as_deref(),
            Some("gdscript")
        );
        assert_eq!(result.diagnostics[1].severity, GdScriptSeverity::Warning);
        assert_eq!(result.diagnostics[1].line, None);
        assert!(!result.truncated);
    }

    #[test]
    fn out_of_mirror_diagnostic_uris_are_rejected() {
        let payload = json!([]);
        let uri = super::super::path_to_file_uri("/elsewhere/x.gd");
        let normalized =
            normalize_publish_diagnostics(&uri, &payload, context());
        if cfg!(windows) {
            // "/elsewhere" is not under C:\mirror.
            assert!(normalized.is_none());
        }
    }

    #[test]
    fn hover_sections_are_bounded_and_masked() {
        let hover = json!({
            "contents": [
                {"kind": "markdown", "value": "### Doc\nBody"},
                {"language": "gdscript", "value": "var speed"},
                "plain note"
            ],
            "range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 9}}
        });
        let result = normalize_hover(
            &super::super::path_to_file_uri(&format!(
                "{MIRROR}/src/player.gd"
            )),
            &hover,
            context(),
        )
        .expect("hover");
        assert_eq!(result.path, "src/player.gd");
        assert_eq!(result.contents.len(), 3);
        assert_eq!(result.contents[0].kind, "markdown");
        assert_eq!(result.contents[1].kind, "plaintext");
        let range = result.range.expect("range");
        assert_eq!(range.start.line, 5);
        assert_eq!(range.start.column, 1);
    }

    #[test]
    fn completion_drops_command_attachments_and_requires_labels() {
        let completion = json!([
            {"label": "move_local_x", "kind": 2, "detail": "(x: float)", "command": {"title": "run"}},
            {"label": ""},
            {"no_label": true},
            {"label": "queue_free", "documentation": {"value": "Frees the node."}}
        ]);
        let result = normalize_completion(
            &super::super::path_to_file_uri(&format!(
                "{MIRROR}/src/player.gd"
            )),
            &completion,
            context(),
        );
        assert_eq!(result.path, "src/player.gd");
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].label, "move_local_x");
        assert_eq!(result.items[0].kind.as_deref(), Some("2"));
        assert!(result.items[0].insert_text.is_none());
        assert_eq!(
            result.items[1].documentation.as_deref(),
            Some("Frees the node.")
        );
    }

    #[test]
    fn definition_locations_map_through_the_mirror() {
        let locations = json!([
            {
                "uri": super::super::path_to_file_uri(&format!("{MIRROR}/src/other.gd")),
                "range": {"start": {"line": 9, "character": 2}, "end": {"line": 9, "character": 12}}
            },
            {"uri": super::super::path_to_file_uri("/engine/internal.gd"), "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}
        ]);
        let result = normalize_definition(
            &super::super::path_to_file_uri(&format!(
                "{MIRROR}/src/player.gd"
            )),
            &locations,
            context(),
        );
        assert_eq!(result.locations.len(), 2);
        assert_eq!(result.locations[0].path, "src/other.gd");
        assert_eq!(result.locations[0].range.start.line, 10);
        assert!(!result.locations[0].external);
        assert!(result.locations[1].external);
    }
}
