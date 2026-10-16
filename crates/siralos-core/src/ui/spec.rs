//! UI component spec types and bounded validation (ADR 0039).
//!
//! A component is data, never code: a spec declares an id, a title,
//! a feed, placement, size hints, and bounded params. All strings
//! are validated and detached at parse; the Host owns rendering.
//! This module never touches TOML, the filesystem, or the
//! terminal — callers parse their own documents into
//! [`UnvalidatedSpec`] and validate here.

use std::collections::BTreeMap;

/// Maximum id length in bytes (`[a-z0-9_-]`, nonempty).
pub const MAX_SPEC_ID_BYTES: usize = 64;
/// Maximum title length in characters.
pub const MAX_SPEC_TITLE_CHARS: usize = 128;
/// Maximum param entries per spec.
pub const MAX_SPEC_PARAMS: usize = 8;
/// Maximum param key length in bytes.
pub const MAX_SPEC_PARAM_KEY_BYTES: usize = 64;
/// Maximum param value length in bytes.
pub const MAX_SPEC_PARAM_VALUE_BYTES: usize = 1024;
/// Minimum size hint (rows or columns).
pub const MIN_SPEC_SIZE_HINT: usize = 1;
/// Maximum size hint (rows or columns).
pub const MAX_SPEC_SIZE_HINT: usize = 99;
/// Default row hint when a document omits `max_rows`.
pub const DEFAULT_SPEC_MAX_ROWS: i64 = 20;
/// Default column hint when a document omits `max_cols`.
pub const DEFAULT_SPEC_MAX_COLS: i64 = 60;
/// Grid rows addressable by `cell:<row>,<col>` placement.
pub const GRID_ROWS: usize = 2;
/// Grid columns addressable by `cell:<row>,<col>` placement.
pub const GRID_COLS: usize = 2;
/// Fixed panel-snapshot viewport width (columns). This single-sources
/// the harness-pinned 80x24 render viewport also used by the
/// `tui-render` snapshot; panel content itself stays bounded by
/// the per-spec size hints.
pub const UI_VIEWPORT_WIDTH: u16 = 80;
/// Fixed panel-snapshot viewport height (rows). See
/// [`UI_VIEWPORT_WIDTH`].
pub const UI_VIEWPORT_HEIGHT: u16 = 24;

/// Closed feed vocabulary (ADR 0039 §2). Feeds map to existing
/// authority classes; no feed grants new authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiFeed {
    /// Bounded workspace listing (`workspace.read`).
    WorkspaceList,
    /// Bounded workspace search (`workspace.read`).
    WorkspaceSearch,
    /// Bounded context summary, no raw content (already visible).
    ContextSummary,
    /// Registry definitions snapshot (already provider-visible).
    ToolsCatalog,
    /// Install records (local user config).
    ModulesStatus,
    /// Checkpoint metadata only, never preimages (`workspace.read`).
    CheckpointsList,
}

impl UiFeed {
    /// Parse the exact wire string. Anything else is refused;
    /// feeds are a closed enum, never an extension point.
    pub fn parse(value: &str) -> Result<Self, UiSpecError> {
        match value {
            "workspace.list" => Ok(Self::WorkspaceList),
            "workspace.search" => Ok(Self::WorkspaceSearch),
            "context.summary" => Ok(Self::ContextSummary),
            "tools.catalog" => Ok(Self::ToolsCatalog),
            "modules.status" => Ok(Self::ModulesStatus),
            "checkpoints.list" => Ok(Self::CheckpointsList),
            _ => Err(UiSpecError::UnknownFeed),
        }
    }

    /// The canonical wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceList => "workspace.list",
            Self::WorkspaceSearch => "workspace.search",
            Self::ContextSummary => "context.summary",
            Self::ToolsCatalog => "tools.catalog",
            Self::ModulesStatus => "modules.status",
            Self::CheckpointsList => "checkpoints.list",
        }
    }
}

