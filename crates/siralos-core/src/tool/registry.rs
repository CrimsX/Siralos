//! Immutable generic Tool Registry and the one callable Tool seam.
//!
//! The registry is a heterogeneous runtime boundary by design: R7.2
//! composes multiple concrete adapter Tools (workspace list/read/search
//! in `siralos-adapters`) plus harness-local deterministic stub Tools.
//! Static dispatch cannot represent a runtime selection of different
//! concrete Tool implementations in one ordered table without making
//! the registry generic over every composition, so `Box<dyn Tool>` is
//! confined to the registry entry storage. The application loop and
//! Tool Round receive `&dyn Tool` only at the single lookup/call seam.

use std::collections::HashMap;

use serde_json::Value;

use crate::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use crate::tool::capability::CapabilityId;

/// One generic callable Tool.
///
/// A Tool exposes provider-visible metadata, its opaque capability, and
/// exactly one execution operation. It receives only detached input and
/// the read-only cancellation observation view; it cannot register
/// itself, broaden its capability, cancel the Host, or mutate Host
/// history.
pub trait Tool {
    /// The provider-visible definition (metadata only, never a runtime
    /// validator).
    fn definition(&self) -> ToolDefinition;

    /// The opaque capability required by this Tool.
    fn capability(&self) -> &CapabilityId;

    /// Execute one call under the current Host authorization.
    ///
    /// Input validation is owned by the Tool; invalid input must return
    /// [`ToolExecutionResult::InvalidInput`] before substantive work.
    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult;
}

/// Registration metadata returned by [`ToolRegistry::definitions`].
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredToolInfo {
    /// Provider-visible definition.
    pub definition: ToolDefinition,
    /// The Tool's opaque capability metadata.
    pub capability: CapabilityId,
}

/// Why Tool Registry construction failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRegistryError {
    /// Two tools declare the same exact name.
    DuplicateName(String),
}

impl ToolRegistryError {
    /// The exact externally observable rejection message.
    pub fn message(&self) -> String {
        match self {
            Self::DuplicateName(name) => {
                format!("Duplicate tool name: {name}")
            }
        }
    }
}

impl std::fmt::Display for ToolRegistryError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for ToolRegistryError {}

/// Immutable ordered Tool Registry.
///
/// Observable definition order is the registration order and is stored
/// independently in a `Vec`. The internal `HashMap` is used only for
/// exact case-sensitive O(1) lookup and its iteration order is never
/// observable.
pub struct ToolRegistry {
    entries: Vec<Box<dyn Tool>>,
    index: HashMap<String, usize>,
}

impl ToolRegistry {
    /// Build one registry from concrete tools in registration order.
    ///
    /// Duplicate names fail construction deterministically with the
    /// frozen message. The table is immutable after construction.
    pub fn new(
        tools: impl IntoIterator<Item = Box<dyn Tool>>,
    ) -> Result<Self, ToolRegistryError> {
        let mut entries = Vec::new();
        let mut index = HashMap::new();
        for tool in tools {
            let name = tool.definition().name;
            if index.contains_key(&name) {
                return Err(ToolRegistryError::DuplicateName(name));
            }
            index.insert(name, entries.len());
            entries.push(tool);
        }
        Ok(Self { entries, index })
    }

