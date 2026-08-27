//! Normalized, bounded representation of an engine-generated
//! `extension_api.json` (from `--dump-extension-api-with-docs`).
//!
//! Godot's dump format is not a formal versioned protocol, so parsing is
//! conservative and unknown fields are tolerated without failing the
//! build. The raw dump is never persisted into the workspace and never
//! becomes an application or provider event; its SHA-256 is computed over
//! the exact raw bytes.

use siralos_core::identity::sha256_hex;
use siralos_core::language::truncate_utf8_bytes;
use siralos_godot::godot::GODOT_LIMITS;

/// One dump parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpParameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    pub param_type: String,
    /// Default-argument expression, if representable.
    pub default_value: Option<String>,
}

/// One dump method or utility function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpMethod {
    /// Method name.
    pub name: String,
    /// Return type, if declared.
    pub return_type: Option<String>,
    /// Declared parameters.
    pub parameters: Vec<GodotApiDumpParameter>,
    /// Qualifiers in engine order.
    pub qualifiers: Vec<String>,
    /// Engine method hash retained as text, if present.
    pub hash: Option<String>,
    /// Bounded description, if any.
    pub description: Option<String>,
}

/// One dump property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpProperty {
    /// Property name.
    pub name: String,
    /// Property type, if declared.
    pub prop_type: Option<String>,
    /// Setter method, if any.
    pub setter: Option<String>,
    /// Getter method, if any.
    pub getter: Option<String>,
    /// Bounded description, if any.
    pub description: Option<String>,
}

/// One dump signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpSignal {
    /// Signal name.
    pub name: String,
    /// Declared parameters.
    pub parameters: Vec<GodotApiDumpParameter>,
    /// Bounded description, if any.
    pub description: Option<String>,
}

/// One dump constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpConstant {
    /// Constant name.
    pub name: String,
    /// String representation of the value when representable.
    pub value: Option<String>,
    /// Bounded description, if any.
    pub description: Option<String>,
}

/// One dump enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpEnum {
    /// Enum name.
    pub name: String,
    /// Declared values.
    pub values: Vec<GodotApiDumpEnumValue>,
    /// Bounded description, if any.
    pub description: Option<String>,
}

/// One dump enum value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpEnumValue {
    /// Value name.
    pub name: String,
    /// String representation of the value.
    pub value: String,
}

/// One dump native class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpClass {
    /// Class name.
    pub name: String,
    /// Base class, if declared.
    pub base_class: Option<String>,
    /// Engine API type label, if declared.
    pub api_type_label: Option<String>,
    /// Bounded brief description, if any.
    pub brief_description: Option<String>,
    /// Bounded description, if any.
    pub description: Option<String>,
    /// Declared methods.
    pub methods: Vec<GodotApiDumpMethod>,
    /// Declared properties.
    pub properties: Vec<GodotApiDumpProperty>,
    /// Declared signals.
    pub signals: Vec<GodotApiDumpSignal>,
    /// Declared constants.
    pub constants: Vec<GodotApiDumpConstant>,
    /// Declared enums.
    pub enums: Vec<GodotApiDumpEnum>,
}

/// One dump built-in class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpBuiltinClass {
    /// Class name.
    pub name: String,
    /// Bounded description, if any.
    pub description: Option<String>,
    /// Declared methods.
    pub methods: Vec<GodotApiDumpMethod>,
    /// Declared operators.
    pub operators: Vec<String>,
    /// Declared constants.
    pub constants: Vec<GodotApiDumpConstant>,
    /// Declared enums.
    pub enums: Vec<GodotApiDumpEnum>,
}

/// Normalized bounded dump document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpDocument {
    /// Header version full name, if present.
    pub version_full_name: Option<String>,
    /// Header hash, if present.
    pub hash: Option<String>,
    /// Native classes.
    pub classes: Vec<GodotApiDumpClass>,
    /// Built-in classes.
    pub builtin_classes: Vec<GodotApiDumpBuiltinClass>,
    /// Global constants.
    pub global_constants: Vec<GodotApiDumpConstant>,
    /// Global enums.
    pub global_enums: Vec<GodotApiDumpEnum>,
    /// Utility functions.
    pub utility_functions: Vec<GodotApiDumpMethod>,
    /// Raw byte length of the dump.
    pub raw_bytes: u64,
    /// SHA-256 over the exact raw bytes.
    pub sha256: String,
}

/// Failure of a dump parse with its bounded message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiDumpParseFailure {
    /// Bounded failure message.
    pub message: String,
}

