//! Structured scene/resource mutation operations (Stage 3 milestone 10,
//! ADR 0026; R9 parity slice).
//!
//! Mirrors `packages/core/src/godot/scene-mutation/operations.ts`.
//! Native mutation is STRUCTURED, never arbitrary text replacement:
//! every operation is a typed intent over the parsed semantic model,
//! validated before it becomes a prepared mutation
//! ([`crate::godot::scene_mutation::prepared`). The provider-facing
//! surface is prepare-only; approval, checkpointing, and apply are
//! application-owned and stay typed `unavailable` while the
//! directory-relative write primitive is unsound (`SECURITY.md`).
//!
//! Values are [`GodotVariantValue`]s; opaque/unknown constructors are
//! rejected so every prepared artifact stays semantically verifiable.

use crate::godot::scene::models::GodotVariantValue;
use serde_json::json;

/// Host-owned hard bounds for mutation operations (never raised by
/// input). Mirrors `MUTATION_LIMITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationLimits;

impl MutationLimits {
    /// Maximum operations per mutation.
    pub const MAX_OPERATIONS: usize = 32;
    /// Maximum properties per operation.
    pub const MAX_PROPERTIES_PER_OPERATION: usize = 32;
    /// Maximum path bytes.
    pub const MAX_PATH_BYTES: usize = 1024;
    /// Maximum name bytes.
    pub const MAX_NAME_BYTES: usize = 256;
    /// Maximum serialized value bytes.
    pub const MAX_VALUE_BYTES: usize = 4096;
}

/// Validation failure at the mutation-operation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationError {
    /// Bounded truthful message (mirrors the oracle strings).
    pub message: String,
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MutationError {}

fn error(message: impl Into<String>) -> MutationError {
    MutationError { message: message.into() }
}

fn require_bounded(
    text: &str,
    max_bytes: usize,
    field: &str,
) -> Result<String, MutationError> {
    let value = text.trim();
    if value.is_empty() {
        return Err(error(format!("{field} must not be empty.")));
    }
    if value.len() > max_bytes {
        return Err(error(format!(
            "{field} exceeds {max_bytes} UTF-8 bytes."
        )));
    }
    Ok(value.to_owned())
}

/// One named property inside `add_node` / subresource operations.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationProperty {
    /// Property name.
    pub name: String,
    /// Structured value.
    pub value: GodotVariantValue,
}

/// One typed scene/resource mutation operation. Scene-scoped operations
/// carry `node_path`; resource-scoped operations leave it `None`
/// (mirroring the shared TypeScript variants).
#[derive(Debug, Clone, PartialEq)]
pub enum MutationOperation {
    /// Assign one property on a node or the resource root.
    SetProperty {
        /// Absolute node path, or root property when `None`.
        node_path: Option<String>,
        /// Property name.
        property: String,
        /// Structured value.
        value: GodotVariantValue,
    },
    /// Remove one property.
    RemoveProperty {
        /// Absolute node path, or root property when `None`.
        node_path: Option<String>,
        /// Property name.
        property: String,
    },
    /// Add one node.
    AddNode {
        /// Node name (no slashes).
        name: String,
        /// Node type.
        node_type: String,
        /// Parent path; root when `None`.
        parent_path: Option<String>,
        /// Initial properties.
        properties: Vec<MutationProperty>,
        /// Group memberships.
        groups: Vec<String>,
    },
    /// Remove one node.
    RemoveNode {
        /// Absolute node path.
        node_path: String,
    },
    /// Attach or detach a script on a node.
    SetScriptAttachment {
        /// Absolute node path.
        node_path: String,
        /// Document-local `ext_resource` id, or `None` to detach.
        ext_resource_id: Option<String>,
    },
    /// Retarget one resource reference.
    ChangeResourceReference {
        /// Document-local resource id.
        resource_id: String,
        /// New `res://` path.
        new_path: Option<String>,
        /// New `uid://`.
        new_uid: Option<String>,
    },
    /// Add one serialized signal connection.
    AddSignalConnection {
        /// Signal name.
        signal: String,
        /// Emitter node path.
        from: String,
        /// Receiver node path.
        to: String,
        /// Receiver method name.
        method: String,
        /// Connection flags.
        flags: Option<u32>,
        /// Bound values.
        binds: Vec<GodotVariantValue>,
    },
    /// Remove one serialized signal connection.
    RemoveSignalConnection {
        /// Signal name.
        signal: String,
        /// Emitter node path.
        from: String,
        /// Receiver node path.
        to: String,
        /// Receiver method name.
        method: String,
    },
    /// Create one sub-resource.
    CreateSubresource {
        /// Document-local id.
        id: String,
        /// Resource type.
        resource_type: String,
        /// Initial properties.
        properties: Vec<MutationProperty>,
    },
    /// Update one sub-resource's properties.
    UpdateSubresource {
        /// Document-local id.
        id: String,
        /// Replacement properties.
        properties: Vec<MutationProperty>,
    },
    /// Remove one sub-resource.
    RemoveSubresource {
        /// Document-local id.
        id: String,
    },
}

