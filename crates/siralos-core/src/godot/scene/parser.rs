//! Bounded Godot scene/resource parsers (R8).
//!
//! Mirrors `packages/core/src/godot/scene/scene-parser.ts` and
//! `packages/core/src/godot/scene/resource-parser.ts`.
//! Pure, bounded, deterministic. Malformed input yields diagnostics and
//! partial/invalid documents, never panics and never executes project code.

use std::collections::{HashMap, HashSet};

use super::limits::GODOT_SCENE_LIMITS;
use super::models::{
    ExternalResourceRef, GodotDiagnosticCode, GodotDiagnosticSeverity,
    GodotParseStatus, GodotProperty, GodotResourceModel, GodotSceneModel,
    GodotSceneNode, GodotSignalConnection, GodotTextDiagnostic,
    GodotTextDocument, GodotTextDocumentKind, GodotVariantValue,
    ResourceReference, SceneReference, SourceRange, SubResourceRef,
};
use super::resolution::{ResPathResolution, resolve_res_path};
use super::text::{
    is_balanced_text, is_comment_line, parse_header_attributes,
    split_key_value,
};
use super::variant::{parse_godot_variant, parse_quoted_string};

/// Internal mutable sub-resource shape.
struct MutableSubResource {
    id: String,
    type_name: String,
    line: u32,
    properties: Vec<GodotProperty>,
}

/// Record returned by `read_record`.
struct Record {
    key: Option<String>,
    value_text: String,
    line: u32,
    end_index: usize,
}

/// Push a diagnostic if the diagnostic budget allows.
fn push_diagnostic(
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    code: GodotDiagnosticCode,
    severity: GodotDiagnosticSeverity,
    message: String,
    line: Option<u32>,
) {
    if diagnostics.len() < GODOT_SCENE_LIMITS.max_diagnostics {
        diagnostics.push(GodotTextDiagnostic {
            code,
            severity,
            message,
            line,
            column: None,
            range: None,
        });
    }
}

/// Report a truncation limit once.
fn report_limit_once(
    reported: &mut HashSet<String>,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    reason: &str,
    code: GodotDiagnosticCode,
    message: String,
    line: Option<u32>,
) {
    if reported.contains(reason) {
        return;
    }
    reported.insert(reason.to_owned());
    push_diagnostic(
        diagnostics,
        code,
        GodotDiagnosticSeverity::Error,
        message,
        line,
    );
}

fn read_attribute<'a>(
    attributes: &'a [super::text::HeaderAttribute],
    name: &str,
) -> Option<&'a super::text::HeaderAttribute> {
    attributes.iter().find(|a| a.name == name)
}

fn unquote_value(value_text: &str) -> Option<String> {
    if value_text.len() >= 2
        && value_text.starts_with('"')
        && value_text.ends_with('"')
    {
        parse_quoted_string(value_text)
    } else if value_text.is_empty() {
        None
    } else {
        Some(value_text.to_owned())
    }
}

fn read_string_attribute(
    attributes: &[super::text::HeaderAttribute],
    name: &str,
) -> Option<String> {
    read_attribute(attributes, name).and_then(|a| unquote_value(&a.value_text))
}

fn unquote_key(key: &str) -> String {
    if key.len() >= 2 && key.starts_with('"') && key.ends_with('"') {
        key[1..key.len() - 1].to_owned()
    } else {
        key.to_owned()
    }
}

fn bounded_raw_value(text: &str) -> String {
    if text.len() <= GODOT_SCENE_LIMITS.max_raw_value_length {
        text.to_owned()
    } else {
        text.chars().take(GODOT_SCENE_LIMITS.max_raw_value_length).collect()
    }
}

fn parse_section_name(inner: &str) -> Option<(String, String)> {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut name_end = 0usize;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name_end += ch.len_utf8();
        } else {
            break;
        }
    }
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_owned();
    if let Some(first) = name.chars().next() {
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }
    }
    let rest = trimmed[name_end..].trim_start().to_owned();
    Some((name, rest))
}

fn make_property_scene(
    key: &str,
    value_text: &str,
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> GodotProperty {
    let parsed = parse_godot_variant(value_text);
    if parsed.truncated {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::SceneValueTruncated,
            GodotDiagnosticSeverity::Warning,
            format!(
                "The value of \"{key}\" at line {line} exceeds interpretation bounds and was preserved partially."
            ),
            Some(line),
        );
    }
    GodotProperty {
        name: unquote_key(key),
        value: parsed.value,
        raw_value: bounded_raw_value(value_text),
        line: Some(line),
    }
}

fn make_property_resource(
    key: &str,
    value_text: &str,
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> GodotProperty {
    let parsed = parse_godot_variant(value_text);
    if parsed.truncated {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::ResourceValueTruncated,
            GodotDiagnosticSeverity::Warning,
            format!(
                "The value of \"{key}\" at line {line} exceeds interpretation bounds and was preserved partially."
            ),
            Some(line),
        );
    }
    GodotProperty {
        name: unquote_key(key),
        value: parsed.value,
        raw_value: bounded_raw_value(value_text),
        line: Some(line),
    }
}

fn read_record(
    lines: &[String],
    start_index: usize,
    line_count: usize,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    kind: GodotTextDocumentKind,
) -> Record {
    let line_number = (start_index + 1) as u32;
    let first_line = &lines[start_index];
    if let Some((key, value_start)) = split_key_value(first_line) {
        let mut value_text = first_line[value_start..].trim().to_owned();
        let mut end_index = start_index;
        let mut continuation = 0usize;
        while !is_balanced_text(&value_text)
            && continuation < GODOT_SCENE_LIMITS.max_value_continuation_lines
        {
            end_index += 1;
            continuation += 1;
            if end_index >= line_count {
                break;
            }
            let next_line = &lines[end_index];
            let next_trimmed = next_line.trim();
            if next_trimmed.is_empty() || is_comment_line(next_trimmed) {
                continue;
            }
            value_text = format!("{value_text}\n{}", next_line.trim());
        }
        if !is_balanced_text(&value_text) {
            let (code, prefix) = match kind {
                GodotTextDocumentKind::Scene => {
                    (GodotDiagnosticCode::SceneUnbalancedValue, "scene")
                }
                GodotTextDocumentKind::Resource => {
                    (GodotDiagnosticCode::ResourceUnbalancedValue, "resource")
                }
            };
            let _ = prefix;
            push_diagnostic(
                diagnostics,
                code,
                GodotDiagnosticSeverity::Error,
                format!(
                    "The value of \"{key}\" at line {line_number} is unbalanced; it was truncated at the continuation bound."
                ),
                Some(line_number),
            );
        }
        Record { key: Some(key), value_text, line: line_number, end_index }
    } else {
        let code = match kind {
            GodotTextDocumentKind::Scene => {
                GodotDiagnosticCode::SceneUnknownProperty
            }
            GodotTextDocumentKind::Resource => {
                GodotDiagnosticCode::ResourceUnknownProperty
            }
        };
        push_diagnostic(
            diagnostics,
            code,
            GodotDiagnosticSeverity::Warning,
            format!(
                "Unrecognized record without a value at line {line_number}."
            ),
            Some(line_number),
        );
        Record {
            key: None,
            value_text: String::new(),
            line: line_number,
            end_index: start_index,
        }
    }
}