/// Parse the exact bytes of a with-docs `extension_api.json`.
pub fn parse_godot_api_dump_with_docs(
    content: &[u8],
) -> Result<GodotApiDumpDocument, GodotApiDumpParseFailure> {
    let max_bytes = GODOT_LIMITS.max_api_dump_with_docs_bytes;
    if content.len() > max_bytes {
        return Err(failure(&format!(
            "The API documentation dump is {} bytes, exceeding the {max_bytes}-byte bound.",
            content.len()
        )));
    }
    let parsed: serde_json::Value = match serde_json::from_slice(content) {
        Ok(value) => value,
        Err(_) => {
            return Err(failure(
                "The API documentation dump is not valid JSON.",
            ));
        }
    };
    let Some(root) = parsed.as_object() else {
        return Err(failure(
            "The API documentation dump is not a JSON object.",
        ));
    };
    let header = as_record(root.get("header"));
    let version_full_name = header
        .and_then(|header| header.get("version_full_name"))
        .and_then(|value| as_string(Some(value)))
        .map(str::to_owned);
    let hash = header
        .and_then(|header| header.get("hash"))
        .and_then(|value| as_string(Some(value)))
        .map(str::to_owned);
    let empty = Vec::new();
    let classes = as_array(root.get("classes"))
        .unwrap_or(&empty)
        .iter()
        .map(|entry| parse_class(Some(entry)))
        .collect();
    let builtin_classes = as_array(root.get("builtin_classes"))
        .unwrap_or(&empty)
        .iter()
        .map(|entry| parse_builtin_class(Some(entry)))
        .collect();
    let global_constants = as_array(root.get("global_constants"))
        .unwrap_or(&empty)
        .iter()
        .map(parse_constant)
        .collect();
    let global_enums = as_array(root.get("global_enums"))
        .unwrap_or(&empty)
        .iter()
        .map(parse_enum)
        .collect();
    let utility_functions = as_array(root.get("utility_functions"))
        .unwrap_or(&empty)
        .iter()
        .map(parse_method)
        .collect();
    Ok(GodotApiDumpDocument {
        version_full_name,
        hash,
        classes,
        builtin_classes,
        global_constants,
        global_enums,
        utility_functions,
        raw_bytes: content.len() as u64,
        sha256: sha256_hex(content),
    })
}

fn failure(message: &str) -> GodotApiDumpParseFailure {
    GodotApiDumpParseFailure { message: message.to_owned() }
}