impl MutationOperation {
    /// The canonical `op` discriminant string.
    #[must_use]
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::SetProperty { .. } => "set_property",
            Self::RemoveProperty { .. } => "remove_property",
            Self::AddNode { .. } => "add_node",
            Self::RemoveNode { .. } => "remove_node",
            Self::SetScriptAttachment { .. } => "set_script_attachment",
            Self::ChangeResourceReference { .. } => {
                "change_resource_reference"
            }
            Self::AddSignalConnection { .. } => "add_signal_connection",
            Self::RemoveSignalConnection { .. } => "remove_signal_connection",
            Self::CreateSubresource { .. } => "create_subresource",
            Self::UpdateSubresource { .. } => "update_subresource",
            Self::RemoveSubresource { .. } => "remove_subresource",
        }
    }

    /// Canonical JSON mirroring the TypeScript oracle's object shape
    /// (camelCase fields; absent options omitted).
    #[must_use]
    pub fn to_canonical_json(&self) -> serde_json::Value {
        fn properties_json(
            properties: &[MutationProperty],
        ) -> serde_json::Value {
            serde_json::Value::Array(
                properties
                    .iter()
                    .map(|property| {
                        json!({
                            "name": property.name,
                            "value": variant_to_json(&property.value),
                        })
                    })
                    .collect(),
            )
        }
        match self {
            Self::SetProperty { node_path, property, value } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("set_property"));
                if let Some(node_path) = node_path {
                    object.insert("nodePath".to_owned(), json!(node_path));
                }
                object.insert("property".to_owned(), json!(property));
                object.insert("value".to_owned(), variant_to_json(value));
                serde_json::Value::Object(object)
            }
            Self::RemoveProperty { node_path, property } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("remove_property"));
                if let Some(node_path) = node_path {
                    object.insert("nodePath".to_owned(), json!(node_path));
                }
                object.insert("property".to_owned(), json!(property));
                serde_json::Value::Object(object)
            }
            Self::AddNode {
                name,
                node_type,
                parent_path,
                properties,
                groups,
            } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("add_node"));
                object.insert("name".to_owned(), json!(name));
                object.insert("type".to_owned(), json!(node_type));
                if let Some(parent_path) = parent_path {
                    object.insert("parentPath".to_owned(), json!(parent_path));
                }
                if !properties.is_empty() {
                    object.insert(
                        "properties".to_owned(),
                        properties_json(properties),
                    );
                }
                if !groups.is_empty() {
                    object.insert("groups".to_owned(), json!(groups));
                }
                serde_json::Value::Object(object)
            }
            Self::RemoveNode { node_path } => {
                json!({"op": "remove_node", "nodePath": node_path})
            }
            Self::SetScriptAttachment { node_path, ext_resource_id } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("set_script_attachment"));
                object.insert("nodePath".to_owned(), json!(node_path));
                object.insert(
                    "extResourceId".to_owned(),
                    match ext_resource_id {
                        Some(id) => json!(id),
                        None => serde_json::Value::Null,
                    },
                );
                serde_json::Value::Object(object)
            }
            Self::ChangeResourceReference {
                resource_id,
                new_path,
                new_uid,
            } => {
                let mut object = serde_json::Map::new();
                object.insert(
                    "op".to_owned(),
                    json!("change_resource_reference"),
                );
                object.insert("resourceId".to_owned(), json!(resource_id));
                if let Some(new_path) = new_path {
                    object.insert("newPath".to_owned(), json!(new_path));
                }
                if let Some(new_uid) = new_uid {
                    object.insert("newUid".to_owned(), json!(new_uid));
                }
                serde_json::Value::Object(object)
            }
            Self::AddSignalConnection {
                signal,
                from,
                to,
                method,
                flags,
                binds,
            } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("add_signal_connection"));
                object.insert("signal".to_owned(), json!(signal));
                object.insert("from".to_owned(), json!(from));
                object.insert("to".to_owned(), json!(to));
                object.insert("method".to_owned(), json!(method));
                if let Some(flags) = flags {
                    object.insert("flags".to_owned(), json!(flags));
                }
                if !binds.is_empty() {
                    object.insert(
                        "binds".to_owned(),
                        serde_json::Value::Array(
                            binds.iter().map(variant_to_json).collect(),
                        ),
                    );
                }
                serde_json::Value::Object(object)
            }
            Self::RemoveSignalConnection { signal, from, to, method } => {
                json!({
                    "op": "remove_signal_connection",
                    "signal": signal,
                    "from": from,
                    "to": to,
                    "method": method,
                })
            }
            Self::CreateSubresource { id, resource_type, properties } => {
                let mut object = serde_json::Map::new();
                object.insert("op".to_owned(), json!("create_subresource"));
                object.insert("id".to_owned(), json!(id));
                object.insert("type".to_owned(), json!(resource_type));
                if !properties.is_empty() {
                    object.insert(
                        "properties".to_owned(),
                        properties_json(properties),
                    );
                }
                serde_json::Value::Object(object)
            }
            Self::UpdateSubresource { id, properties } => json!({
                "op": "update_subresource",
                "id": id,
                "properties": properties_json(properties),
            }),
            Self::RemoveSubresource { id } => {
                json!({"op": "remove_subresource", "id": id})
            }
        }
    }
}