/// Placement of one component in the panel host: a full-bleed tab
/// or one cell of the 2×2 split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPlacement {
    /// Full-area panel behind its own tab.
    Tab,
    /// Pinned grid cell.
    Cell {
        /// Grid row (`0..GRID_ROWS`).
        row: usize,
        /// Grid column (`0..GRID_COLS`).
        col: usize,
    },
}

impl UiPlacement {
    /// Parse `"tab"` or `"cell:<row>,<col>"` exactly (lowercase,
    /// no whitespace). Anything else is refused.
    pub fn parse(value: &str) -> Result<Self, UiSpecError> {
        if value == "tab" {
            return Ok(Self::Tab);
        }
        let tail = value
            .strip_prefix("cell:")
            .ok_or(UiSpecError::InvalidPlacement)?;
        let (row, col) =
            tail.split_once(',').ok_or(UiSpecError::InvalidPlacement)?;
        let row: usize =
            row.parse().map_err(|_| UiSpecError::InvalidPlacement)?;
        let col: usize =
            col.parse().map_err(|_| UiSpecError::InvalidPlacement)?;
        if row >= GRID_ROWS || col >= GRID_COLS {
            return Err(UiSpecError::InvalidPlacement);
        }
        Ok(Self::Cell { row, col })
    }
}

impl std::fmt::Display for UiPlacement {
    /// The canonical wire string: `"tab"` or `"cell:<row>,<col>"`.
    /// This is the single spelling; every surface renders
    /// placements through here and parses them with
    /// [`UiPlacement::parse`].
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Tab => formatter.write_str("tab"),
            Self::Cell { row, col } => {
                write!(formatter, "cell:{row},{col}")
            }
        }
    }
}

/// Why a component spec was refused. Every refusal is typed; a
/// malformed document never yields a partially applied spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSpecError {
    /// The id is empty, oversized, or outside `[a-z0-9_-]`.
    InvalidId,
    /// The title is empty, oversized, or carries control characters.
    InvalidTitle,
    /// The feed string names no closed-vocabulary feed.
    UnknownFeed,
    /// The placement string is not `tab` or `cell:<row>,<col>`.
    InvalidPlacement,
    /// A size hint is outside `1..=99`.
    SizeOutOfRange,
    /// More than `MAX_SPEC_PARAMS` params.
    TooManyParams,
    /// A param key is empty, oversized, or outside `[a-z0-9_-]`.
    InvalidParamKey,
    /// A param value is oversized.
    ParamTooLarge,
}

impl UiSpecError {
    /// The exact externally observable refusal message.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidId => {
                "component id must be 1..=64 [a-z0-9_-] characters"
            }
            Self::InvalidTitle => {
                "component title must be 1..=128 characters with no controls"
            }
            Self::UnknownFeed => "component feed is not a known feed",
            Self::InvalidPlacement => {
                "component placement must be `tab` or `cell:<row>,<col>`"
            }
            Self::SizeOutOfRange => "component size hints must be 1..=99",
            Self::TooManyParams => "component carries too many params",
            Self::InvalidParamKey => {
                "component param keys must be 1..=64 [a-z0-9_-] characters"
            }
            Self::ParamTooLarge => "component param values must fit 1KiB",
        }
    }
}

impl std::fmt::Display for UiSpecError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for UiSpecError {}

fn valid_spec_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_SPEC_ID_BYTES {
        return false;
    }
    value.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || byte == b'_'
            || byte == b'-'
    })
}

fn has_controls(value: &str) -> bool {
    value.chars().any(|cell| cell.is_control() || cell == '\u{7f}')
}

