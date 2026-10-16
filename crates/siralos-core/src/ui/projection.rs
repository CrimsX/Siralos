//! UI component projection: registry plus one projection code
//! path (ADR 0039 §6, shaped like `ToolProjector`).
//!
//! The projector owns spec ordering, enablement filtering, cell
//! occupancy, and size truncation — all pure and deterministic.
//! Feed content arrives already resolved and sanitized from the
//! caller (adapters resolve, the cli sanitizes); the projector
//! never fetches, never renders, and never trusts unsanitized
//! text beyond truncating it to the spec bounds.

use super::spec::{GRID_COLS, GRID_ROWS, UiComponentSpec, UiPlacement};

/// Marker appended when lines are cut to the row hint.
pub const UI_TRUNCATION_MARKER: &str = "…";

/// One projected slot: a titled, bounded, sanitized line vector
/// ready for the TUI host to draw verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedSlot {
    /// Component id (traceability, never rendered raw).
    pub id: String,
    /// Sanitized title.
    pub title: String,
    /// Sanitized content lines, truncated to the row hint with a
    /// host-text marker when cut; each line truncated to the
    /// column hint at a character boundary.
    pub lines: Vec<String>,
}

/// The projected panel model: ordered tabs plus the 2×2 grid.
/// Empty slots stay `None`; the host renders placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiProjection {
    /// `tab`-placed components in registration order.
    pub tabs: Vec<ProjectedSlot>,
    /// `cell`-placed components by row and column.
    pub grid: [[Option<ProjectedSlot>; GRID_COLS]; GRID_ROWS],
}

/// Why registration refused a spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRegistryError {
    /// Another spec already occupies the grid cell.
    CellOccupied,
}

impl UiRegistryError {
    /// The exact externally observable refusal message.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::CellOccupied => {
                "another component already occupies the grid cell"
            }
        }
    }
}

impl std::fmt::Display for UiRegistryError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for UiRegistryError {}

/// Truncate sanitized lines to the spec bounds: at most `max_rows`
/// lines plus one host-text marker when cut, each line at most
/// `max_cols` characters (character-boundary safe).
fn truncate_lines(
    lines: &[String],
    max_rows: usize,
    max_cols: usize,
) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for line in lines.iter().take(max_rows) {
        let cut: String = line.chars().take(max_cols).collect();
        kept.push(cut);
    }
    if lines.len() > max_rows {
        kept.push(format!(
            "{UI_TRUNCATION_MARKER} ({} more lines)",
            lines.len() - max_rows
        ));
    }
    kept
}

/// Ordered registry of validated component specs. Insert replaces
/// a same-id spec (the merge layer owns precedence); a second spec
/// claiming an occupied grid cell is refused typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiRegistry {
    specs: Vec<UiComponentSpec>,
}

impl UiRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { specs: Vec::new() }
    }

    /// Insert a spec, replacing any same-id entry. Refuses a grid
    /// cell another spec already occupies.
    pub fn insert(
        &mut self,
        spec: UiComponentSpec,
    ) -> Result<(), UiRegistryError> {
        if let UiPlacement::Cell { row, col } = spec.placement() {
            let clash = self.specs.iter().any(|other| {
                other.id() != spec.id()
                    && other.placement() == UiPlacement::Cell { row, col }
            });
            if clash {
                return Err(UiRegistryError::CellOccupied);
            }
        }
        if let Some(slot) =
            self.specs.iter_mut().find(|other| other.id() == spec.id())
        {
            *slot = spec;
        } else {
            self.specs.push(spec);
        }
        Ok(())
    }

    /// Remove a spec by id, reporting whether one existed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.specs.len();
        self.specs.retain(|spec| spec.id() != id);
        self.specs.len() != before
    }

    /// Look up a spec by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&UiComponentSpec> {
        self.specs.iter().find(|spec| spec.id() == id)
    }

    /// The registered specs in registration order.
    #[must_use]
    pub fn specs(&self) -> &[UiComponentSpec] {
        &self.specs
    }
}

