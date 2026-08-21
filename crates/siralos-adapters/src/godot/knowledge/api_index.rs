//! Bounded API index builder plus literal/token search and exact lookup.
//!
//! The index is built from the exact engine-generated dump: engine-native
//! names are preserved, symbol identities are deterministic, limits are
//! enforced at build time (excess classes or symbols fail safely), and
//! every description is truncated to the immutable bound. The provider
//! can never request the raw dump or raw index files; it receives only
//! bounded structured search and lookup results.

use std::collections::HashSet;

use siralos_core::godot::{
    GODOT_LIMITS, GodotApiIndex, GodotApiLookupResult, GodotApiParameter,
    GodotApiSearchKind, GodotApiSearchOutcome, GodotApiSearchRank,
    GodotApiSearchResult, GodotApiSymbol, GodotApiSymbolDetails,
    GodotApiSymbolKind, GodotApiType, godot_symbol_id,
};
use siralos_core::language::truncate_utf8_bytes;

use super::api_dump::{
    GodotApiDumpClass, GodotApiDumpDocument, GodotApiDumpEnumValue,
    GodotApiDumpMethod,
};

/// Failure of an index build with its bounded message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotApiIndexBuildFailure {
    /// Bounded failure message.
    pub message: String,
}

/// Build the deterministic bounded index from one parsed dump document.
pub fn build_godot_api_index(
    document: &GodotApiDumpDocument,
) -> Result<GodotApiIndex, GodotApiIndexBuildFailure> {
    let max_bytes = GODOT_LIMITS.max_api_dump_with_docs_bytes as u64;
    if document.raw_bytes > max_bytes {
        return Err(failure(&format!(
            "The API documentation dump is {} bytes, exceeding the {max_bytes}-byte bound.",
            document.raw_bytes
        )));
    }
    let class_count = document.classes.len() + document.builtin_classes.len();
    if class_count > GODOT_LIMITS.max_api_classes {
        return Err(failure(&format!(
            "The API dump declares {class_count} classes, exceeding the {}-class bound.",
            GODOT_LIMITS.max_api_classes
        )));
    }
    let mut builder = IndexBuilder::default();
    for godot_class in &document.classes {
        if !builder.add_class_symbols(godot_class) {
            return Err(symbol_limit_failure());
        }
    }
    for builtin in &document.builtin_classes {
        if !builder.add_builtin_symbols(builtin) {
            return Err(symbol_limit_failure());
        }
    }
    for constant in &document.global_constants {
        if !builder.add_constant(
            &constant.name,
            None,
            GodotApiType::Native,
            &constant.value,
            &constant.description,
        ) {
            return Err(symbol_limit_failure());
        }
    }
    for entry in &document.global_enums {
        if !builder.add_enum(
            &entry.name,
            None,
            GodotApiType::Native,
            &entry.values,
            &entry.description,
        ) {
            return Err(symbol_limit_failure());
        }
    }
    for utility in &document.utility_functions {
        if !builder.add_method(
            GodotApiSymbolKind::Utility,
            utility,
            None,
            GodotApiType::Native,
        ) {
            return Err(symbol_limit_failure());
        }
    }
    builder.symbols.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(GodotApiIndex {
        schema_version: GODOT_LIMITS.knowledge_schema_version,
        engine_version: document
            .version_full_name
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        dump_sha256: document.sha256.clone(),
        symbols: builder.symbols,
        dump_bytes: document.raw_bytes,
    })
}

#[derive(Default)]
struct IndexBuilder {
    symbols: Vec<GodotApiSymbol>,
    used_ids: HashSet<String>,
}