fn parse_scene_header(
    attributes: &[super::text::HeaderAttribute],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    line: u32,
) -> (Option<u32>, Option<u32>, Option<String>) {
    let mut format: Option<u32> = None;
    let mut load_steps: Option<u32> = None;
    let mut uid: Option<String> = None;
    for attr in attributes {
        if attr.name == "format" {
            if let Ok(parsed) = attr.value_text.parse::<i64>() {
                if parsed >= 0 {
                    if let Ok(v) = u32::try_from(parsed) {
                        format = Some(v);
                    }
                } else {
                    push_diagnostic(
                        diagnostics,
                        GodotDiagnosticCode::SceneUnknownHeaderAttribute,
                        GodotDiagnosticSeverity::Warning,
                        format!("Unsupported format value at line {line}."),
                        Some(line),
                    );
                }
            } else {
                push_diagnostic(
                    diagnostics,
                    GodotDiagnosticCode::SceneUnknownHeaderAttribute,
                    GodotDiagnosticSeverity::Warning,
                    format!("Unsupported format value at line {line}."),
                    Some(line),
                );
            }
        } else if attr.name == "load_steps" {
            if let Ok(parsed) = attr.value_text.parse::<i64>() {
                if parsed >= 0 {
                    if let Ok(v) = u32::try_from(parsed) {
                        load_steps = Some(v);
                    }
                }
            }
        } else if attr.name == "uid" {
            if let Some(v) = unquote_value(&attr.value_text) {
                if v.starts_with("uid://") {
                    uid = Some(v);
                }
            }
        }
    }
    (format, load_steps, uid)
}

fn parse_resource_header(
    attributes: &[super::text::HeaderAttribute],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    line: u32,
) -> (String, Option<u32>, Option<u32>, Option<String>) {
    let type_name =
        read_string_attribute(attributes, "type").unwrap_or_default();
    if type_name.is_empty() {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::ResourceMalformedSection,
            GodotDiagnosticSeverity::Error,
            format!(
                "The resource header at line {line} is missing its type attribute."
            ),
            Some(line),
        );
    }
    let mut format: Option<u32> = None;
    let mut load_steps: Option<u32> = None;
    let mut uid: Option<String> = None;
    for attr in attributes {
        if attr.name == "format" {
            if let Ok(parsed) = attr.value_text.parse::<i64>() {
                if parsed >= 0 {
                    if let Ok(v) = u32::try_from(parsed) {
                        format = Some(v);
                    }
                }
            }
        } else if attr.name == "load_steps" {
            if let Ok(parsed) = attr.value_text.parse::<i64>() {
                if parsed >= 0 {
                    if let Ok(v) = u32::try_from(parsed) {
                        load_steps = Some(v);
                    }
                }
            }
        } else if attr.name == "uid" {
            if let Some(v) = unquote_value(&attr.value_text) {
                if v.starts_with("uid://") {
                    uid = Some(v);
                }
            }
        }
    }
    (type_name, format, load_steps, uid)
}

fn parse_ext_resource_scene(
    attributes: &[super::text::HeaderAttribute],
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> Option<ExternalResourceRef> {
    let Some(id) = read_string_attribute(attributes, "id") else {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::SceneMissingResourceId,
            GodotDiagnosticSeverity::Error,
            format!(
                "ext_resource at line {line} is missing its id attribute."
            ),
            Some(line),
        );
        return None;
    };
    let type_name = read_string_attribute(attributes, "type");
    let path = read_string_attribute(attributes, "path");
    let uid = read_string_attribute(attributes, "uid");
    Some(ExternalResourceRef { id, type_name, path, uid, line: Some(line) })
}

fn parse_ext_resource_resource(
    attributes: &[super::text::HeaderAttribute],
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> Option<ExternalResourceRef> {
    let Some(id) = read_string_attribute(attributes, "id") else {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::ResourceMissingResourceId,
            GodotDiagnosticSeverity::Error,
            format!(
                "ext_resource at line {line} is missing its id attribute."
            ),
            Some(line),
        );
        return None;
    };
    let type_name = read_string_attribute(attributes, "type");
    let path = read_string_attribute(attributes, "path");
    let uid = read_string_attribute(attributes, "uid");
    Some(ExternalResourceRef { id, type_name, path, uid, line: Some(line) })
}

fn parse_instance_reference(value_text: &str) -> Option<SceneReference> {
    let parsed = parse_godot_variant(value_text);
    if let GodotVariantValue::ExtResource(id) = parsed.value {
        Some(SceneReference {
            resource: ExternalResourceRef {
                id,
                type_name: None,
                path: None,
                uid: None,
                line: None,
            },
            resolved_path: None,
        })
    } else {
        None
    }
}

fn parse_group_list(
    attributes: &[super::text::HeaderAttribute],
) -> Vec<String> {
    let Some(attr) = read_attribute(attributes, "groups") else {
        return Vec::new();
    };
    let parsed = parse_godot_variant(&attr.value_text);
    let GodotVariantValue::Array(items) = parsed.value else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    for item in items {
        if groups.len() >= GODOT_SCENE_LIMITS.max_groups_per_node {
            break;
        }
        match item {
            GodotVariantValue::String(s)
            | GodotVariantValue::StringName(s) => groups.push(s),
            _ => {}
        }
    }
    groups
}