/// Convert one parsed Variant value to its oracle JSON shape
/// (`packages/core/src/godot/scene/models.ts` discriminated union).
#[must_use]
pub fn variant_to_json(value: &GodotVariantValue) -> serde_json::Value {
    match value {
        GodotVariantValue::Null => json!({"kind": "null"}),
        GodotVariantValue::Boolean(inner) => {
            json!({"kind": "boolean", "value": inner})
        }
        GodotVariantValue::Integer(inner) => {
            json!({"kind": "integer", "value": inner})
        }
        GodotVariantValue::Float(inner) => {
            let number = serde_json::Number::from_f64(*inner)
                .unwrap_or(serde_json::Number::from(0));
            json!({"kind": "float", "value": number})
        }
        GodotVariantValue::String(inner) => {
            json!({"kind": "string", "value": inner})
        }
        GodotVariantValue::StringName(inner) => {
            json!({"kind": "string_name", "value": inner})
        }
        GodotVariantValue::NodePath(inner) => {
            json!({"kind": "node_path", "value": inner})
        }
        GodotVariantValue::Array(items) => json!({
            "kind": "array",
            "items": items.iter().map(variant_to_json).collect::<Vec<_>>(),
        }),
        GodotVariantValue::Dictionary(entries) => json!({
            "kind": "dictionary",
            "entries": entries
                .iter()
                .map(|entry| {
                    json!({
                        "key": variant_to_json(&entry.key),
                        "value": variant_to_json(&entry.value),
                    })
                })
                .collect::<Vec<_>>(),
        }),
        GodotVariantValue::Vector { type_name, components } => json!({
            "kind": "vector",
            "typeName": type_name,
            "components": components,
        }),
        GodotVariantValue::Color(components) => {
            json!({"kind": "color", "components": components})
        }
        GodotVariantValue::PackedArray { type_name, items } => json!({
            "kind": "packed_array",
            "typeName": type_name,
            "items": items.iter().map(variant_to_json).collect::<Vec<_>>(),
        }),
        GodotVariantValue::ExtResource(id) => {
            json!({"kind": "ext_resource", "id": id})
        }
        GodotVariantValue::SubResource(id) => {
            json!({"kind": "sub_resource", "id": id})
        }
        GodotVariantValue::Resource { uid, path, type_name } => {
            let mut object = serde_json::Map::new();
            object.insert("kind".to_owned(), json!("resource"));
            if let Some(uid) = uid {
                object.insert("uid".to_owned(), json!(uid));
            }
            if let Some(path) = path {
                object.insert("path".to_owned(), json!(path));
            }
            if let Some(type_name) = type_name {
                object.insert("type".to_owned(), json!(type_name));
            }
            serde_json::Value::Object(object)
        }
        GodotVariantValue::Opaque { type_name, raw } => json!({
            "kind": "opaque",
            "typeName": type_name,
            "raw": {"text": raw.text, "truncated": raw.truncated},
        }),
    }
}