impl IndexBuilder {
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        kind: GodotApiSymbolKind,
        name: &str,
        owner: Option<&str>,
        api_type: GodotApiType,
        description: Option<&str>,
        inherited_from: Option<&str>,
        signature: Option<String>,
        details: GodotApiSymbolDetails,
    ) -> bool {
        let mut ordinal: u32 = 1;
        let mut id = godot_symbol_id(kind, name, owner, None);
        while self.used_ids.contains(&id) {
            ordinal += 1;
            id = godot_symbol_id(kind, name, owner, Some(ordinal));
        }
        if self.symbols.len() >= GODOT_LIMITS.max_api_symbols {
            return false;
        }
        self.used_ids.insert(id.clone());
        self.symbols.push(GodotApiSymbol {
            id,
            kind,
            name: name.to_owned(),
            owner: owner.map(str::to_owned),
            api_type,
            summary: summarize(description),
            description: description.map(str::to_owned),
            signature,
            inherited_from: inherited_from.map(str::to_owned),
            ordinal: if ordinal > 1 { Some(ordinal) } else { None },
            details,
        });
        true
    }

    fn add_class_symbols(&mut self, godot_class: &GodotApiDumpClass) -> bool {
        let description =
            first_of(&godot_class.brief_description, &godot_class.description);
        if !self.push(
            GodotApiSymbolKind::Class,
            &godot_class.name,
            None,
            GodotApiType::Native,
            description,
            godot_class.base_class.as_deref(),
            None,
            GodotApiSymbolDetails::default(),
        ) {
            return false;
        }
        for method in &godot_class.methods {
            if !self.add_method(
                GodotApiSymbolKind::Method,
                method,
                Some(&godot_class.name),
                GodotApiType::Native,
            ) {
                return false;
            }
        }
        for property in &godot_class.properties {
            let signature = property
                .prop_type
                .as_ref()
                .map(|prop_type| format!("{}: {prop_type}", property.name));
            let details = GodotApiSymbolDetails {
                param_type: property.prop_type.clone(),
                setter: Some(property.setter.clone()),
                getter: Some(property.getter.clone()),
                ..GodotApiSymbolDetails::default()
            };
            if !self.push(
                GodotApiSymbolKind::Property,
                &property.name,
                Some(&godot_class.name),
                GodotApiType::Native,
                property.description.as_deref(),
                None,
                signature,
                details,
            ) {
                return false;
            }
        }
        for signal in &godot_class.signals {
            let parameters = signal
                .parameters
                .iter()
                .map(|parameter| GodotApiParameter {
                    name: parameter.name.clone(),
                    param_type: parameter.param_type.clone(),
                    default_value: parameter.default_value.clone(),
                })
                .collect();
            let details = GodotApiSymbolDetails {
                parameters,
                ..GodotApiSymbolDetails::default()
            };
            let arguments_text = signal
                .parameters
                .iter()
                .map(|parameter| {
                    format!("{}: {}", parameter.name, parameter.param_type)
                })
                .collect::<Vec<_>>()
                .join(", ");
            if !self.push(
                GodotApiSymbolKind::Signal,
                &signal.name,
                Some(&godot_class.name),
                GodotApiType::Native,
                signal.description.as_deref(),
                None,
                Some(format!("{}({arguments_text})", signal.name)),
                details,
            ) {
                return false;
            }
        }
        for constant in &godot_class.constants {
            if !self.add_constant(
                &constant.name,
                Some(&godot_class.name),
                GodotApiType::Native,
                &constant.value,
                &constant.description,
            ) {
                return false;
            }
        }
        for entry in &godot_class.enums {
            if !self.add_enum(
                &entry.name,
                Some(&godot_class.name),
                GodotApiType::Native,
                &entry.values,
                &entry.description,
            ) {
                return false;
            }
        }
        true
    }

    fn add_builtin_symbols(
        &mut self,
        builtin: &super::api_dump::GodotApiDumpBuiltinClass,
    ) -> bool {
        if !self.push(
            GodotApiSymbolKind::Class,
            &builtin.name,
            None,
            GodotApiType::Builtin,
            builtin.description.as_deref(),
            None,
            None,
            GodotApiSymbolDetails::default(),
        ) {
            return false;
        }
        for method in &builtin.methods {
            if !self.add_method(
                GodotApiSymbolKind::Method,
                method,
                Some(&builtin.name),
                GodotApiType::Builtin,
            ) {
                return false;
            }
        }
        for operator in &builtin.operators {
            if !self.push(
                GodotApiSymbolKind::Operator,
                operator,
                Some(&builtin.name),
                GodotApiType::Builtin,
                None,
                None,
                Some(operator.clone()),
                GodotApiSymbolDetails::default(),
            ) {
                return false;
            }
        }
        for constant in &builtin.constants {
            if !self.add_constant(
                &constant.name,
                Some(&builtin.name),
                GodotApiType::Builtin,
                &constant.value,
                &constant.description,
            ) {
                return false;
            }
        }
        for entry in &builtin.enums {
            if !self.add_enum(
                &entry.name,
                Some(&builtin.name),
                GodotApiType::Builtin,
                &entry.values,
                &entry.description,
            ) {
                return false;
            }
        }
        true
    }

    fn add_method(
        &mut self,
        kind: GodotApiSymbolKind,
        method: &GodotApiDumpMethod,
        owner: Option<&str>,
        api_type: GodotApiType,
    ) -> bool {
        let parameters = method
            .parameters
            .iter()
            .map(|parameter| GodotApiParameter {
                name: parameter.name.clone(),
                param_type: parameter.param_type.clone(),
                default_value: parameter.default_value.clone(),
            })
            .collect();
        let details = GodotApiSymbolDetails {
            return_type: method.return_type.clone(),
            parameters,
            qualifiers: method.qualifiers.clone(),
            hash: method.hash.clone(),
            ..GodotApiSymbolDetails::default()
        };
        if !self.push(
            kind,
            &method.name,
            owner,
            api_type,
            method.description.as_deref(),
            None,
            Some(method_signature(&method.name, method)),
            details,
        ) {
            return false;
        }
        true
    }

    fn add_constant(
        &mut self,
        name: &str,
        owner: Option<&str>,
        api_type: GodotApiType,
        value: &Option<String>,
        description: &Option<String>,
    ) -> bool {
        let signature =
            value.as_ref().map(|value| format!("{name} = {value}"));
        let details = GodotApiSymbolDetails {
            value: value.clone(),
            ..GodotApiSymbolDetails::default()
        };
        self.push(
            GodotApiSymbolKind::Constant,
            name,
            owner,
            api_type,
            description.as_deref(),
            None,
            signature,
            details,
        )
    }

    fn add_enum(
        &mut self,
        name: &str,
        owner: Option<&str>,
        api_type: GodotApiType,
        values: &[GodotApiDumpEnumValue],
        description: &Option<String>,
    ) -> bool {
        let details = GodotApiSymbolDetails {
            values: values
                .iter()
                .map(|value| siralos_core::godot::GodotApiNamedValue {
                    name: value.name.clone(),
                    value: value.value.clone(),
                })
                .collect(),
            ..GodotApiSymbolDetails::default()
        };
        self.push(
            GodotApiSymbolKind::Enum,
            name,
            owner,
            api_type,
            description.as_deref(),
            None,
            None,
            details,
        )
    }
}