/// Project registered specs into ordered slots. `resolve` supplies
/// one component's sanitized feed lines; missing content resolves
/// to an empty slot (the host renders the placeholder). Disabled
/// specs never reach the projection. Deterministic: same specs
/// plus same content always yields the same projection.
#[must_use]
pub fn project_components(
    registry: &UiRegistry,
    resolve: impl Fn(&UiComponentSpec) -> Vec<String>,
) -> UiProjection {
    let mut tabs = Vec::new();
    let mut grid: [[Option<ProjectedSlot>; GRID_COLS]; GRID_ROWS] =
        [[None, None], [None, None]];
    for spec in registry.specs() {
        if !spec.enabled() {
            continue;
        }
        let slot = ProjectedSlot {
            id: spec.id().to_owned(),
            title: spec.title().to_owned(),
            lines: truncate_lines(
                &resolve(spec),
                spec.max_rows(),
                spec.max_cols(),
            ),
        };
        match spec.placement() {
            UiPlacement::Tab => tabs.push(slot),
            UiPlacement::Cell { row, col } => {
                grid[row][col] = Some(slot);
            }
        }
    }
    UiProjection { tabs, grid }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{UiRegistry, UiRegistryError, project_components};
    use crate::ui::spec::{UiComponentSpec, UiFeed, UnvalidatedSpec};

    fn spec(id: &str, placement: &str, enabled: bool) -> UiComponentSpec {
        UiComponentSpec::parse(&UnvalidatedSpec {
            id: id.to_owned(),
            title: format!("{id} title"),
            feed: UiFeed::ToolsCatalog.as_str().to_owned(),
            enabled,
            placement: placement.to_owned(),
            max_rows: 20,
            max_cols: 60,
            params: Vec::new(),
        })
        .expect("test spec parses")
    }

    #[test]
    fn tabs_order_grid_cells_and_disabled_filtering() {
        let mut registry = UiRegistry::new();
        registry.insert(spec("b-tab", "tab", true)).expect("insert");
        registry.insert(spec("cell", "cell:1,0", true)).expect("insert");
        registry.insert(spec("a-tab", "tab", true)).expect("insert");
        registry.insert(spec("off", "tab", false)).expect("insert");
        let mut contents = BTreeMap::new();
        contents.insert("b-tab".to_owned(), vec!["b".to_owned()]);
        contents.insert("cell".to_owned(), vec!["c".to_owned()]);
        contents.insert("a-tab".to_owned(), vec!["a".to_owned()]);
        let projected = project_components(&registry, |spec| {
            contents.get(spec.id()).cloned().unwrap_or_default()
        });
        let titles: Vec<&str> =
            projected.tabs.iter().map(|slot| slot.title.as_str()).collect();
        assert_eq!(titles, vec!["b-tab title", "a-tab title"]);
        let cell = projected.grid[1][0].as_ref().expect("cell filled");
        assert_eq!(cell.title, "cell title");
        assert_eq!(cell.lines, vec!["c".to_owned()]);
        assert!(projected.grid[0][0].is_none());
        assert!(projected.grid[0][1].is_none());
        assert!(projected.grid[1][1].is_none());
    }

    #[test]
    fn missing_content_resolves_to_an_empty_slot() {
        let mut registry = UiRegistry::new();
        registry.insert(spec("lonely", "tab", true)).expect("insert");
        let projected = project_components(&registry, |_| Vec::new());
        assert_eq!(projected.tabs.len(), 1);
        assert!(projected.tabs[0].lines.is_empty());
    }

    #[test]
    fn truncation_cuts_rows_and_chars_with_a_host_marker() {
        let mut raw = UnvalidatedSpec {
            id: "long".to_owned(),
            title: "long".to_owned(),
            feed: UiFeed::WorkspaceList.as_str().to_owned(),
            enabled: true,
            placement: "tab".to_owned(),
            max_rows: 2,
            max_cols: 4,
            params: Vec::new(),
        };
        let parsed = UiComponentSpec::parse(&raw).expect("parses");
        let mut registry = UiRegistry::new();
        registry.insert(parsed).expect("insert");
        raw.id = "unused".to_owned();
        let lines = vec![
            "abcdef".to_owned(),
            "héllo wörld".to_owned(),
            "third".to_owned(),
        ];
        let projected = project_components(&registry, |_| lines.clone());
        assert_eq!(
            projected.tabs[0].lines,
            vec![
                "abcd".to_owned(),
                "héll".to_owned(),
                "… (1 more lines)".to_owned()
            ]
        );
    }

    #[test]
    fn duplicate_cells_refuse_and_same_ids_replace() {
        let mut registry = UiRegistry::new();
        registry.insert(spec("first", "cell:0,0", true)).expect("insert");
        assert_eq!(
            registry.insert(spec("second", "cell:0,0", true)),
            Err(UiRegistryError::CellOccupied)
        );
        assert_eq!(
            UiRegistryError::CellOccupied.message(),
            "another component already occupies the grid cell"
        );
        // Same id replaces, even onto its own cell.
        registry.insert(spec("first", "cell:0,0", false)).expect("replace");
        assert_eq!(registry.specs().len(), 1);
        assert!(!registry.get("first").expect("present").enabled());
        assert!(registry.remove("first"));
        assert!(!registry.remove("first"));
        assert!(registry.get("first").is_none());
    }
}