/// Validate an absolute scene node path (root-relative, no traversal,
/// no backslashes, no NUL).
pub fn validate_node_path(path: &str) -> Result<String, MutationError> {
    let value =
        require_bounded(path, MutationLimits::MAX_PATH_BYTES, "A node path")?;
    if value.contains('\\')
        || value.contains('\0')
        || value.split('/').any(|segment| segment == "..")
    {
        return Err(error(format!("Invalid node path: {value}")));
    }
    Ok(value)
}

fn validate_name(name: &str, field: &str) -> Result<String, MutationError> {
    let value = require_bounded(name, MutationLimits::MAX_NAME_BYTES, field)?;
    if value.contains('/') || value.contains('\0') {
        return Err(error(format!(
            "{field} must not contain slashes or NUL: {value}"
        )));
    }
    Ok(value)
}

fn validate_value(
    value: &GodotVariantValue,
    field: &str,
) -> Result<GodotVariantValue, MutationError> {
    let serialized =
        crate::identity::canonicalize_json(&variant_to_json(value));
    if serialized.len() > MutationLimits::MAX_VALUE_BYTES {
        return Err(error(format!(
            "{field} exceeds {} UTF-8 bytes.",
            MutationLimits::MAX_VALUE_BYTES
        )));
    }
    if matches!(value, GodotVariantValue::Opaque { .. }) {
        return Err(error(format!(
            "{field} must not carry opaque/unknown constructors; use structured values."
        )));
    }
    Ok(value.clone())
}

fn validate_properties(
    properties: &[MutationProperty],
    field: &str,
) -> Result<Vec<MutationProperty>, MutationError> {
    if properties.len() > MutationLimits::MAX_PROPERTIES_PER_OPERATION {
        return Err(error(format!(
            "{field} accepts at most {} properties.",
            MutationLimits::MAX_PROPERTIES_PER_OPERATION
        )));
    }
    let mut seen = std::collections::HashSet::new();
    let mut validated = Vec::with_capacity(properties.len());
    for property in properties {
        let name =
            validate_name(&property.name, &format!("{field} property name"))?;
        if !seen.insert(name.clone()) {
            return Err(error(format!(
                "{field} contains a duplicate property: {name}"
            )));
        }
        validated.push(MutationProperty {
            name,
            value: validate_value(
                &property.value,
                &format!("{field} property {}", property.name.trim()),
            )?,
        });
    }
    Ok(validated)
}

fn validate_reference_change(
    resource_id: &str,
    new_path: Option<&str>,
    new_uid: Option<&str>,
) -> Result<(), MutationError> {
    validate_name(resource_id, "A resource id")?;
    if let Some(new_path) = new_path {
        require_bounded(
            new_path,
            MutationLimits::MAX_PATH_BYTES,
            "A resource path",
        )?;
    }
    if let Some(new_uid) = new_uid {
        require_bounded(
            new_uid,
            MutationLimits::MAX_PATH_BYTES,
            "A resource uid",
        )?;
    }
    if new_path.is_none() && new_uid.is_none() {
        return Err(error(
            "change_resource_reference requires newPath and/or newUid.",
        ));
    }
    Ok(())
}