fn parse_node(
    attributes: &[super::text::HeaderAttribute],
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> GodotSceneNode {
    let name = read_string_attribute(attributes, "name");
    if name.is_none() {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::SceneMalformedSection,
            GodotDiagnosticSeverity::Error,
            format!("[node] at line {line} is missing its name attribute."),
            Some(line),
        );
    }
    let type_name = read_string_attribute(attributes, "type");
    let parent_path = read_string_attribute(attributes, "parent");
    let owner_path = read_string_attribute(attributes, "owner");
    let instance = read_attribute(attributes, "instance")
        .and_then(|a| parse_instance_reference(&a.value_text));
    let groups = parse_group_list(attributes);
    let mut raw_attributes = Vec::new();
    for attr in attributes {
        if matches!(
            attr.name.as_str(),
            "name" | "type" | "parent" | "owner" | "instance" | "groups"
        ) {
            continue;
        }
        raw_attributes.push((attr.name.clone(), attr.value_text.clone()));
    }
    GodotSceneNode {
        name: name.unwrap_or_default(),
        type_name,
        parent_path,
        owner_path,
        instance,
        script: None,
        groups,
        properties: Vec::new(),
        raw_attributes,
        source_range: Some(SourceRange {
            start_line: line,
            start_column: 1,
            end_line: line,
            end_column: 1,
        }),
    }
}

fn parse_connection(
    attributes: &[super::text::HeaderAttribute],
    line: u32,
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> Option<GodotSignalConnection> {
    let signal_val = read_string_attribute(attributes, "signal");
    let from_val = read_string_attribute(attributes, "from");
    let to_val = read_string_attribute(attributes, "to");
    let method_val = read_string_attribute(attributes, "method");
    let (Some(signal), Some(from), Some(to), Some(method)) =
        (signal_val, from_val, to_val, method_val)
    else {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::SceneMalformedSection,
            GodotDiagnosticSeverity::Error,
            format!(
                "Connection at line {line} requires signal, from, to, and method attributes."
            ),
            Some(line),
        );
        return None;
    };
    let mut flags: Option<u32> = None;
    if let Some(attr) = read_attribute(attributes, "flags") {
        if let Ok(parsed) = attr.value_text.parse::<i64>() {
            if let Ok(v) = u32::try_from(parsed) {
                flags = Some(v);
            } else if parsed >= 0 {
                // Fallback for large values that still fit i64 but not u32
                flags = None;
            }
        }
    }
    let mut binds: Option<Vec<GodotVariantValue>> = None;
    if let Some(attr) = read_attribute(attributes, "binds") {
        let parsed = parse_godot_variant(&attr.value_text);
        if let GodotVariantValue::Array(items) = parsed.value {
            binds = Some(items);
        }
    }
    Some(GodotSignalConnection {
        signal,
        from,
        to,
        method,
        flags,
        binds,
        line: Some(line),
    })
}

fn resolve_scene_reference(
    reference: &SceneReference,
    external_resources: &[ExternalResourceRef],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
    severity: GodotDiagnosticSeverity,
) -> SceneReference {
    let Some(declared) =
        external_resources.iter().find(|r| r.id == reference.resource.id)
    else {
        push_diagnostic(
            diagnostics,
            GodotDiagnosticCode::SceneUnknownResourceReference,
            severity,
            format!(
                "Unknown resource reference ExtResource(\"{}\" ) — no matching ext_resource declaration.",
                reference.resource.id
            ),
            None,
        );
        return reference.clone();
    };
    let resolved_path = if let Some(path) = &declared.path {
        match resolve_res_path(path) {
            ResPathResolution::Ok(rel) => Some(rel),
            ResPathResolution::Err(_) => None,
        }
    } else {
        None
    };
    SceneReference { resource: declared.clone(), resolved_path }
}

fn resolve_script_reference_scene(
    properties: &[GodotProperty],
    external_resources: &[ExternalResourceRef],
    sub_resources: &[SubResourceRef],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> Option<ResourceReference> {
    let script_property = properties.iter().find(|p| p.name == "script")?;
    match &script_property.value {
        GodotVariantValue::ExtResource(id) => {
            let Some(declared) =
                external_resources.iter().find(|r| &r.id == id)
            else {
                push_diagnostic(
                    diagnostics,
                    GodotDiagnosticCode::SceneUnknownResourceReference,
                    GodotDiagnosticSeverity::Warning,
                    format!(
                        "Unknown script reference ExtResource(\"{id}\") — no matching ext_resource declaration."
                    ),
                    script_property.line,
                );
                return None;
            };
            let resolved_path = if let Some(path) = &declared.path {
                match resolve_res_path(path) {
                    ResPathResolution::Ok(rel) => Some(rel),
                    ResPathResolution::Err(_) => None,
                }
            } else {
                None
            };
            Some(ResourceReference {
                resource: declared.clone(),
                resolved_path,
            })
        }
        GodotVariantValue::SubResource(id) => {
            let Some(declared) = sub_resources.iter().find(|r| &r.id == id)
            else {
                push_diagnostic(
                    diagnostics,
                    GodotDiagnosticCode::SceneUnknownResourceReference,
                    GodotDiagnosticSeverity::Warning,
                    format!(
                        "Unknown script reference SubResource(\"{id}\") — no matching sub_resource declaration."
                    ),
                    script_property.line,
                );
                return None;
            };
            Some(ResourceReference {
                resource: ExternalResourceRef {
                    id: declared.id.clone(),
                    type_name: Some(declared.type_name.clone()),
                    path: None,
                    uid: None,
                    line: declared.line,
                },
                resolved_path: None,
            })
        }
        _ => None,
    }
}

fn resolve_script_reference_resource(
    script_property: Option<&GodotProperty>,
    external_resources: &[ExternalResourceRef],
    sub_resources: &[SubResourceRef],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) -> Option<ResourceReference> {
    let prop = script_property?;
    match &prop.value {
        GodotVariantValue::ExtResource(id) => {
            let Some(declared) =
                external_resources.iter().find(|r| &r.id == id)
            else {
                push_diagnostic(
                    diagnostics,
                    GodotDiagnosticCode::ResourceUnknownResourceReference,
                    GodotDiagnosticSeverity::Warning,
                    format!(
                        "Unknown script reference ExtResource(\"{id}\") — no matching ext_resource declaration."
                    ),
                    prop.line,
                );
                return None;
            };
            let resolved_path = if let Some(path) = &declared.path {
                match resolve_res_path(path) {
                    ResPathResolution::Ok(rel) => Some(rel),
                    ResPathResolution::Err(_) => None,
                }
            } else {
                None
            };
            Some(ResourceReference {
                resource: declared.clone(),
                resolved_path,
            })
        }
        GodotVariantValue::SubResource(id) => {
            let Some(declared) = sub_resources.iter().find(|r| &r.id == id)
            else {
                push_diagnostic(
                    diagnostics,
                    GodotDiagnosticCode::ResourceUnknownResourceReference,
                    GodotDiagnosticSeverity::Warning,
                    format!(
                        "Unknown script reference SubResource(\"{id}\") — no matching sub_resource declaration."
                    ),
                    prop.line,
                );
                return None;
            };
            Some(ResourceReference {
                resource: ExternalResourceRef {
                    id: declared.id.clone(),
                    type_name: Some(declared.type_name.clone()),
                    path: None,
                    uid: None,
                    line: declared.line,
                },
                resolved_path: None,
            })
        }
        _ => None,
    }
}

fn node_paths(nodes: &[GodotSceneNode]) -> Vec<String> {
    let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for node in nodes {
        let parent =
            node.parent_path.clone().unwrap_or_else(|| ".".to_owned());
        by_parent.entry(parent).or_default().push(node.name.clone());
    }
    let mut paths: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    fn walk(
        parent_path: &str,
        by_parent: &HashMap<String, Vec<String>>,
        paths: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        let Some(children) = by_parent.get(parent_path) else {
            return;
        };
        for name in children {
            let path = if parent_path == "." {
                name.clone()
            } else {
                format!("{parent_path}/{name}")
            };
            if seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());
            paths.push(path.clone());
            walk(&path, by_parent, paths, seen);
        }
    }
    walk(".", &by_parent, &mut paths, &mut seen);
    let mut ordered: Vec<String> = Vec::new();
    ordered.push(".".to_owned());
    if let Some(root) = nodes.first() {
        if !root.name.is_empty() {
            ordered.push(root.name.clone());
        }
    }
    ordered.extend(paths);
    ordered
}