/// One unvalidated component spec as parsed from a TOML document
/// by the caller (adapters for manifests, cli for `ui.toml`).
/// Plain owned data; validation happens in [`UiComponentSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnvalidatedSpec {
    /// Raw id string.
    pub id: String,
    /// Raw title string.
    pub title: String,
    /// Raw feed string.
    pub feed: String,
    /// Whether the component renders when merged.
    pub enabled: bool,
    /// Raw placement string.
    pub placement: String,
    /// Raw row hint.
    pub max_rows: i64,
    /// Raw column hint.
    pub max_cols: i64,
    /// Raw params in document order.
    pub params: Vec<(String, String)>,
}

/// One validated UI component spec. Only [`UiComponentSpec::parse`]
/// constructs it, so holders never re-check bounds at use sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiComponentSpec {
    id: String,
    title: String,
    feed: UiFeed,
    enabled: bool,
    placement: UiPlacement,
    max_rows: usize,
    max_cols: usize,
    params: BTreeMap<String, String>,
}

impl UiComponentSpec {
    /// Validate one parsed spec. The first failure wins with its
    /// typed error; no partial spec escapes.
    pub fn parse(raw: &UnvalidatedSpec) -> Result<Self, UiSpecError> {
        if !valid_spec_id(&raw.id) {
            return Err(UiSpecError::InvalidId);
        }
        if raw.title.is_empty()
            || raw.title.chars().count() > MAX_SPEC_TITLE_CHARS
            || has_controls(&raw.title)
        {
            return Err(UiSpecError::InvalidTitle);
        }
        let feed = UiFeed::parse(&raw.feed)?;
        let placement = UiPlacement::parse(&raw.placement)?;
        if raw.max_rows < MIN_SPEC_SIZE_HINT as i64
            || raw.max_rows > MAX_SPEC_SIZE_HINT as i64
            || raw.max_cols < MIN_SPEC_SIZE_HINT as i64
            || raw.max_cols > MAX_SPEC_SIZE_HINT as i64
        {
            return Err(UiSpecError::SizeOutOfRange);
        }
        if raw.params.len() > MAX_SPEC_PARAMS {
            return Err(UiSpecError::TooManyParams);
        }
        let mut params = BTreeMap::new();
        for (key, value) in &raw.params {
            if key.is_empty()
                || key.len() > MAX_SPEC_PARAM_KEY_BYTES
                || !key.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
            {
                return Err(UiSpecError::InvalidParamKey);
            }
            if value.len() > MAX_SPEC_PARAM_VALUE_BYTES {
                return Err(UiSpecError::ParamTooLarge);
            }
            params.insert(key.clone(), value.clone());
        }
        Ok(Self {
            id: raw.id.clone(),
            title: raw.title.clone(),
            feed,
            enabled: raw.enabled,
            placement,
            max_rows: raw.max_rows as usize,
            max_cols: raw.max_cols as usize,
            params,
        })
    }

    /// The validated component id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The validated title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The validated feed.
    #[must_use]
    pub fn feed(&self) -> UiFeed {
        self.feed
    }

    /// Whether the component renders when merged.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The validated placement.
    #[must_use]
    pub fn placement(&self) -> UiPlacement {
        self.placement
    }

    /// The validated row hint.
    #[must_use]
    pub fn max_rows(&self) -> usize {
        self.max_rows
    }

    /// The validated column hint.
    #[must_use]
    pub fn max_cols(&self) -> usize {
        self.max_cols
    }

    /// The validated params in canonical order.
    #[must_use]
    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }
}

/// Where a component spec was declared. User specs live in
/// workspace `.siralos/ui.toml`; module specs are re-parsed from
/// installed module manifests each session, never cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecSource {
    /// Workspace user spec.
    User,
    /// Module-declared spec, with the declaring module id.
    Module {
        /// Declaring module package id.
        id: String,
    },
}

/// One validated spec with its declaration source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcedSpec {
    /// Declaration source.
    pub source: SpecSource,
    /// The validated spec.
    pub spec: UiComponentSpec,
}