fn parse_class(entry: Option<&serde_json::Value>) -> GodotApiDumpClass {
    let record = record_or_empty(entry);
    GodotApiDumpClass {
        name: bounded_string(record.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        base_class: nullable_string(record.get("base_class")),
        api_type_label: nullable_string(record.get("api_type")),
        brief_description: bounded_description(
            record.get("brief_description"),
        ),
        description: bounded_description(record.get("description")),
        methods: array_or_empty(record.get("methods"))
            .iter()
            .map(parse_method)
            .collect(),
        properties: array_or_empty(record.get("properties"))
            .iter()
            .map(parse_property)
            .collect(),
        signals: array_or_empty(record.get("signals"))
            .iter()
            .map(parse_signal)
            .collect(),
        constants: array_or_empty(record.get("constants"))
            .iter()
            .map(parse_constant)
            .collect(),
        enums: array_or_empty(record.get("enums"))
            .iter()
            .map(parse_enum)
            .collect(),
    }
}

fn parse_builtin_class(
    entry: Option<&serde_json::Value>,
) -> GodotApiDumpBuiltinClass {
    let record = record_or_empty(entry);
    GodotApiDumpBuiltinClass {
        name: bounded_string(record.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        description: bounded_description(record.get("description")),
        methods: array_or_empty(record.get("methods"))
            .iter()
            .map(parse_method)
            .collect(),
        operators: array_or_empty(record.get("operators"))
            .iter()
            .filter_map(|operator| as_record(Some(operator)))
            .map(|operator| {
                operator
                    .get("name")
                    .and_then(|value| as_string(Some(value)))
                    .map(|name| {
                        truncate_utf8_bytes(name, max_description_bytes())
                    })
                    .unwrap_or_else(|| "?".to_owned())
            })
            .collect(),
        constants: array_or_empty(record.get("constants"))
            .iter()
            .map(parse_constant)
            .collect(),
        enums: array_or_empty(record.get("enums"))
            .iter()
            .map(parse_enum)
            .collect(),
    }
}

fn parse_method(record: &serde_json::Value) -> GodotApiDumpMethod {
    let source = record_or_empty(Some(record));
    let mut qualifiers = Vec::new();
    if source.get("is_static") == Some(&serde_json::Value::Bool(true)) {
        qualifiers.push("static".to_owned());
    }
    if source.get("is_vararg") == Some(&serde_json::Value::Bool(true)) {
        qualifiers.push("vararg".to_owned());
    }
    if source.get("is_const") == Some(&serde_json::Value::Bool(true)) {
        qualifiers.push("const".to_owned());
    }
    if source.get("is_virtual") == Some(&serde_json::Value::Bool(true)) {
        qualifiers.push("virtual".to_owned());
    }
    GodotApiDumpMethod {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        return_type: nullable_string(source.get("return_type")),
        parameters: array_or_empty(source.get("arguments"))
            .iter()
            .map(parse_parameter)
            .collect(),
        qualifiers,
        hash: nullable_hash(source.get("hash")),
        description: bounded_description(source.get("description")),
    }
}

fn parse_parameter(record: &serde_json::Value) -> GodotApiDumpParameter {
    let source = record_or_empty(Some(record));
    GodotApiDumpParameter {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "arg".to_owned()),
        param_type: bounded_string(source.get("type"))
            .unwrap_or_else(|| "Variant".to_owned()),
        default_value: representable_value(source.get("default_value")),
    }
}

fn parse_property(record: &serde_json::Value) -> GodotApiDumpProperty {
    let source = record_or_empty(Some(record));
    GodotApiDumpProperty {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        prop_type: nullable_string(source.get("type")),
        setter: nullable_string(source.get("setter")),
        getter: nullable_string(source.get("getter")),
        description: bounded_description(source.get("description")),
    }
}

fn parse_signal(record: &serde_json::Value) -> GodotApiDumpSignal {
    let source = record_or_empty(Some(record));
    GodotApiDumpSignal {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        parameters: array_or_empty(source.get("arguments"))
            .iter()
            .map(parse_parameter)
            .collect(),
        description: bounded_description(source.get("description")),
    }
}

fn parse_constant(record: &serde_json::Value) -> GodotApiDumpConstant {
    let source = record_or_empty(Some(record));
    GodotApiDumpConstant {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        value: representable_value(source.get("value")),
        description: bounded_description(source.get("description")),
    }
}

fn parse_enum(record: &serde_json::Value) -> GodotApiDumpEnum {
    let source = record_or_empty(Some(record));
    let values = array_or_empty(source.get("values"))
        .iter()
        .filter_map(|entry| as_record(Some(entry)))
        .map(|entry| GodotApiDumpEnumValue {
            name: bounded_string(entry.get("name"))
                .unwrap_or_else(|| "unnamed".to_owned()),
            value: representable_value(entry.get("value"))
                .unwrap_or_else(|| "?".to_owned()),
        })
        .collect();
    GodotApiDumpEnum {
        name: bounded_string(source.get("name"))
            .unwrap_or_else(|| "unnamed".to_owned()),
        values,
        description: bounded_description(source.get("description")),
    }
}

fn max_description_bytes() -> usize {
    GODOT_LIMITS.max_api_description_bytes
}

fn representable_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(truncate_utf8_bytes(
            &format!("\"{text}\""),
            max_description_bytes(),
        )),
        serde_json::Value::Number(number) => Some(truncate_utf8_bytes(
            &number.to_string(),
            max_description_bytes(),
        )),
        serde_json::Value::Bool(flag) => Some(truncate_utf8_bytes(
            if *flag { "true" } else { "false" },
            max_description_bytes(),
        )),
        _ => None,
    }
}

fn bounded_description(value: Option<&serde_json::Value>) -> Option<String> {
    bounded_string(value)
}

fn bounded_string(value: Option<&serde_json::Value>) -> Option<String> {
    let text = as_string(value)?;
    Some(truncate_utf8_bytes(text, max_description_bytes()))
}

fn nullable_string(value: Option<&serde_json::Value>) -> Option<String> {
    let text = as_string(value)?;
    Some(truncate_utf8_bytes(text, max_description_bytes()))
}

/// Method hashes are numbers in the dump; they are retained as text.
fn nullable_hash(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::Number(number) => Some(number.to_string()),
        other => nullable_string(Some(other)),
    }
}

fn as_record(
    value: Option<&serde_json::Value>,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    value?.as_object()
}

fn as_array(
    value: Option<&serde_json::Value>,
) -> Option<&Vec<serde_json::Value>> {
    value?.as_array()
}

fn as_string(value: Option<&serde_json::Value>) -> Option<&str> {
    value?.as_str()
}

fn record_or_empty(
    entry: Option<&serde_json::Value>,
) -> &serde_json::Map<String, serde_json::Value> {
    static EMPTY: std::sync::OnceLock<
        serde_json::Map<String, serde_json::Value>,
    > = std::sync::OnceLock::new();
    as_record(entry).unwrap_or_else(|| EMPTY.get_or_init(serde_json::Map::new))
}

fn array_or_empty(
    value: Option<&serde_json::Value>,
) -> &Vec<serde_json::Value> {
    static EMPTY: std::sync::OnceLock<Vec<serde_json::Value>> =
        std::sync::OnceLock::new();
    as_array(value).unwrap_or_else(|| EMPTY.get_or_init(Vec::new))
}