/// Validate and detach one mutation operation. Paths are workspace-safe
/// and node paths are absolute; values are structured (opaque
/// constructors are rejected); counts are bounded.
pub fn validate_mutation_operation(
    operation: &MutationOperation,
) -> Result<MutationOperation, MutationError> {
    Ok(match operation {
        MutationOperation::SetProperty { node_path, property, value } => {
            MutationOperation::SetProperty {
                node_path: match node_path {
                    Some(node_path) => Some(validate_node_path(node_path)?),
                    None => None,
                },
                property: validate_name(property, "A property name")?,
                value: validate_value(value, "A property value")?,
            }
        }
        MutationOperation::RemoveProperty { node_path, property } => {
            MutationOperation::RemoveProperty {
                node_path: match node_path {
                    Some(node_path) => Some(validate_node_path(node_path)?),
                    None => None,
                },
                property: validate_name(property, "A property name")?,
            }
        }
        MutationOperation::AddNode {
            name,
            node_type,
            parent_path,
            properties,
            groups,
        } => MutationOperation::AddNode {
            name: validate_name(name, "A node name")?,
            node_type: validate_name(node_type, "A node type")?,
            parent_path: match parent_path {
                Some(parent_path) => Some(validate_node_path(parent_path)?),
                None => None,
            },
            properties: validate_properties(properties, "add_node")?,
            groups: {
                let bounded = groups
                    .iter()
                    .take(MutationLimits::MAX_PROPERTIES_PER_OPERATION);
                let mut validated = Vec::new();
                for group in bounded {
                    validated.push(validate_name(group, "A group name")?);
                }
                validated
            },
        },
        MutationOperation::RemoveNode { node_path } => {
            MutationOperation::RemoveNode {
                node_path: validate_node_path(node_path)?,
            }
        }
        MutationOperation::SetScriptAttachment {
            node_path,
            ext_resource_id,
        } => MutationOperation::SetScriptAttachment {
            node_path: validate_node_path(node_path)?,
            ext_resource_id: match ext_resource_id {
                Some(id) => Some(validate_name(id, "A resource id")?),
                None => None,
            },
        },
        MutationOperation::ChangeResourceReference {
            resource_id,
            new_path,
            new_uid,
        } => {
            validate_reference_change(
                resource_id,
                new_path.as_deref(),
                new_uid.as_deref(),
            )?;
            MutationOperation::ChangeResourceReference {
                resource_id: validate_name(resource_id, "A resource id")?,
                new_path: new_path
                    .as_deref()
                    .map(str::trim)
                    .map(str::to_owned),
                new_uid: new_uid.as_deref().map(str::trim).map(str::to_owned),
            }
        }
        MutationOperation::AddSignalConnection {
            signal,
            from,
            to,
            method,
            flags,
            binds,
        } => MutationOperation::AddSignalConnection {
            signal: validate_name(signal, "A signal name")?,
            from: validate_node_path(from)?,
            to: validate_node_path(to)?,
            method: validate_name(method, "A method name")?,
            flags: *flags,
            binds: {
                let bounded = binds
                    .iter()
                    .take(MutationLimits::MAX_PROPERTIES_PER_OPERATION)
                    .enumerate();
                let mut validated = Vec::new();
                for (index, value) in bounded {
                    validated.push(validate_value(
                        value,
                        &format!("A bind value {index}"),
                    )?);
                }
                validated
            },
        },
        MutationOperation::RemoveSignalConnection {
            signal,
            from,
            to,
            method,
        } => MutationOperation::RemoveSignalConnection {
            signal: validate_name(signal, "A signal name")?,
            from: validate_node_path(from)?,
            to: validate_node_path(to)?,
            method: validate_name(method, "A method name")?,
        },
        MutationOperation::CreateSubresource {
            id,
            resource_type,
            properties,
        } => MutationOperation::CreateSubresource {
            id: validate_name(id, "A subresource id")?,
            resource_type: validate_name(resource_type, "A subresource type")?,
            properties: validate_properties(properties, "create_subresource")?,
        },
        MutationOperation::UpdateSubresource { id, properties } => {
            MutationOperation::UpdateSubresource {
                id: validate_name(id, "A subresource id")?,
                properties: validate_properties(
                    properties,
                    "update_subresource",
                )?,
            }
        }
        MutationOperation::RemoveSubresource { id } => {
            MutationOperation::RemoveSubresource {
                id: validate_name(id, "A subresource id")?,
            }
        }
    })
}

