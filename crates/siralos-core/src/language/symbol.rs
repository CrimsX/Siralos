//! Generic symbol model and deterministic symbol result normalization
//! (Stage 3R R5).
//!
//! The symbol kind vocabulary is a small stable generic set derived from
//! the reference structural scanner's declaration categories (class,
//! function, property, constant, enum, enum member, signal) plus an
//! explicit unknown kind; it deliberately does not enumerate
//! language-specific variants. Symbols remain distinguishable by
//! location: duplicate names at different locations are never collapsed.
//! Results are bounded with explicit truncation and ordered
//! deterministically by (line, kind, name) under the reference UTF-16
//! string ordering.

use crate::language::diagnostic::utf16_cmp;
use crate::language::limits::LanguageLimits;

use std::cmp::Ordering;

/// Small stable generic symbol kind vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    /// A class/type declaration.
    Class,
    /// A function declaration.
    Function,
    /// A property/field declaration.
    Property,
    /// A constant declaration.
    Constant,
    /// An enum declaration.
    Enum,
    /// One enum member.
    EnumMember,
    /// A signal/event declaration.
    Signal,
    /// An unknown or opaque kind.
    Unknown,
}

impl SymbolKind {
    /// The canonical protocol string for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Function => "function",
            Self::Property => "property",
            Self::Constant => "constant",
            Self::Enum => "enum",
            Self::EnumMember => "enum_member",
            Self::Signal => "signal",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a protocol kind string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "class" => Some(Self::Class),
            "function" => Some(Self::Function),
            "property" => Some(Self::Property),
            "constant" => Some(Self::Constant),
            "enum" => Some(Self::Enum),
            "enum_member" => Some(Self::EnumMember),
            "signal" => Some(Self::Signal),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// One generic symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Display name.
    pub name: String,
    /// Language-neutral kind.
    pub kind: SymbolKind,
    /// One-based declaration line; None when unknown.
    pub line: Option<u64>,
    /// Optional signature/detail text.
    pub detail: Option<String>,
}

/// A bounded, deterministically ordered symbol result for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolResult {
    /// Workspace-relative path of the document.
    pub path: String,
    /// R4 revision handle of the exact source state observed.
    pub revision: Option<String>,
    /// Symbols ordered by (line, kind, name).
    pub symbols: Vec<Symbol>,
    /// True when the per-document bound was applied.
    pub truncated: bool,
}

/// Normalize one symbol collection: deterministic ordering by (line,
/// kind, name) with the reference UTF-16 string ordering, bounded with
/// explicit truncation. Duplicate names at different locations are
/// preserved (location distinguishes identity).
pub fn normalize_symbols(
    path: &str,
    revision: Option<String>,
    mut symbols: Vec<Symbol>,
    limits: &LanguageLimits,
) -> SymbolResult {
    symbols.sort_by(|left, right| {
        let left_line = left.line.unwrap_or(0);
        let right_line = right.line.unwrap_or(0);
        let left_key: i64 = if left.line.is_some() {
            left_line.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        let right_key: i64 = if right.line.is_some() {
            right_line.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        if left_key != right_key {
            return left_key.cmp(&right_key);
        }
        let by_kind = left.kind.cmp(&right.kind);
        if by_kind != Ordering::Equal {
            return by_kind;
        }
        utf16_cmp(&left.name, &right.name)
    });
    let truncated = symbols.len() > limits.max_symbols_per_document;
    symbols.truncate(limits.max_symbols_per_document);
    SymbolResult { path: path.to_owned(), revision, symbols, truncated }
}