fn path_exists(connection_path: &str, paths: &[String]) -> bool {
    if connection_path == "." {
        return true;
    }
    if paths.contains(&connection_path.to_owned()) {
        return true;
    }
    let segments: Vec<&str> = connection_path.split('/').collect();
    for index in (1..segments.len()).rev() {
        let prefix = segments[..index].join("/");
        if paths.contains(&prefix) {
            return true;
        }
    }
    false
}

fn validate_parents_and_connections(
    nodes: &[GodotSceneNode],
    connections: &[GodotSignalConnection],
    diagnostics: &mut Vec<GodotTextDiagnostic>,
) {
    if nodes.is_empty() {
        return;
    }
    let paths = node_paths(nodes);
    for (idx, node) in nodes.iter().enumerate() {
        let effective_parent: Option<String> =
            if let Some(p) = &node.parent_path {
                Some(p.clone())
            } else if idx == 0 {
                Some(".".to_owned())
            } else {
                None
            };
        let Some(effective) = effective_parent else {
            push_diagnostic(
                diagnostics,
                GodotDiagnosticCode::SceneUnresolvedParent,
                GodotDiagnosticSeverity::Warning,
                format!(
                    "Node \"{}\" has no parent attribute and is not the root node; its parent relationship is unresolved.",
                    node.name
                ),
                node.source_range.map(|r| r.start_line),
            );
            continue;
        };
        if effective != "." && !paths.contains(&effective) {
            push_diagnostic(
                diagnostics,
                GodotDiagnosticCode::SceneUnresolvedParent,
                GodotDiagnosticSeverity::Warning,
                format!(
                    "Node \"{}\" declares parent \"{effective}\" which is not a declared node in this scene.",
                    node.name
                ),
                node.source_range.map(|r| r.start_line),
            );
        }
    }
    for conn in connections {
        if !path_exists(&conn.from, &paths) {
            push_diagnostic(
                diagnostics,
                GodotDiagnosticCode::SceneMissingSignalSource,
                GodotDiagnosticSeverity::Warning,
                format!(
                    "Connection \"{}\" references source node \"{}\" which is not declared in this scene.",
                    conn.signal, conn.from
                ),
                conn.line,
            );
        }
        if !path_exists(&conn.to, &paths) {
            push_diagnostic(
                diagnostics,
                GodotDiagnosticCode::SceneMissingSignalTarget,
                GodotDiagnosticSeverity::Warning,
                format!(
                    "Connection \"{}\" references target node \"{}\" which is not declared in this scene.",
                    conn.signal, conn.to
                ),
                conn.line,
            );
        }
    }
}