/// Validate and detach one bounded operation set.
pub fn validate_mutation_operations(
    operations: &[MutationOperation],
) -> Result<Vec<MutationOperation>, MutationError> {
    if operations.len() > MutationLimits::MAX_OPERATIONS {
        return Err(error(format!(
            "A mutation accepts at most {} operations.",
            MutationLimits::MAX_OPERATIONS
        )));
    }
    if operations.is_empty() {
        return Err(error("A mutation requires at least one operation."));
    }
    operations.iter().map(validate_mutation_operation).collect()
}

/// Post-apply semantic expectation derived from the operation set.
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticExpectation {
    /// The node exists.
    NodeExists {
        /// Resolved absolute node path.
        node_path: String,
    },
    /// The node is absent.
    NodeAbsent {
        /// Absolute node path.
        node_path: String,
    },
    /// The property equals the value.
    PropertyEquals {
        /// Node path, or resource root when `None`.
        node_path: Option<String>,
        /// Property name.
        property: String,
        /// Expected value.
        value: GodotVariantValue,
    },
    /// The property is absent.
    PropertyAbsent {
        /// Node path, or resource root when `None`.
        node_path: Option<String>,
        /// Property name.
        property: String,
    },
    /// The connection exists.
    ConnectionExists {
        /// Signal name.
        signal: String,
        /// Emitter path.
        from: String,
        /// Receiver path.
        to: String,
        /// Method name.
        method: String,
    },
    /// The connection is absent.
    ConnectionAbsent {
        /// Signal name.
        signal: String,
        /// Emitter path.
        from: String,
        /// Receiver path.
        to: String,
        /// Method name.
        method: String,
    },
    /// The script attachment state holds.
    ScriptAttachment {
        /// Node path.
        node_path: String,
        /// Attached `ext_resource` id, or detached when `None`.
        ext_resource_id: Option<String>,
    },
    /// The sub-resource exists.
    SubresourceExists {
        /// Document-local id.
        id: String,
    },
    /// The sub-resource is absent.
    SubresourceAbsent {
        /// Document-local id.
        id: String,
    },
    /// The resource reference points at the new target.
    ResourceReference {
        /// Document-local resource id.
        resource_id: String,
        /// New path.
        new_path: Option<String>,
        /// New uid.
        new_uid: Option<String>,
    },
    /// The resource carries the type.
    ResourceType {
        /// Type name.
        type_name: String,
    },
}