    /// Exact case-sensitive lookup; unknown names return `None`.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        let position = self.index.get(name)?;
        self.entries.get(*position).map(Box::as_ref)
    }

    /// Fresh registration-ordered definitions.
    ///
    /// The returned `Vec` and each `ToolDefinition` are freshly cloned
    /// owned values; caller mutation cannot affect registry state.
    pub fn definitions(&self) -> Vec<RegisteredToolInfo> {
        self.entries
            .iter()
            .map(|tool| RegisteredToolInfo {
                definition: tool.definition(),
                capability: tool.capability().clone(),
            })
            .collect()
    }

    /// The number of registered tools.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ToolRegistry")
            .field(
                "tools",
                &self
                    .entries
                    .iter()
                    .map(|tool| tool.definition().name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Host-approved visible Tool surface produced by R7.3 projection.
///
/// When present, a proposed registered Tool whose name is absent is denied
/// before execution; when absent, the policy-filtered registry remains the
/// request surface and no additional guard applies. R7.5 only renders this
/// detached surface and never changes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovedToolSurface {
    names: std::collections::BTreeSet<String>,
}

impl ApprovedToolSurface {
    /// Build the approved surface from exact Tool names.
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        Self { names: names.into_iter().collect() }
    }

    /// Whether a Tool name is present in the approved surface.
    pub fn contains(&self, tool_name: &str) -> bool {
        self.names.contains(tool_name)
    }

    /// The number of approved Tool names.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the approved surface is empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Deterministic ordered iteration over the approved names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use crate::provider::{
        CancellationSignal, ToolDefinition, ToolExecutionResult,
    };
    use serde_json::json;

    use super::{
        ApprovedToolSurface, CapabilityId, RegisteredToolInfo, Tool,
        ToolRegistry, ToolRegistryError,
    };

    struct StubTool {
        name: String,
        capability: CapabilityId,
    }

    impl Tool for StubTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.clone(),
                description: format!("Stub {}", self.name),
                input_schema: json!({}),
            }
        }

        fn capability(&self) -> &CapabilityId {
            &self.capability
        }

        fn execute(
            &self,
            _input: &serde_json::Value,
            _cancellation: CancellationSignal<'_>,
        ) -> ToolExecutionResult {
            ToolExecutionResult::Success {
                output: json!({ "ok": true }),
                summary: "ok".to_owned(),
            }
        }
    }

    fn tool(name: &str) -> Box<dyn Tool> {
        Box::new(StubTool {
            name: name.to_owned(),
            capability: CapabilityId::parse("workspace.read").unwrap(),
        })
    }

    #[test]
    fn rejects_duplicate_names_with_the_exact_message() {
        let error = ToolRegistry::new([tool("a.tool"), tool("a.tool")])
            .expect_err("duplicate must fail");
        assert_eq!(
            error,
            ToolRegistryError::DuplicateName("a.tool".to_owned())
        );
        assert_eq!(error.message(), "Duplicate tool name: a.tool");
    }

    #[test]
    fn lookup_is_exact_and_case_sensitive() {
        let registry =
            ToolRegistry::new([tool("a.tool"), tool("A.tool")]).unwrap();
        assert!(registry.get("a.tool").is_some());
        assert!(registry.get("A.tool").is_some());
        assert!(registry.get("a.Tool").is_none());
        assert!(registry.get("x.tool").is_none());
    }

    #[test]
    fn definitions_preserve_registration_order_and_are_detached() {
        let registry =
            ToolRegistry::new([tool("b.tool"), tool("a.tool")]).unwrap();
        let definitions = registry.definitions();
        let names: Vec<&str> = definitions
            .iter()
            .map(|info| info.definition.name.as_str())
            .collect();
        assert_eq!(names, ["b.tool", "a.tool"]);
        let mut definitions = definitions;
        definitions[0].definition.name = "mutated.tool".to_owned();
        definitions[0].capability = CapabilityId::parse("other.read").unwrap();
        assert_eq!(registry.definitions()[0].definition.name, "b.tool");
        let expected: Vec<RegisteredToolInfo> = registry.definitions();
        assert_ne!(definitions[0], expected[0]);
    }

    #[test]
    fn approved_surface_matches_exact_names_deterministically() {
        let surface = ApprovedToolSurface::new([
            "z.tool".to_owned(),
            "a.tool".to_owned(),
        ]);
        assert!(surface.contains("a.tool"));
        assert!(surface.contains("z.tool"));
        assert!(!surface.contains("A.tool"));
        assert_eq!(surface.iter().collect::<Vec<_>>(), ["a.tool", "z.tool"]);
    }
}