/// Parse a Godot `.tscn` text document into a bounded semantic model.
///
/// Pure, bounded, deterministic. Malformed input yields diagnostics and a
/// partial/invalid document, never panics.
#[must_use]
pub fn parse_godot_scene(
    content: &str,
    path: &str,
    revision: Option<String>,
) -> GodotTextDocument<GodotSceneModel> {
    let mut diagnostics: Vec<GodotTextDiagnostic> = Vec::new();
    let mut reported_limits: HashSet<String> = HashSet::new();
    let mut truncated = false;

    let raw_lines: Vec<String> = content
        .split('\n')
        .map(|s| {
            if let Some(stripped) = s.strip_suffix('\r') {
                stripped.to_owned()
            } else {
                s.to_owned()
            }
        })
        .collect();

    let line_count =
        std::cmp::min(raw_lines.len(), GODOT_SCENE_LIMITS.max_lines);
    if raw_lines.len() > GODOT_SCENE_LIMITS.max_lines {
        truncated = true;
        push_diagnostic(
            &mut diagnostics,
            GodotDiagnosticCode::SceneDocumentTruncated,
            GodotDiagnosticSeverity::Error,
            "The document exceeds the line bound; parsing stopped.".to_owned(),
            None,
        );
    }

    let mut external_resources: Vec<ExternalResourceRef> = Vec::new();
    let mut sub_resources: Vec<MutableSubResource> = Vec::new();
    let mut nodes: Vec<GodotSceneNode> = Vec::new();
    let mut connections: Vec<GodotSignalConnection> = Vec::new();
    let mut editable_instances: Vec<String> = Vec::new();
    let mut ext_ids: HashSet<String> = HashSet::new();
    let mut sub_ids: HashSet<String> = HashSet::new();

    let mut header_format: Option<u32> = None;
    let mut header_load_steps: Option<u32> = None;
    let mut header_uid: Option<String> = None;
    let mut seen_scene_header = false;
    let mut header_present = false;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Header,
        ExtResource,
        SubResource,
        Node,
        Connection,
        Editable,
        Body,
    }
    let mut current_section = Section::Header;
    let mut current_sub: Option<usize> = None;
    let mut current_node: Option<usize> = None;
    let mut section_count: usize = 0;
    let mut resource_count: usize = 0;
    let mut property_count: usize = 0;
    let mut index: usize = 0;
    while index < line_count {
        let raw_line = &raw_lines[index];
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || is_comment_line(trimmed) {
            index += 1;
            continue;
        }
        if trimmed.starts_with('[') {
            section_count += 1;
            if section_count > GODOT_SCENE_LIMITS.max_sections {
                truncated = true;
                report_limit_once(
                    &mut reported_limits,
                    &mut diagnostics,
                    "sections",
                    GodotDiagnosticCode::SceneDocumentTruncated,
                    format!(
                        "The section count exceeded the bound ({}); parsing stopped.",
                        GODOT_SCENE_LIMITS.max_sections
                    ),
                    None,
                );
                break;
            }
            let Some(close_idx) = trimmed.rfind(']') else {
                push_diagnostic(
                    &mut diagnostics,
                    GodotDiagnosticCode::SceneMalformedSection,
                    GodotDiagnosticSeverity::Error,
                    format!(
                        "Malformed section header at line {}: missing closing bracket.",
                        index + 1
                    ),
                    Some((index + 1) as u32),
                );
                current_section = Section::Body;
                current_sub = None;
                current_node = None;
                index += 1;
                continue;
            };
            let inner = &trimmed[1..close_idx];
            let Some((section_name, attrs_text)) = parse_section_name(inner)
            else {
                push_diagnostic(
                    &mut diagnostics,
                    GodotDiagnosticCode::SceneMalformedSection,
                    GodotDiagnosticSeverity::Error,
                    format!("Malformed section header at line {}.", index + 1),
                    Some((index + 1) as u32),
                );
                current_section = Section::Body;
                index += 1;
                continue;
            };
            let (attributes, _) = parse_header_attributes(
                &attrs_text,
                GODOT_SCENE_LIMITS.max_header_attributes,
            );
            match section_name.as_str() {
                "gd_scene" => {
                    if seen_scene_header {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::SceneUnexpectedHeader,
                            GodotDiagnosticSeverity::Error,
                            format!(
                                "Unexpected duplicate scene header at line {}.",
                                index + 1
                            ),
                            Some((index + 1) as u32),
                        );
                    }
                    seen_scene_header = true;
                    header_present = true;
                    let (fmt, ls, uid) = parse_scene_header(
                        &attributes,
                        &mut diagnostics,
                        (index + 1) as u32,
                    );
                    header_format = fmt;
                    header_load_steps = ls;
                    header_uid = uid;
                    current_section = Section::Header;
                    current_sub = None;
                    current_node = None;
                }
                "gd_resource" => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::SceneUnexpectedHeader,
                        GodotDiagnosticSeverity::Error,
                        format!(
                            "A resource header ([gd_resource]) is not valid inside a .tscn document (line {}).",
                            index + 1
                        ),
                        Some((index + 1) as u32),
                    );
                    current_section = Section::Header;
                }
                "ext_resource" => {
                    current_section = Section::ExtResource;
                    current_sub = None;
                    current_node = None;
                    if resource_count >= GODOT_SCENE_LIMITS.max_resources {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "resources",
                            GodotDiagnosticCode::SceneDocumentTruncated,
                            format!(
                                "The resource count exceeded the bound ({}); remaining resources are ignored.",
                                GODOT_SCENE_LIMITS.max_resources
                            ),
                            Some((index + 1) as u32),
                        );
                    } else if let Some(r) = parse_ext_resource_scene(
                        &attributes,
                        (index + 1) as u32,
                        &mut diagnostics,
                    ) {
                        resource_count += 1;
                        if ext_ids.contains(&r.id) {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::SceneDuplicateResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "Duplicate ext_resource id \"{}\" at line {}; the later declaration is ignored.",
                                    r.id,
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                        } else {
                            ext_ids.insert(r.id.clone());
                            external_resources.push(r);
                        }
                    }
                }
                "sub_resource" => {
                    current_section = Section::SubResource;
                    current_node = None;
                    if resource_count >= GODOT_SCENE_LIMITS.max_resources {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "resources",
                            GodotDiagnosticCode::SceneDocumentTruncated,
                            format!(
                                "The resource count exceeded the bound ({}); remaining resources are ignored.",
                                GODOT_SCENE_LIMITS.max_resources
                            ),
                            Some((index + 1) as u32),
                        );
                        current_sub = None;
                    } else {
                        resource_count += 1;
                        let type_name =
                            read_string_attribute(&attributes, "type")
                                .unwrap_or_default();
                        let Some(id) =
                            read_string_attribute(&attributes, "id")
                        else {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::SceneMissingResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "sub_resource at line {} is missing its id attribute.",
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                            current_sub = None;
                            index += 1;
                            continue;
                        };
                        if sub_ids.contains(&id) {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::SceneDuplicateResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "Duplicate sub_resource id \"{id}\" at line {}; the later declaration is ignored.",
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                            current_sub = None;
                        } else {
                            sub_ids.insert(id.clone());
                            sub_resources.push(MutableSubResource {
                                id,
                                type_name,
                                line: (index + 1) as u32,
                                properties: Vec::new(),
                            });
                            current_sub = Some(sub_resources.len() - 1);
                        }
                    }
                }
                "node" => {
                    current_section = Section::Node;
                    current_sub = None;
                    if nodes.len() >= GODOT_SCENE_LIMITS.max_nodes {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "nodes",
                            GodotDiagnosticCode::SceneDocumentTruncated,
                            format!(
                                "The node count exceeded the bound ({}); remaining nodes are ignored.",
                                GODOT_SCENE_LIMITS.max_nodes
                            ),
                            Some((index + 1) as u32),
                        );
                        current_node = None;
                    } else {
                        let node = parse_node(
                            &attributes,
                            (index + 1) as u32,
                            &mut diagnostics,
                        );
                        nodes.push(node);
                        current_node = Some(nodes.len() - 1);
                    }
                }
                "connection" => {
                    current_section = Section::Connection;
                    current_sub = None;
                    current_node = None;
                    if connections.len() >= GODOT_SCENE_LIMITS.max_connections
                    {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "connections",
                            GodotDiagnosticCode::SceneDocumentTruncated,
                            format!(
                                "The connection count exceeded the bound ({}); remaining connections are ignored.",
                                GODOT_SCENE_LIMITS.max_connections
                            ),
                            Some((index + 1) as u32),
                        );
                    } else if let Some(conn) = parse_connection(
                        &attributes,
                        (index + 1) as u32,
                        &mut diagnostics,
                    ) {
                        connections.push(conn);
                    }
                }
                "editable" => {
                    current_section = Section::Editable;
                    current_sub = None;
                    current_node = None;
                    if let Some(path_attr) =
                        read_string_attribute(&attributes, "path")
                    {
                        if editable_instances.len()
                            < GODOT_SCENE_LIMITS.max_editable_instances
                        {
                            editable_instances.push(path_attr);
                        } else {
                            truncated = true;
                        }
                    } else {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::SceneMalformedSection,
                            GodotDiagnosticSeverity::Warning,
                            format!(
                                "[editable] at line {} is missing its path attribute.",
                                index + 1
                            ),
                            Some((index + 1) as u32),
                        );
                    }
                }
                _ => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::SceneMalformedSection,
                        GodotDiagnosticSeverity::Error,
                        format!(
                            "Unknown section header \"[{section_name}]\" at line {}.",
                            index + 1
                        ),
                        Some((index + 1) as u32),
                    );
                    current_section = Section::Body;
                    current_node = None;
                    current_sub = None;
                }
            }
            index += 1;
            continue;
        }

        let record = read_record(
            &raw_lines,
            index,
            line_count,
            &mut diagnostics,
            GodotTextDocumentKind::Scene,
        );
        let end = record.end_index;
        if let Some(key) = record.key {
            match current_section {
                Section::Node => {
                    if let Some(node_idx) = current_node {
                        if property_count >= GODOT_SCENE_LIMITS.max_properties
                        {
                            truncated = true;
                            report_limit_once(
                                &mut reported_limits,
                                &mut diagnostics,
                                "properties",
                                GodotDiagnosticCode::SceneDocumentTruncated,
                                format!(
                                    "The property count exceeded the bound ({}); remaining properties are ignored.",
                                    GODOT_SCENE_LIMITS.max_properties
                                ),
                                Some(record.line),
                            );
                        } else {
                            property_count += 1;
                            let prop = make_property_scene(
                                &key,
                                &record.value_text,
                                record.line,
                                &mut diagnostics,
                            );
                            if let Some(node) = nodes.get_mut(node_idx) {
                                node.properties.push(prop);
                            }
                        }
                    } else {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::SceneUnknownProperty,
                            GodotDiagnosticSeverity::Warning,
                            format!(
                                "Property \"{key}\" at line {} is not valid in the current section and was ignored.",
                                record.line
                            ),
                            Some(record.line),
                        );
                    }
                }
                Section::SubResource => {
                    if let Some(sub_idx) = current_sub {
                        if property_count >= GODOT_SCENE_LIMITS.max_properties
                        {
                            truncated = true;
                            report_limit_once(
                                &mut reported_limits,
                                &mut diagnostics,
                                "properties",
                                GodotDiagnosticCode::SceneDocumentTruncated,
                                format!(
                                    "The property count exceeded the bound ({}); remaining properties are ignored.",
                                    GODOT_SCENE_LIMITS.max_properties
                                ),
                                Some(record.line),
                            );
                        } else {
                            property_count += 1;
                            let prop = make_property_scene(
                                &key,
                                &record.value_text,
                                record.line,
                                &mut diagnostics,
                            );
                            if let Some(sub) = sub_resources.get_mut(sub_idx) {
                                sub.properties.push(prop);
                            }
                        }
                    } else {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::SceneUnknownProperty,
                            GodotDiagnosticSeverity::Warning,
                            format!(
                                "Property \"{key}\" at line {} is not valid in the current section and was ignored.",
                                record.line
                            ),
                            Some(record.line),
                        );
                    }
                }
                _ => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::SceneUnknownProperty,
                        GodotDiagnosticSeverity::Warning,
                        format!(
                            "Property \"{key}\" at line {} is not valid in the current section and was ignored.",
                            record.line
                        ),
                        Some(record.line),
                    );
                }
            }
        }
        index = end + 1;
    }

    let sub_resources_model: Vec<super::models::SubResourceRef> =
        sub_resources
            .into_iter()
            .map(|s| super::models::SubResourceRef {
                id: s.id,
                type_name: s.type_name,
                properties: s.properties,
                line: Some(s.line),
            })
            .collect();

    let document: Option<GodotSceneModel> = if !header_present {
        None
    } else {
        let mut resolved_nodes: Vec<GodotSceneNode> = Vec::new();
        for mut node in nodes {
            let script = resolve_script_reference_scene(
                &node.properties,
                &external_resources,
                &sub_resources_model,
                &mut diagnostics,
            );
            let instance = node.instance.take().map(|inst| {
                resolve_scene_reference(
                    &inst,
                    &external_resources,
                    &mut diagnostics,
                    GodotDiagnosticSeverity::Error,
                )
            });
            node.instance = instance;
            node.script = script;
            resolved_nodes.push(node);
        }
        let root = resolved_nodes.iter().find(|n| {
            n.parent_path.is_none() || n.parent_path.as_deref() == Some(".")
        });
        let base_scene = root.and_then(|r| r.instance.clone());
        validate_parents_and_connections(
            &resolved_nodes,
            &connections,
            &mut diagnostics,
        );
        Some(GodotSceneModel {
            path: path.to_owned(),
            revision: revision.clone(),
            uid: header_uid,
            format: header_format,
            load_steps: header_load_steps,
            base_scene,
            external_resources,
            sub_resources: sub_resources_model,
            nodes: resolved_nodes,
            connections,
            editable_instances,
        })
    };

    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity == GodotDiagnosticSeverity::Error)
        .count();
    let status = if !seen_scene_header || document.is_none() {
        GodotParseStatus::Invalid
    } else if error_count == 0 {
        GodotParseStatus::Complete
    } else {
        GodotParseStatus::Partial
    };

    GodotTextDocument {
        path: path.to_owned(),
        revision,
        kind: GodotTextDocumentKind::Scene,
        status,
        document,
        diagnostics,
        truncated,
    }
}