/// Merge user and module specs into registration order: user
/// specs first in file order, then module specs in caller order,
/// with user specs winning every id conflict. Deterministic for
/// identical inputs.
#[must_use]
pub fn merge_specs(
    user: Vec<UiComponentSpec>,
    modules: Vec<(String, UiComponentSpec)>,
) -> Vec<SourcedSpec> {
    let mut merged: Vec<SourcedSpec> = user
        .into_iter()
        .map(|spec| SourcedSpec { source: SpecSource::User, spec })
        .collect();
    for (module_id, spec) in modules {
        if merged.iter().any(|sourced| sourced.spec.id() == spec.id()) {
            continue;
        }
        merged.push(SourcedSpec {
            source: SpecSource::Module { id: module_id },
            spec,
        });
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{
        UiComponentSpec, UiFeed, UiPlacement, UiSpecError, UnvalidatedSpec,
    };

    fn valid_raw() -> UnvalidatedSpec {
        UnvalidatedSpec {
            id: "recent-files".to_owned(),
            title: "Recent files".to_owned(),
            feed: "workspace.list".to_owned(),
            enabled: true,
            placement: "cell:0,1".to_owned(),
            max_rows: 20,
            max_cols: 60,
            params: vec![("path".to_owned(), "src".to_owned())],
        }
    }

    #[test]
    fn feed_vocabulary_is_closed_and_round_trips() {
        for (wire, feed) in [
            ("workspace.list", UiFeed::WorkspaceList),
            ("workspace.search", UiFeed::WorkspaceSearch),
            ("context.summary", UiFeed::ContextSummary),
            ("tools.catalog", UiFeed::ToolsCatalog),
            ("modules.status", UiFeed::ModulesStatus),
            ("checkpoints.list", UiFeed::CheckpointsList),
        ] {
            assert_eq!(UiFeed::parse(wire).expect("known feed"), feed);
            assert_eq!(feed.as_str(), wire);
        }
        assert_eq!(
            UiFeed::parse("workspace.delete"),
            Err(UiSpecError::UnknownFeed)
        );
        assert_eq!(UiFeed::parse(""), Err(UiSpecError::UnknownFeed));
        assert_eq!(
            UiFeed::parse("Workspace.List"),
            Err(UiSpecError::UnknownFeed)
        );
    }

    #[test]
    fn placement_parses_tabs_and_in_grid_cells_only() {
        assert_eq!(UiPlacement::parse("tab").expect("tab"), UiPlacement::Tab);
        assert_eq!(
            UiPlacement::parse("cell:0,0").expect("origin"),
            UiPlacement::Cell { row: 0, col: 0 }
        );
        assert_eq!(
            UiPlacement::parse("cell:1,1").expect("corner"),
            UiPlacement::Cell { row: 1, col: 1 }
        );
        for bad in [
            "",
            "cell",
            "cell:2,0",
            "cell:0,2",
            "cell:0",
            "cell:0,0,0",
            "cell:-1,0",
            "cell:0,0 ",
            "Cell:0,0",
            "TAB",
        ] {
            assert_eq!(
                UiPlacement::parse(bad),
                Err(UiSpecError::InvalidPlacement),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn placement_display_round_trips_through_parse() {
        for placement in [
            UiPlacement::Tab,
            UiPlacement::Cell { row: 0, col: 0 },
            UiPlacement::Cell { row: 0, col: 1 },
            UiPlacement::Cell { row: 1, col: 0 },
            UiPlacement::Cell { row: 1, col: 1 },
        ] {
            let text = placement.to_string();
            assert_eq!(
                UiPlacement::parse(&text).expect("display output parses"),
                placement,
                "{text:?}"
            );
        }
        assert_eq!(UiPlacement::Tab.to_string(), "tab");
        assert_eq!(
            UiPlacement::Cell { row: 1, col: 0 }.to_string(),
            "cell:1,0"
        );
    }

    #[test]
    fn valid_spec_parses_with_all_fields() {
        let spec = UiComponentSpec::parse(&valid_raw()).expect("valid");
        assert_eq!(spec.id(), "recent-files");
        assert_eq!(spec.title(), "Recent files");
        assert_eq!(spec.feed(), UiFeed::WorkspaceList);
        assert!(spec.enabled());
        assert_eq!(spec.placement(), UiPlacement::Cell { row: 0, col: 1 });
        assert_eq!(spec.max_rows(), 20);
        assert_eq!(spec.max_cols(), 60);
        assert_eq!(spec.params().get("path").map(String::as_str), Some("src"));
    }

    #[test]
    fn hostile_ids_titles_and_bounds_are_refused() {
        let mut raw = valid_raw();
        for id in ["", "ABC", "a b", "a/b", &"a".repeat(65)] {
            raw.id = id.to_owned();
            assert_eq!(
                UiComponentSpec::parse(&raw),
                Err(UiSpecError::InvalidId),
                "{id:?}"
            );
        }
        raw = valid_raw();
        for title in ["", &"a".repeat(129), "a\nb", "a\x1bb", "a\x7fb"] {
            raw.title = title.to_owned();
            assert_eq!(
                UiComponentSpec::parse(&raw),
                Err(UiSpecError::InvalidTitle),
                "{title:?}"
            );
        }
        raw = valid_raw();
        for (rows, cols) in [(0, 60), (20, 0), (100, 60), (20, 100), (-5, 60)]
        {
            raw.max_rows = rows;
            raw.max_cols = cols;
            assert_eq!(
                UiComponentSpec::parse(&raw),
                Err(UiSpecError::SizeOutOfRange),
                "{rows}x{cols}"
            );
        }
        raw = valid_raw();
        raw.params = (0..9)
            .map(|index| (format!("k{index}"), "v".to_owned()))
            .collect();
        assert_eq!(
            UiComponentSpec::parse(&raw),
            Err(UiSpecError::TooManyParams)
        );
        raw = valid_raw();
        raw.params = vec![("Bad Key!".to_owned(), "v".to_owned())];
        assert_eq!(
            UiComponentSpec::parse(&raw),
            Err(UiSpecError::InvalidParamKey)
        );
        raw = valid_raw();
        raw.params = vec![("ok".to_owned(), "v".repeat(1025))];
        assert_eq!(
            UiComponentSpec::parse(&raw),
            Err(UiSpecError::ParamTooLarge)
        );
    }

    #[test]
    fn refusal_messages_are_stable() {
        assert_eq!(
            UiSpecError::UnknownFeed.message(),
            "component feed is not a known feed"
        );
        assert_eq!(
            UiSpecError::InvalidPlacement.message(),
            "component placement must be `tab` or `cell:<row>,<col>`"
        );
    }

    fn parsed(id: &str, title: &str) -> UiComponentSpec {
        UiComponentSpec::parse(&UnvalidatedSpec {
            id: id.to_owned(),
            title: title.to_owned(),
            feed: "tools.catalog".to_owned(),
            enabled: true,
            placement: "tab".to_owned(),
            max_rows: 20,
            max_cols: 60,
            params: Vec::new(),
        })
        .expect("test spec parses")
    }

    #[test]
    fn merge_prefers_user_specs_and_keeps_order() {
        use super::{SpecSource, merge_specs};
        let merged = merge_specs(
            vec![parsed("user-only", "User"), parsed("shared", "Mine")],
            vec![
                ("mod-a".to_owned(), parsed("shared", "Theirs")),
                ("mod-a".to_owned(), parsed("mod-only", "Mod")),
            ],
        );
        let ids: Vec<&str> =
            merged.iter().map(|sourced| sourced.spec.id()).collect();
        assert_eq!(ids, vec!["user-only", "shared", "mod-only"]);
        assert!(matches!(merged[1].source, SpecSource::User));
        assert_eq!(merged[1].spec.title(), "Mine");
        assert!(matches!(merged[2].source, SpecSource::Module { .. }));
    }
}