/// Deterministic post-apply expectations derived from the operation set.
#[must_use]
pub fn expected_semantic_effect(
    operations: &[MutationOperation],
) -> Vec<SemanticExpectation> {
    let mut expectations: Vec<SemanticExpectation> = Vec::new();
    for operation in operations {
        match operation {
            MutationOperation::SetProperty { node_path, property, value } => {
                expectations.push(SemanticExpectation::PropertyEquals {
                    node_path: node_path.clone(),
                    property: property.clone(),
                    value: value.clone(),
                })
            }
            MutationOperation::RemoveProperty { node_path, property } => {
                expectations.push(SemanticExpectation::PropertyAbsent {
                    node_path: node_path.clone(),
                    property: property.clone(),
                });
            }
            MutationOperation::AddNode {
                name,
                parent_path,
                properties,
                ..
            } => {
                // The expectation binds the node's RESOLVED absolute
                // path (parentPath/name), not the bare name.
                let node_path = match parent_path.as_deref() {
                    None | Some(".") => name.clone(),
                    Some(parent) => format!("{parent}/{name}"),
                };
                expectations.push(SemanticExpectation::NodeExists {
                    node_path: node_path.clone(),
                });
                for property in properties {
                    expectations.push(SemanticExpectation::PropertyEquals {
                        node_path: Some(node_path.clone()),
                        property: property.name.clone(),
                        value: property.value.clone(),
                    });
                }
            }
            MutationOperation::RemoveNode { node_path } => {
                expectations.push(SemanticExpectation::NodeAbsent {
                    node_path: node_path.clone(),
                });
            }
            MutationOperation::SetScriptAttachment {
                node_path,
                ext_resource_id,
            } => expectations.push(SemanticExpectation::ScriptAttachment {
                node_path: node_path.clone(),
                ext_resource_id: ext_resource_id.clone(),
            }),
            MutationOperation::ChangeResourceReference {
                resource_id,
                new_path,
                new_uid,
            } => expectations.push(SemanticExpectation::ResourceReference {
                resource_id: resource_id.clone(),
                new_path: new_path.clone(),
                new_uid: new_uid.clone(),
            }),
            MutationOperation::AddSignalConnection {
                signal,
                from,
                to,
                method,
                ..
            } => expectations.push(SemanticExpectation::ConnectionExists {
                signal: signal.clone(),
                from: from.clone(),
                to: to.clone(),
                method: method.clone(),
            }),
            MutationOperation::RemoveSignalConnection {
                signal,
                from,
                to,
                method,
            } => expectations.push(SemanticExpectation::ConnectionAbsent {
                signal: signal.clone(),
                from: from.clone(),
                to: to.clone(),
                method: method.clone(),
            }),
            MutationOperation::CreateSubresource { id, .. } => {
                expectations.push(SemanticExpectation::SubresourceExists {
                    id: id.clone(),
                });
            }
            MutationOperation::UpdateSubresource { id, .. } => {
                expectations.push(SemanticExpectation::SubresourceExists {
                    id: id.clone(),
                });
            }
            MutationOperation::RemoveSubresource { id } => {
                expectations.push(SemanticExpectation::SubresourceAbsent {
                    id: id.clone(),
                });
            }
        }
    }
    expectations
}

pub use prepared::{
    CreatePreparedGodotMutationInput, GodotMutationPreview, MutationKind,
    PreparedGodotMutation, compute_mutation_fingerprint,
    create_prepared_godot_mutation,
};

pub mod prepared;

#[cfg(test)]
mod tests {
    use super::{
        MutationError, MutationOperation, MutationProperty,
        SemanticExpectation, expected_semantic_effect,
        validate_mutation_operation, validate_mutation_operations,
        validate_node_path,
    };
    use crate::godot::scene::models::GodotVariantValue;

    fn string_value(value: &str) -> GodotVariantValue {
        GodotVariantValue::String(value.to_owned())
    }

    fn set_property() -> MutationOperation {
        MutationOperation::SetProperty {
            node_path: Some("Root/Button".to_owned()),
            property: "text".to_owned(),
            value: string_value("Play"),
        }
    }

    #[test]
    fn node_paths_reject_traversal_and_backslashes_with_oracle_message() {
        assert_eq!(
            validate_node_path("Root/../Child"),
            Err(MutationError {
                message: "Invalid node path: Root/../Child".to_owned()
            })
        );
        assert_eq!(
            validate_node_path("Root\\Child"),
            Err(MutationError {
                message: "Invalid node path: Root\\Child".to_owned()
            })
        );
        assert_eq!(
            validate_node_path("  "),
            Err(MutationError {
                message: "A node path must not be empty.".to_owned()
            })
        );
        assert_eq!(
            validate_node_path("Root/Child").expect("valid"),
            "Root/Child"
        );
    }

