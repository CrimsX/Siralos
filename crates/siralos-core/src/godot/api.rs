//! Provider-neutral Godot API symbol model (R8).
//!
//! Mirrors `packages/core/src/godot/api.ts`.
//! Core owns symbol kinds, deterministic identities, and query/result models.
//! Parsing, index building, and querying are adapter-owned.

/// Symbol kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotApiSymbolKind {
    /// Class.
    Class,
    /// Method.
    Method,
    /// Property.
    Property,
    /// Signal.
    Signal,
    /// Constant.
    Constant,
    /// Enum.
    Enum,
    /// Utility function.
    Utility,
    /// Operator.
    Operator,
}

/// Whether a symbol comes from a native or built-in class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotApiType {
    /// Native engine class.
    Native,
    /// Built-in class.
    Builtin,
}

/// Bounded parameter of a method/signal/utility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiParameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    pub param_type: String,
    /// Default-argument expression, if any.
    pub default_value: Option<String>,
}

/// Bounded symbol details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiSymbolDetails {
    /// Return type, if any.
    pub return_type: Option<String>,
    /// Qualifiers (e.g. `static`, `vararg`).
    pub qualifiers: Vec<String>,
    /// Engine-provided method hash, if any.
    pub hash: Option<String>,
    /// Property type, if known.
    pub param_type: Option<String>,
}

/// One bounded indexed API symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiSymbol {
    /// Deterministic id, e.g. `class:Node/method:add_child`.
    pub id: String,
    /// Kind.
    pub kind: GodotApiSymbolKind,
    /// Engine-native name.
    pub name: String,
    /// Owning class or `None` for globals.
    pub owner: Option<String>,
    /// Native or built-in.
    pub api_type: GodotApiType,
    /// First-line summary.
    pub summary: String,
    /// Full description, if any.
    pub description: Option<String>,
    /// Canonical signature, if any.
    pub signature: Option<String>,
    /// Inherited from class, if known.
    pub inherited_from: Option<String>,
    /// Overload ordinal (1-based), if any.
    pub ordinal: Option<u32>,
    /// Details.
    pub details: GodotApiSymbolDetails,
}

/// Deterministic symbol identity, matching `godotSymbolId` in `api.ts`.
#[must_use]
pub fn godot_symbol_id(
    kind: GodotApiSymbolKind,
    name: &str,
    owner: Option<&str>,
    ordinal: Option<u32>,
) -> String {
    let ord = match ordinal {
        Some(n) if n > 1 => format!("#{n}"),
        _ => String::new(),
    };
    match kind {
        GodotApiSymbolKind::Utility => format!("utility:{name}{ord}"),
        GodotApiSymbolKind::Constant | GodotApiSymbolKind::Enum => {
            if owner.is_none_or(|o| o.is_empty()) {
                format!("global:{kind:?}:{name}{ord}").to_lowercase()
            } else {
                format!(
                    "class:{}/{}:{name}{ord}",
                    owner.unwrap_or_default(),
                    match kind {
                        GodotApiSymbolKind::Constant => "constant",
                        GodotApiSymbolKind::Enum => "enum",
                        _ => unreachable!(),
                    }
                )
            }
        }
        GodotApiSymbolKind::Class => format!("class:{name}{ord}"),
        GodotApiSymbolKind::Operator => {
            if owner.is_none_or(|o| o.is_empty()) {
                format!("operator:{name}{ord}")
            } else {
                format!(
                    "class:{}/operator:{name}{ord}",
                    owner.unwrap_or_default()
                )
            }
        }
        _ => {
            if owner.is_none_or(|o| o.is_empty()) {
                format!("{kind:?}:{name}{ord}").to_lowercase()
            } else {
                format!(
                    "class:{}/{}:{name}{ord}",
                    owner.unwrap_or_default(),
                    format!("{kind:?}").to_lowercase()
                )
            }
        }
    }
}

/// Search kind filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotApiSearchKind {
    /// Class.
    Class,
    /// Method.
    Method,
    /// Property.
    Property,
    /// Signal.
    Signal,
    /// Constant.
    Constant,
    /// Enum.
    Enum,
    /// Utility.
    Utility,
    /// Operator.
    Operator,
}

/// Search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiSearchQuery {
    /// Required query text.
    pub query: String,
    /// Optional kind filter.
    pub kinds: Option<Vec<GodotApiSearchKind>>,
    /// Optional result bound.
    pub limit: Option<usize>,
}

/// Rank tier for search hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GodotApiSearchRank {
    /// Exact name match.
    Exact,
    /// Prefix match.
    Prefix,
    /// Token match.
    Token,
    /// Document (full-text) match.
    Document,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiSearchResult {
    /// Symbol id.
    pub symbol: String,
    /// Kind.
    pub kind: GodotApiSymbolKind,
    /// Name.
    pub name: String,
    /// Owner, if any.
    pub owner: Option<String>,
    /// Summary.
    pub summary: String,
    /// Rank.
    pub rank: GodotApiSearchRank,
    /// Native or built-in.
    pub api_type: GodotApiType,
}

#[cfg(test)]
mod tests {
    use super::{GodotApiSymbolKind, godot_symbol_id};

    #[test]
    fn symbol_id_examples() {
        assert_eq!(
            godot_symbol_id(GodotApiSymbolKind::Class, "Node", None, None),
            "class:Node"
        );
        assert_eq!(
            godot_symbol_id(
                GodotApiSymbolKind::Method,
                "add_child",
                Some("Node"),
                None
            ),
            "class:Node/method:add_child"
        );
        assert_eq!(
            godot_symbol_id(GodotApiSymbolKind::Utility, "lerp", None, None),
            "utility:lerp"
        );
    }

    #[test]
    fn symbol_id_with_ordinal() {
        assert_eq!(
            godot_symbol_id(
                GodotApiSymbolKind::Method,
                "foo",
                Some("Node"),
                Some(2)
            ),
            "class:Node/method:foo#2"
        );
        assert_eq!(
            godot_symbol_id(
                GodotApiSymbolKind::Method,
                "foo",
                Some("Node"),
                Some(1)
            ),
            "class:Node/method:foo"
        );
    }
}