/// Literal/token search with deterministic ranking: exact name matches
/// first, then prefix matches, then token matches, then document matches;
/// ties break by name length and then symbol id.
pub fn search_godot_api_index(
    index: &GodotApiIndex,
    query: &str,
    kinds: Option<&[GodotApiSearchKind]>,
    limit: Option<usize>,
) -> GodotApiSearchOutcome {
    let normalized_query = query.trim().to_lowercase();
    let tokens: Vec<&str> = normalized_query
        .split(|character: char| {
            !(character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_')
        })
        .filter(|token| !token.is_empty())
        .collect();
    let limit = limit
        .unwrap_or(GODOT_LIMITS.max_api_search_results)
        .min(GODOT_LIMITS.max_api_search_results);
    let mut ranked: Vec<(&GodotApiSymbol, GodotApiSearchRank)> = Vec::new();
    for symbol in &index.symbols {
        if let Some(kinds) = kinds
            && !kinds.is_empty()
            && !kinds.contains(&search_kind(symbol.kind))
        {
            continue;
        }
        let name = symbol.name.to_lowercase();
        if let Some(rank) = rank_symbol(&name, &tokens, symbol) {
            ranked.push((symbol, rank));
        }
    }
    ranked.sort_by(|(left_symbol, left_rank), (right_symbol, right_rank)| {
        left_rank
            .cmp(right_rank)
            .then_with(|| {
                utf16_len(&left_symbol.name)
                    .cmp(&utf16_len(&right_symbol.name))
            })
            .then_with(|| left_symbol.id.cmp(&right_symbol.id))
    });
    let truncated = ranked.len() > limit;
    let results = ranked
        .into_iter()
        .take(limit)
        .map(|(symbol, rank)| GodotApiSearchResult {
            symbol: symbol.id.clone(),
            kind: symbol.kind,
            name: symbol.name.clone(),
            owner: symbol.owner.clone(),
            summary: symbol.summary.clone(),
            rank,
            api_type: symbol.api_type,
        })
        .collect();
    GodotApiSearchOutcome { results, truncated }
}

/// Exact-symbol lookup; unknown symbols return `None`.
pub fn lookup_godot_api_symbol(
    index: &GodotApiIndex,
    symbol_id: &str,
) -> Option<GodotApiLookupResult> {
    index.symbols.iter().find(|symbol| symbol.id == symbol_id).map(|symbol| {
        GodotApiLookupResult {
            symbol: symbol.id.clone(),
            kind: symbol.kind,
            name: symbol.name.clone(),
            owner: symbol.owner.clone(),
            inherited_from: symbol.inherited_from.clone(),
            signature: symbol.signature.clone(),
            description: symbol.description.clone(),
            api_type: symbol.api_type,
            details: symbol.details.clone(),
        }
    })
}

fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

fn search_kind(kind: GodotApiSymbolKind) -> GodotApiSearchKind {
    match kind {
        GodotApiSymbolKind::Class => GodotApiSearchKind::Class,
        GodotApiSymbolKind::Method => GodotApiSearchKind::Method,
        GodotApiSymbolKind::Property => GodotApiSearchKind::Property,
        GodotApiSymbolKind::Signal => GodotApiSearchKind::Signal,
        GodotApiSymbolKind::Constant => GodotApiSearchKind::Constant,
        GodotApiSymbolKind::Enum => GodotApiSearchKind::Enum,
        GodotApiSymbolKind::Utility => GodotApiSearchKind::Utility,
        GodotApiSymbolKind::Operator => GodotApiSearchKind::Operator,
    }
}

fn rank_symbol(
    name: &str,
    tokens: &[&str],
    symbol: &GodotApiSymbol,
) -> Option<GodotApiSearchRank> {
    let query = tokens.join(" ");
    if query.is_empty() {
        return None;
    }
    if name == query {
        return Some(GodotApiSearchRank::Exact);
    }
    if name.starts_with(&query) {
        return Some(GodotApiSearchRank::Prefix);
    }
    if tokens.iter().any(|token| name.contains(token)) {
        return Some(GodotApiSearchRank::Token);
    }
    let document = format!(
        "{} {}",
        symbol.summary,
        symbol.description.as_deref().unwrap_or("")
    )
    .to_lowercase();
    if tokens.iter().all(|token| document.contains(token)) {
        return Some(GodotApiSearchRank::Document);
    }
    None
}

fn method_signature(name: &str, method: &GodotApiDumpMethod) -> String {
    let qualifiers: Vec<&str> = method
        .qualifiers
        .iter()
        .map(String::as_str)
        .filter(|qualifier| *qualifier == "static" || *qualifier == "vararg")
        .collect();
    let arguments_text = method
        .parameters
        .iter()
        .map(|parameter| match &parameter.default_value {
            None => format!("{}: {}", parameter.name, parameter.param_type),
            Some(default_value) => {
                format!(
                    "{}: {} := {default_value}",
                    parameter.name, parameter.param_type
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = if qualifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", qualifiers.join(" "))
    };
    let return_type = method.return_type.as_deref().unwrap_or("void");
    format!("{prefix}{name}({arguments_text}) -> {return_type}")
}

fn first_of<'a>(
    left: &'a Option<String>,
    right: &'a Option<String>,
) -> Option<&'a str> {
    if let Some(value) = left
        && !value.is_empty()
    {
        return Some(value);
    }
    if let Some(value) = right
        && !value.is_empty()
    {
        return Some(value);
    }
    None
}

fn summarize(description: Option<&str>) -> String {
    let Some(description) = description else {
        return String::new();
    };
    let first_line = description.split('\n').next().unwrap_or("");
    let first_line = first_line.strip_suffix('\r').unwrap_or(first_line);
    truncate_utf8_bytes(first_line.trim(), GODOT_LIMITS.max_api_summary_bytes)
}

fn failure(message: &str) -> GodotApiIndexBuildFailure {
    GodotApiIndexBuildFailure { message: message.to_owned() }
}

fn symbol_limit_failure() -> GodotApiIndexBuildFailure {
    failure(&format!(
        "The API dump expands beyond the {}-symbol bound; the index build failed safely.",
        GODOT_LIMITS.max_api_symbols
    ))
}