    #[test]
    fn opaque_values_are_rejected_structured_values_accepted() {
        let opaque = MutationOperation::SetProperty {
            node_path: None,
            property: "shader".to_owned(),
            value: GodotVariantValue::Opaque {
                type_name: "Shader".to_owned(),
                raw: crate::godot::scene::models::GodotRawValue {
                    text: "Shader(...)".to_owned(),
                    truncated: false,
                },
            },
        };
        assert_eq!(
            validate_mutation_operation(&opaque),
            Err(MutationError {
                message: "A property value must not carry opaque/unknown constructors; use structured values.".to_owned()
            })
        );
        let validated = validate_mutation_operation(&set_property())
            .expect("structured operation validates");
        assert_eq!(validated.op_name(), "set_property");
    }

    #[test]
    fn duplicate_properties_are_rejected_per_operation() {
        let operation = MutationOperation::AddNode {
            name: "HUD".to_owned(),
            node_type: "CanvasLayer".to_owned(),
            parent_path: None,
            properties: vec![
                MutationProperty {
                    name: "visible".to_owned(),
                    value: GodotVariantValue::Boolean(true),
                },
                MutationProperty {
                    name: "visible".to_owned(),
                    value: GodotVariantValue::Boolean(false),
                },
            ],
            groups: Vec::new(),
        };
        assert_eq!(
            validate_mutation_operation(&operation),
            Err(MutationError {
                message: "add_node contains a duplicate property: visible"
                    .to_owned()
            })
        );
    }

    #[test]
    fn change_resource_reference_requires_a_target() {
        let operation = MutationOperation::ChangeResourceReference {
            resource_id: "1_abcde".to_owned(),
            new_path: None,
            new_uid: None,
        };
        assert_eq!(
            validate_mutation_operation(&operation),
            Err(MutationError {
                message:
                    "change_resource_reference requires newPath and/or newUid."
                        .to_owned()
            })
        );
    }

    #[test]
    fn operation_sets_are_bounded_and_non_empty() {
        assert_eq!(
            validate_mutation_operations(&[]),
            Err(MutationError {
                message: "A mutation requires at least one operation."
                    .to_owned()
            })
        );
        let many: Vec<MutationOperation> =
            std::iter::repeat_n(set_property(), 33).collect();
        assert_eq!(
            validate_mutation_operations(&many),
            Err(MutationError {
                message: "A mutation accepts at most 32 operations."
                    .to_owned()
            })
        );
    }

    #[test]
    fn expectations_derive_resolved_add_node_paths_and_root_properties() {
        let operations = vec![
            MutationOperation::SetProperty {
                node_path: None,
                property: "config_version".to_owned(),
                value: GodotVariantValue::Integer(5),
            },
            MutationOperation::AddNode {
                name: "HUD".to_owned(),
                node_type: "CanvasLayer".to_owned(),
                parent_path: Some(".".to_owned()),
                properties: vec![MutationProperty {
                    name: "visible".to_owned(),
                    value: GodotVariantValue::Boolean(true),
                }],
                groups: Vec::new(),
            },
            MutationOperation::RemoveNode { node_path: "Root/Old".to_owned() },
        ];
        let expectations = expected_semantic_effect(&operations);
        assert_eq!(
            expectations,
            vec![
                SemanticExpectation::PropertyEquals {
                    node_path: None,
                    property: "config_version".to_owned(),
                    value: GodotVariantValue::Integer(5),
                },
                SemanticExpectation::NodeExists {
                    node_path: "HUD".to_owned(),
                },
                SemanticExpectation::PropertyEquals {
                    node_path: Some("HUD".to_owned()),
                    property: "visible".to_owned(),
                    value: GodotVariantValue::Boolean(true),
                },
                SemanticExpectation::NodeAbsent {
                    node_path: "Root/Old".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn nested_add_nodes_resolve_parent_slash_name() {
        let operations = vec![MutationOperation::AddNode {
            name: "Label".to_owned(),
            node_type: "Label".to_owned(),
            parent_path: Some("Root/HUD".to_owned()),
            properties: Vec::new(),
            groups: Vec::new(),
        }];
        let expectations = expected_semantic_effect(&operations);
        assert_eq!(
            expectations[0],
            SemanticExpectation::NodeExists {
                node_path: "Root/HUD/Label".to_owned(),
            }
        );
    }
}