/// Parse a Godot `.tres` (or other text resource) document into a bounded model.
///
/// Pure, bounded, deterministic. Malformed input yields diagnostics and a
/// partial/invalid document, never panics.
#[must_use]
pub fn parse_godot_resource(
    content: &str,
    path: &str,
    revision: Option<String>,
) -> GodotTextDocument<GodotResourceModel> {
    let mut diagnostics: Vec<GodotTextDiagnostic> = Vec::new();
    let mut reported_limits: HashSet<String> = HashSet::new();
    let mut truncated = false;

    let raw_lines: Vec<String> = content
        .split('\n')
        .map(|s| {
            if let Some(stripped) = s.strip_suffix('\r') {
                stripped.to_owned()
            } else {
                s.to_owned()
            }
        })
        .collect();

    let line_count =
        std::cmp::min(raw_lines.len(), GODOT_SCENE_LIMITS.max_lines);
    if raw_lines.len() > GODOT_SCENE_LIMITS.max_lines {
        truncated = true;
        push_diagnostic(
            &mut diagnostics,
            GodotDiagnosticCode::ResourceDocumentTruncated,
            GodotDiagnosticSeverity::Error,
            "The document exceeds the line bound; parsing stopped.".to_owned(),
            None,
        );
    }

    let mut external_resources: Vec<ExternalResourceRef> = Vec::new();
    let mut sub_resources: Vec<MutableSubResource> = Vec::new();
    struct RawProperty {
        name: String,
        value_text: String,
        line: u32,
    }
    let mut properties_raw: Vec<RawProperty> = Vec::new();
    let mut ext_ids: HashSet<String> = HashSet::new();
    let mut sub_ids: HashSet<String> = HashSet::new();

    let mut header_type: Option<String> = None;
    let mut header_format: Option<u32> = None;
    let mut header_load_steps: Option<u32> = None;
    let mut header_uid: Option<String> = None;
    let mut seen_resource_header = false;
    let mut header_present = false;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Header,
        ExtResource,
        SubResource,
        Resource,
        Body,
    }
    let mut current_section = Section::Header;
    let mut current_sub: Option<usize> = None;
    let mut section_count: usize = 0;
    let mut resource_count: usize = 0;
    let mut property_count: usize = 0;

    let mut index: usize = 0;
    while index < line_count {
        let raw_line = &raw_lines[index];
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || is_comment_line(trimmed) {
            index += 1;
            continue;
        }
        if trimmed.starts_with('[') {
            section_count += 1;
            if section_count > GODOT_SCENE_LIMITS.max_sections {
                truncated = true;
                report_limit_once(
                    &mut reported_limits,
                    &mut diagnostics,
                    "sections",
                    GodotDiagnosticCode::ResourceDocumentTruncated,
                    format!(
                        "The section count exceeded the bound ({}); parsing stopped.",
                        GODOT_SCENE_LIMITS.max_sections
                    ),
                    None,
                );
                break;
            }
            let Some(close_idx) = trimmed.rfind(']') else {
                push_diagnostic(
                    &mut diagnostics,
                    GodotDiagnosticCode::ResourceMalformedSection,
                    GodotDiagnosticSeverity::Error,
                    format!(
                        "Malformed section header at line {}: missing closing bracket.",
                        index + 1
                    ),
                    Some((index + 1) as u32),
                );
                current_section = Section::Body;
                current_sub = None;
                index += 1;
                continue;
            };
            let inner = &trimmed[1..close_idx];
            let Some((section_name, attrs_text)) = parse_section_name(inner)
            else {
                push_diagnostic(
                    &mut diagnostics,
                    GodotDiagnosticCode::ResourceMalformedSection,
                    GodotDiagnosticSeverity::Error,
                    format!("Malformed section header at line {}.", index + 1),
                    Some((index + 1) as u32),
                );
                current_section = Section::Body;
                index += 1;
                continue;
            };
            let (attributes, _) = parse_header_attributes(
                &attrs_text,
                GODOT_SCENE_LIMITS.max_header_attributes,
            );
            match section_name.as_str() {
                "gd_resource" => {
                    if seen_resource_header {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::ResourceUnexpectedHeader,
                            GodotDiagnosticSeverity::Error,
                            format!(
                                "Unexpected duplicate resource header at line {}.",
                                index + 1
                            ),
                            Some((index + 1) as u32),
                        );
                    }
                    seen_resource_header = true;
                    header_present = true;
                    let (t, fmt, ls, uid) = parse_resource_header(
                        &attributes,
                        &mut diagnostics,
                        (index + 1) as u32,
                    );
                    header_type = Some(t);
                    header_format = fmt;
                    header_load_steps = ls;
                    header_uid = uid;
                    current_section = Section::Header;
                    current_sub = None;
                }
                "gd_scene" => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::ResourceUnexpectedHeader,
                        GodotDiagnosticSeverity::Error,
                        format!(
                            "A scene header ([gd_scene]) is not valid inside a .tres document (line {}).",
                            index + 1
                        ),
                        Some((index + 1) as u32),
                    );
                    current_section = Section::Header;
                }
                "ext_resource" => {
                    current_section = Section::ExtResource;
                    current_sub = None;
                    if resource_count >= GODOT_SCENE_LIMITS.max_resources {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "resources",
                            GodotDiagnosticCode::ResourceDocumentTruncated,
                            format!(
                                "The resource count exceeded the bound ({}); remaining resources are ignored.",
                                GODOT_SCENE_LIMITS.max_resources
                            ),
                            Some((index + 1) as u32),
                        );
                    } else if let Some(res) = parse_ext_resource_resource(
                        &attributes,
                        (index + 1) as u32,
                        &mut diagnostics,
                    ) {
                        resource_count += 1;
                        if ext_ids.contains(&res.id) {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::ResourceDuplicateResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "Duplicate ext_resource id \"{}\" at line {}; the later declaration is ignored.",
                                    res.id,
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                        } else {
                            ext_ids.insert(res.id.clone());
                            external_resources.push(res);
                        }
                    }
                }
                "sub_resource" => {
                    current_section = Section::SubResource;
                    if resource_count >= GODOT_SCENE_LIMITS.max_resources {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "resources",
                            GodotDiagnosticCode::ResourceDocumentTruncated,
                            format!(
                                "The resource count exceeded the bound ({}); remaining resources are ignored.",
                                GODOT_SCENE_LIMITS.max_resources
                            ),
                            Some((index + 1) as u32),
                        );
                        current_sub = None;
                    } else {
                        resource_count += 1;
                        let type_name =
                            read_string_attribute(&attributes, "type")
                                .unwrap_or_default();
                        let Some(id) =
                            read_string_attribute(&attributes, "id")
                        else {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::ResourceMissingResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "sub_resource at line {} is missing its id attribute.",
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                            current_sub = None;
                            index += 1;
                            continue;
                        };
                        if sub_ids.contains(&id) {
                            push_diagnostic(
                                &mut diagnostics,
                                GodotDiagnosticCode::ResourceDuplicateResourceId,
                                GodotDiagnosticSeverity::Error,
                                format!(
                                    "Duplicate sub_resource id \"{id}\" at line {}; the later declaration is ignored.",
                                    index + 1
                                ),
                                Some((index + 1) as u32),
                            );
                            current_sub = None;
                        } else {
                            sub_ids.insert(id.clone());
                            sub_resources.push(MutableSubResource {
                                id,
                                type_name,
                                line: (index + 1) as u32,
                                properties: Vec::new(),
                            });
                            current_sub = Some(sub_resources.len() - 1);
                        }
                    }
                }
                "resource" => {
                    current_section = Section::Resource;
                    current_sub = None;
                }
                _ => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::ResourceMalformedSection,
                        GodotDiagnosticSeverity::Error,
                        format!(
                            "Unknown section header \"[{section_name}]\" at line {}.",
                            index + 1
                        ),
                        Some((index + 1) as u32),
                    );
                    current_section = Section::Body;
                    current_sub = None;
                }
            }
            index += 1;
            continue;
        }

        let record = read_record(
            &raw_lines,
            index,
            line_count,
            &mut diagnostics,
            GodotTextDocumentKind::Resource,
        );
        let end = record.end_index;
        if let Some(key) = record.key {
            match current_section {
                Section::Resource => {
                    if property_count >= GODOT_SCENE_LIMITS.max_properties {
                        truncated = true;
                        report_limit_once(
                            &mut reported_limits,
                            &mut diagnostics,
                            "properties",
                            GodotDiagnosticCode::ResourceDocumentTruncated,
                            format!(
                                "The property count exceeded the bound ({}); remaining properties are ignored.",
                                GODOT_SCENE_LIMITS.max_properties
                            ),
                            Some(record.line),
                        );
                    } else {
                        property_count += 1;
                        properties_raw.push(RawProperty {
                            name: unquote_key(&key),
                            value_text: record.value_text,
                            line: record.line,
                        });
                    }
                }
                Section::SubResource => {
                    if let Some(sub_idx) = current_sub {
                        if property_count >= GODOT_SCENE_LIMITS.max_properties
                        {
                            truncated = true;
                            report_limit_once(
                                &mut reported_limits,
                                &mut diagnostics,
                                "properties",
                                GodotDiagnosticCode::ResourceDocumentTruncated,
                                format!(
                                    "The property count exceeded the bound ({}); remaining properties are ignored.",
                                    GODOT_SCENE_LIMITS.max_properties
                                ),
                                Some(record.line),
                            );
                        } else {
                            property_count += 1;
                            let prop = make_property_resource(
                                &key,
                                &record.value_text,
                                record.line,
                                &mut diagnostics,
                            );
                            if let Some(sub) = sub_resources.get_mut(sub_idx) {
                                sub.properties.push(prop);
                            }
                        }
                    } else {
                        push_diagnostic(
                            &mut diagnostics,
                            GodotDiagnosticCode::ResourceUnknownProperty,
                            GodotDiagnosticSeverity::Warning,
                            format!(
                                "Property \"{key}\" at line {} is not valid in the current section and was ignored.",
                                record.line
                            ),
                            Some(record.line),
                        );
                    }
                }
                _ => {
                    push_diagnostic(
                        &mut diagnostics,
                        GodotDiagnosticCode::ResourceUnknownProperty,
                        GodotDiagnosticSeverity::Warning,
                        format!(
                            "Property \"{key}\" at line {} is not valid in the current section and was ignored.",
                            record.line
                        ),
                        Some(record.line),
                    );
                }
            }
        }
        index = end + 1;
    }

    let sub_resources_model: Vec<super::models::SubResourceRef> =
        sub_resources
            .into_iter()
            .map(|s| super::models::SubResourceRef {
                id: s.id,
                type_name: s.type_name,
                properties: s.properties,
                line: Some(s.line),
            })
            .collect();

    let document: Option<GodotResourceModel> = if !header_present
        || header_type.is_none()
    {
        None
    } else {
        let type_name = header_type.unwrap_or_default();
        let mut props: Vec<GodotProperty> = Vec::new();
        for raw in &properties_raw {
            let parsed = parse_godot_variant(&raw.value_text);
            if parsed.truncated {
                push_diagnostic(
                    &mut diagnostics,
                    GodotDiagnosticCode::ResourceValueTruncated,
                    GodotDiagnosticSeverity::Warning,
                    format!(
                        "The value of \"{}\" at line {} exceeds interpretation bounds and was preserved partially.",
                        raw.name, raw.line
                    ),
                    Some(raw.line),
                );
            }
            props.push(GodotProperty {
                name: raw.name.clone(),
                value: parsed.value,
                raw_value: bounded_raw_value(&raw.value_text),
                line: Some(raw.line),
            });
        }
        let script = {
            let found = props.iter().find(|p| p.name == "script");
            resolve_script_reference_resource(
                found,
                &external_resources,
                &sub_resources_model,
                &mut diagnostics,
            )
        };
        Some(GodotResourceModel {
            path: path.to_owned(),
            revision: revision.clone(),
            type_name,
            uid: header_uid,
            format: header_format,
            load_steps: header_load_steps,
            script,
            external_resources,
            sub_resources: sub_resources_model,
            properties: props,
        })
    };

    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity == GodotDiagnosticSeverity::Error)
        .count();
    let status = if !seen_resource_header || document.is_none() {
        GodotParseStatus::Invalid
    } else if error_count == 0 {
        GodotParseStatus::Complete
    } else {
        GodotParseStatus::Partial
    };

    GodotTextDocument {
        path: path.to_owned(),
        revision,
        kind: GodotTextDocumentKind::Resource,
        status,
        document,
        diagnostics,
        truncated,
    }
}
