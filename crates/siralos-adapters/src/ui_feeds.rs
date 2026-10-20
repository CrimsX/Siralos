//! Read-only UI feed resolvers (ADR 0039 §2).
//!
//! Each feed reuses an existing read-only projection; no feed
//! grants authority, opens new surfaces, or bypasses gates. Feed
//! content is raw strings: the cli sanitizes every line before it
//! reaches the projector or the terminal. Params go through the
//! same bounded, path-contained parsing as the tools.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use siralos_core::tool::{
    PermissionDecision, PermissionPolicy, RegisteredToolInfo,
    evaluate_permission,
};
use siralos_core::ui::UiFeed;
use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

use crate::module::manifest::ModuleRecord;
use crate::workspace::checkpoint::open_checkpoint_store;
use crate::workspace::list::{ListOutcome, list_directory};
use crate::workspace::search::{SearchInput, SearchOutcome, search};

/// Maximum checkpoints one feed resolution lists.
pub const MAX_FEED_CHECKPOINTS: usize = 20;
/// Maximum lines one feed resolution returns before projection
/// truncation. Feeds stay small; the projector enforces spec
/// bounds on top.
pub const MAX_FEED_LINES: usize = 200;

/// The existing module-namespace capability one feed draws on,
/// if any (dash form: `workspace-read`, matching manifest and
/// grant vocabularies — not the dotted tool capability ids).
/// Feeds with no gate show already-visible host state and are open
/// to every spec; gated feeds require the capability in the
/// declaring module's authority (ADR 0039 §2).
pub fn feed_capability(feed: UiFeed) -> Option<&'static str> {
    match feed {
        UiFeed::WorkspaceList | UiFeed::WorkspaceSearch => {
            Some("workspace-read")
        }
        UiFeed::CheckpointsList => Some("workspace-read"),
        UiFeed::ContextSummary
        | UiFeed::ToolsCatalog
        | UiFeed::ModulesStatus
        | UiFeed::TimeWindow => None,
    }
}

/// Wall-clock boundary for timestamp-bearing feeds: unix milliseconds
/// in UTC. `None` (pre-epoch clock) means "unknown"; such feeds render
/// their honest no-time form rather than a fabricated epoch date.
#[must_use]
pub fn now_utc_millis() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .ok()
}

/// Bounded context summary, built from a projection snapshot by
/// the caller. Numbers and closed-vocabulary state only — no raw
/// content ever crosses this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSummary {
    /// Projection mode wire string.
    pub mode: String,
    /// Stable segment bytes.
    pub stable_bytes: usize,
    /// Contextual segment bytes.
    pub contextual_bytes: usize,
    /// Volatile segment bytes.
    pub volatile_bytes: usize,
    /// Estimated tokens.
    pub estimated_tokens: usize,
    /// Working maximum tokens (mirrors the pressure bound type).
    pub working_maximum: i64,
    /// Pressure state wire string.
    pub pressure_state: String,
    /// Pressure percent.
    pub pressure_percent: u64,
    /// Available tools in the projection.
    pub tools_available: usize,
    /// Gated tools in the projection.
    pub tools_gated: usize,
    /// Hidden tools in the projection.
    pub tools_hidden: usize,
}

/// Host state one feed resolution may read. All handles are
/// caller-owned; feeds borrow, never retain.
pub struct FeedContext<'a> {
    /// Canonical workspace root.
    pub workspace_root: &'a Path,
    /// Registry definitions snapshot in registration order.
    pub tools: &'a [RegisteredToolInfo],
    /// Effective permission policy.
    pub policy: &'a PermissionPolicy,
    /// Installed module records.
    pub modules: &'a [ModuleRecord],
    /// Latest context summary, when computed.
    pub context: Option<&'a ContextSummary>,
    /// Caller state directory for the checkpoint store. `None`
    /// refuses only the checkpoints feed; every other feed ignores
    /// it.
    pub checkpoint_state_dir: Option<&'a Path>,
    /// Unix milliseconds now (UTC) for the timestamp-bearing feeds.
    /// Callers supply the wall clock; tests and the corpus inject
    /// fixed values, so no feed owns a hidden nondeterministic
    /// clock. `None` renders the no-time form for those feeds.
    pub now_utc_ms: Option<u64>,
}

/// Why a feed refused to resolve. Typed and stable; paths are
/// workspace-relative, never absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// A param is missing, misshaped, oversized, or unknown.
    InvalidParams(String),
    /// The feed input was rejected by a containment gate.
    Denied(String),
}

impl FeedError {
    /// The exact externally observable message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::InvalidParams(reason) => {
                format!("Invalid feed params: {reason}")
            }
            Self::Denied(reason) => reason.clone(),
        }
    }
}

impl std::fmt::Display for FeedError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for FeedError {}

fn param<'a>(
    params: &'a BTreeMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    params.get(key).map(String::as_str)
}

fn reject_unknown_params(
    params: &BTreeMap<String, String>,
    known: &[&str],
    feed: &str,
) -> Result<(), FeedError> {
    for key in params.keys() {
        if !known.contains(&key.as_str()) {
            return Err(FeedError::InvalidParams(format!(
                "unknown param {key:?} for feed {feed}"
            )));
        }
    }
    Ok(())
}

fn resolve_list(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["path"], "workspace.list")?;
    let requested = param(params, "path").unwrap_or(".");
    match list_directory(context.workspace_root, requested, &WORKSPACE_LIMITS)
    {
        ListOutcome::Denied { message } => Err(FeedError::Denied(message)),
        ListOutcome::Failed { message } => Err(FeedError::Denied(message)),
        ListOutcome::Success { path, entries, truncated } => {
            let mut lines = vec![format!("{path}/")];
            for entry in entries {
                let kind = match entry.kind {
                    crate::workspace::list::EntryKind::Directory => "dir",
                    crate::workspace::list::EntryKind::File { .. } => "file",
                    crate::workspace::list::EntryKind::Symlink => "link",
                    crate::workspace::list::EntryKind::Other => "other",
                };
                lines.push(format!("{kind} {}", entry.name));
            }
            if truncated {
                lines.push("… (truncated)".to_owned());
            }
            Ok(lines)
        }
    }
}

fn resolve_search(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["query", "path"], "workspace.search")?;
    let query = param(params, "query").ok_or_else(|| {
        FeedError::InvalidParams("missing required param \"query\"".to_owned())
    })?;
    if query.is_empty() {
        return Err(FeedError::InvalidParams(
            "\"query\" must not be empty".to_owned(),
        ));
    }
    let input = SearchInput {
        query: query.to_owned(),
        path: param(params, "path").unwrap_or(".").to_owned(),
        max_results: WORKSPACE_LIMITS.max_search_matches,
    };
    match search(context.workspace_root, &input, &WORKSPACE_LIMITS, false) {
        SearchOutcome::InvalidInput { message }
        | SearchOutcome::Denied { message } => Err(FeedError::Denied(message)),
        SearchOutcome::Cancelled => {
            Err(FeedError::Denied("search was cancelled".to_owned()))
        }
        SearchOutcome::Success { matches, truncated, .. } => {
            let mut lines = Vec::new();
            for each in matches {
                lines.push(format!(
                    "{}:{}:{}: {}",
                    each.path, each.line, each.column, each.text
                ));
            }
            if truncated {
                lines.push("… (truncated)".to_owned());
            }
            Ok(lines)
        }
    }
}

fn resolve_context_summary(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    let Some(summary) = context.context else {
        return Ok(vec!["context: not computed".to_owned()]);
    };
    Ok(vec![
        format!("mode: {}", summary.mode),
        format!(
            "bytes: stable={} contextual={} volatile={}",
            summary.stable_bytes,
            summary.contextual_bytes,
            summary.volatile_bytes
        ),
        format!(
            "tokens: {} estimated / {} working",
            summary.estimated_tokens, summary.working_maximum
        ),
        format!(
            "pressure: {} ({}%)",
            summary.pressure_state, summary.pressure_percent
        ),
        format!(
            "tools: {} available, {} gated, {} hidden",
            summary.tools_available, summary.tools_gated, summary.tools_hidden
        ),
    ])
}

/// Display class of one catalog tool: mutation, execution, or
/// read-only. The fallback is intentional contract, not an
/// oversight: the capability vocabulary is open (`CapabilityId`
/// parses any well-formed id, including future ones), so an
/// unknown capability must render as the weakest class, never as
/// a stronger one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    Write,
    Execute,
    ReadOnly,
}

impl ToolKind {
    /// The exact display word used in catalog lines.
    fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Execute => "execute",
            Self::ReadOnly => "read-only",
        }
    }
}

fn tool_kind(capability: &siralos_core::tool::CapabilityId) -> ToolKind {
    match capability.as_str() {
        "workspace.write" => ToolKind::Write,
        "process.execute" => ToolKind::Execute,
        _ => ToolKind::ReadOnly,
    }
}

fn resolve_tools_catalog(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    if context.tools.is_empty() {
        return Ok(vec!["tools: none registered".to_owned()]);
    }
    let mut lines = Vec::new();
    for info in context.tools {
        let status =
            match evaluate_permission(&info.capability, context.policy) {
                PermissionDecision::Deny { .. } => "denied",
                PermissionDecision::Ask { .. } => "approval required",
                PermissionDecision::Allow => "allowed",
            };
        lines.push(format!(
            "{} ({}, {})",
            info.definition.name,
            tool_kind(&info.capability).as_str(),
            status
        ));
    }
    Ok(lines)
}

fn resolve_modules_status(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    if context.modules.is_empty() {
        return Ok(vec!["modules: none installed".to_owned()]);
    }
    let mut lines = Vec::new();
    for record in context.modules {
        let digest =
            record.digest.strip_prefix("sha256:").unwrap_or(&record.digest);
        lines.push(format!(
            "{} (digest {}, path {})",
            record.id,
            digest.chars().take(8).collect::<String>(),
            record.path
        ));
    }
    Ok(lines)
}

fn resolve_checkpoints_list(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["limit"], "checkpoints.list")?;
    let limit = match param(params, "limit") {
        None => MAX_FEED_CHECKPOINTS,
        Some(text) => text.parse::<usize>().map_err(|_| {
            FeedError::InvalidParams(
                "\"limit\" must be an integer 1..=100".to_owned(),
            )
        })?,
    };
    if !(1..=100).contains(&limit) {
        return Err(FeedError::InvalidParams(
            "\"limit\" must be an integer 1..=100".to_owned(),
        ));
    }
    let Some(state_dir) = context.checkpoint_state_dir else {
        return Err(FeedError::Denied(
            "checkpoint store unavailable".to_owned(),
        ));
    };
    let store = open_checkpoint_store(context.workspace_root, state_dir)
        .map_err(|_| {
            FeedError::Denied("checkpoint store unavailable".to_owned())
        })?;
    let listed = store.list(None, Some(limit));
    if listed.is_empty() {
        return Ok(vec!["checkpoints: none recorded".to_owned()]);
    }
    let mut lines = Vec::new();
    for checkpoint in listed {
        // Metadata only: id, path, creation, state, byte counts.
        // Preimage bytes and content hashes never cross this
        // boundary (adversarial-pinned below).
        let bytes = checkpoint.after.byte_length.unwrap_or(0);
        lines.push(format!(
            "{} {} {} {} {}B",
            checkpoint.id,
            checkpoint.relative_path,
            checkpoint.operation.as_str(),
            checkpoint.state.as_str(),
            bytes
        ));
    }
    Ok(lines)
}

/// Resolve one feed to raw content lines. Raw means unresolved
/// for display: the cli sanitizes every line before projection
/// or terminal output. Refusals are typed; a feed never renders.
pub fn resolve_feed(
    context: &FeedContext<'_>,
    feed: UiFeed,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    let lines = match feed {
        UiFeed::WorkspaceList => resolve_list(context, params)?,
        UiFeed::WorkspaceSearch => resolve_search(context, params)?,
        UiFeed::ContextSummary => {
            reject_unknown_params(params, &[], "context.summary")?;
            resolve_context_summary(context)?
        }
        UiFeed::ToolsCatalog => {
            reject_unknown_params(params, &[], "tools.catalog")?;
            resolve_tools_catalog(context)?
        }
        UiFeed::ModulesStatus => {
            reject_unknown_params(params, &[], "modules.status")?;
            resolve_modules_status(context)?
        }
        UiFeed::CheckpointsList => resolve_checkpoints_list(context, params)?,
        UiFeed::TimeWindow => resolve_time_window(context, params)?,
    };
    Ok(lines.into_iter().take(MAX_FEED_LINES).collect())
}

/// Maximum windows one `time.window` schedule declares.
pub const MAX_FEED_TIME_WINDOWS: usize = 8;
/// Maximum holidays one `time.window` schedule lists.
pub const MAX_FEED_HOLIDAYS: usize = 64;
/// Days the next-switch scan may look ahead before reporting no
/// boundary. Honest scan cap, not a guarantee beyond the schedule.
pub const MAX_FEED_TIME_SCAN_DAYS: i64 = 400;

const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// One parsed `time.window` schedule. Windows are minutes of day in
/// UTC, `[start, end)`, sorted and non-overlapping; `days` masks the
/// Mon-first weekdays on which the windows apply; holidays are civil
/// dates that force off-peak for the whole day.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeWindowSchedule {
    windows: Vec<(u16, u16)>,
    days: [bool; 7],
    holidays: Vec<(i64, u32, u32)>,
    label: Option<String>,
}

/// Howard Hinnant. Days since the Unix epoch to a civil `(y, m, d)`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}

/// Howard Hinnant. A civil `(y, m, d)` to days since the epoch.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let month = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * month + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Weekday position (Mon 0..Sun 6), refusing anything else.
fn weekday_position(day: &str, raw: &str) -> Result<usize, FeedError> {
    WEEKDAYS.iter().position(|name| *name == day).ok_or_else(|| {
        FeedError::InvalidParams(format!(
            "{raw:?} must be `all` or weekday names like `mon` or a `mon-fri` range"
        ))
    })
}

/// Parse `HH:MM` into minutes of day. Exactly five bytes, digits
/// with a middle colon, hours 00..=23, minutes 00..=59.
fn parse_hhmm(value: &str) -> Result<u16, FeedError> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Err(FeedError::InvalidParams(format!(
            "{value:?} must be a clock time `HH:MM`"
        )));
    }
    let hours: u16 = value[..2].parse().map_err(|_| {
        FeedError::InvalidParams(format!(
            "{value:?} must be a clock time `HH:MM`"
        ))
    })?;
    let minutes: u16 = value[3..].parse().map_err(|_| {
        FeedError::InvalidParams(format!(
            "{value:?} must be a clock time `HH:MM`"
        ))
    })?;
    if hours > 23 || minutes > 59 {
        return Err(FeedError::InvalidParams(format!(
            "{value:?} must be a clock time `HH:MM` within 00:00..=23:59"
        )));
    }
    Ok(hours * 60 + minutes)
}

/// Render minutes of day as `HH:MM`.
fn render_hhmm(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn parse_time_window_schedule(
    params: &BTreeMap<String, String>,
) -> Result<TimeWindowSchedule, FeedError> {
    reject_unknown_params(
        params,
        &["windows", "days", "holidays", "label"],
        "time.window",
    )?;
    let windows_text = param(params, "windows").ok_or_else(|| {
        FeedError::InvalidParams(
            "missing required param \"windows\"".to_owned(),
        )
    })?;
    if windows_text.is_empty() {
        return Err(FeedError::InvalidParams(
            "\"windows\" must name at least one `HH:MM-HH:MM` window"
                .to_owned(),
        ));
    }
    let mut windows = Vec::new();
    for entry in windows_text.split(',') {
        if windows.len() >= MAX_FEED_TIME_WINDOWS {
            return Err(FeedError::InvalidParams(format!(
                "\"windows\" carries more than {MAX_FEED_TIME_WINDOWS} windows"
            )));
        }
        let Some((start_text, end_text)) = entry.split_once('-') else {
            return Err(FeedError::InvalidParams(format!(
                "{entry:?} must be a `HH:MM-HH:MM` window"
            )));
        };
        let start = parse_hhmm(start_text)?;
        let end = parse_hhmm(end_text)?;
        if start >= end {
            return Err(FeedError::InvalidParams(format!(
                "{entry:?} must end after it starts (no midnight wrap)"
            )));
        }
        windows.push((start, end));
    }
    windows.sort_unstable();
    for pair in windows.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(FeedError::InvalidParams(
                "\"windows\" must not overlap".to_owned(),
            ));
        }
    }
    let days = match param(params, "days").unwrap_or("all") {
        "all" => [true; 7],
        text => {
            let mut days = [false; 7];
            match text.split_once('-') {
                Some((first, last)) => {
                    let mut index = weekday_position(first, text)?;
                    loop {
                        days[index] = true;
                        if index == weekday_position(last, text)? {
                            break;
                        }
                        index = (index + 1) % 7;
                    }
                }
                None => {
                    days[weekday_position(text, text)?] = true;
                }
            }
            days
        }
    };
    let mut holidays = Vec::new();
    if let Some(text) = param(params, "holidays") {
        if !text.is_empty() {
            for date in text.split(',') {
                if holidays.len() >= MAX_FEED_HOLIDAYS {
                    return Err(FeedError::InvalidParams(format!(
                        "\"holidays\" carries more than {MAX_FEED_HOLIDAYS} dates"
                    )));
                }
                let bytes = date.as_bytes();
                if bytes.len() != 10
                    || (bytes[4], bytes[7]) != (b'-', b'-')
                    || !bytes.iter().enumerate().all(|(index, byte)| {
                        index == 4 || index == 7 || byte.is_ascii_digit()
                    })
                {
                    return Err(FeedError::InvalidParams(format!(
                        "{date:?} must be a calendar date `YYYY-MM-DD`"
                    )));
                }
                let year: i64 = date[..4].parse().map_err(|_| {
                    FeedError::InvalidParams(format!(
                        "{date:?} must be a calendar date `YYYY-MM-DD`"
                    ))
                })?;
                let month: u32 = date[5..7].parse().map_err(|_| {
                    FeedError::InvalidParams(format!(
                        "{date:?} must be a calendar date `YYYY-MM-DD`"
                    ))
                })?;
                let day: u32 = date[8..10].parse().map_err(|_| {
                    FeedError::InvalidParams(format!(
                        "{date:?} must be a calendar date `YYYY-MM-DD`"
                    ))
                })?;
                let valid = (1..=12).contains(&month)
                    && (1..=9999).contains(&year)
                    && days_from_civil(year, month, day) >= 0
                    && civil_from_days(days_from_civil(year, month, day))
                        == (year, month, day);
                if !valid {
                    return Err(FeedError::InvalidParams(format!(
                        "{date:?} must be a calendar date `YYYY-MM-DD`"
                    )));
                }
                holidays.push((year, month, day));
            }
        }
    }
    let label = match param(params, "label") {
        Some(text) if text.chars().count() > 256 => {
            return Err(FeedError::InvalidParams(
                "\"label\" must fit 256 characters".to_owned(),
            ));
        }
        Some(text) => Some(text.to_owned()),
        None => None,
    };
    Ok(TimeWindowSchedule { windows, days, holidays, label })
}

/// Weekday index (Mon 0..Sun 6) of epoch `days`.
fn weekday_index(days: i64) -> usize {
    usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// The countdown wording: `Xh Ym` under a day, `Xd Yh` at or over.
fn render_countdown(delta_minutes: i64) -> String {
    if delta_minutes < 1440 {
        format!("{}h {}m", delta_minutes / 60, delta_minutes % 60)
    } else {
        format!("{}d {}h", delta_minutes / 1440, (delta_minutes % 1440) / 60)
    }
}

/// Render the schedule line the module's params describe.
fn render_time_window_schedule(schedule: &TimeWindowSchedule) -> String {
    let windows: Vec<String> = schedule
        .windows
        .iter()
        .map(|(start, end)| {
            format!("{}-{}", render_hhmm(*start), render_hhmm(*end))
        })
        .collect();
    let days_wire = if schedule.days.iter().all(|day| *day) {
        "all".to_owned()
    } else {
        let first = schedule.days.iter().position(|day| *day).unwrap_or(0);
        let last = schedule.days.iter().rposition(|day| *day).unwrap_or(0);
        format!("{}-{}", WEEKDAYS[first], WEEKDAYS[last])
    };
    let label = schedule
        .label
        .as_deref()
        .map(|label| format!(" ({label})"))
        .unwrap_or_default();
    format!("peak {} UTC {}{}", windows.join(" & "), days_wire, label)
}

/// The next instant the tier changes to "peak", strictly after
/// `now`: the first window start on a masked, non-holiday day from
/// `days` on. `(day, start)` names the boundary instant.
fn next_time_window_boundary(
    schedule: &TimeWindowSchedule,
    days: i64,
    minutes: u16,
) -> Option<(i64, u16)> {
    for offset in 0..=MAX_FEED_TIME_SCAN_DAYS {
        let day = days + offset;
        if !schedule.days[weekday_index(day)] {
            continue;
        }
        let (y, m, d) = civil_from_days(day);
        if schedule.holidays.contains(&(y, m, d)) {
            continue;
        }
        for (start, _) in &schedule.windows {
            if offset == 0 && *start <= minutes {
                // Strictly after now on the current day.
                continue;
            }
            return Some((day, *start));
        }
    }
    None
}

fn resolve_time_window(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    let schedule = parse_time_window_schedule(params)?;
    let Some(now_utc_ms) = context.now_utc_ms else {
        return Ok(vec![
            "now: no wall clock".to_owned(),
            render_time_window_schedule(&schedule),
            "switch: no wall clock".to_owned(),
        ]);
    };
    let secs = now_utc_ms / 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let minutes = u16::try_from((secs % 86_400) / 60).unwrap_or(0);
    let weekday = weekday_index(days);
    let (y, m, d) = civil_from_days(days);
    let holiday = schedule.holidays.contains(&(y, m, d));
    let in_window = schedule
        .windows
        .iter()
        .any(|(start, end)| minutes >= *start && minutes < *end);
    let peak = schedule.days[weekday] && !holiday && in_window;
    let mut lines = vec![
        format!(
            "now {y:04}-{m:02}-{d:02} {} UTC ({})",
            render_hhmm(minutes),
            if peak { "peak" } else { "off-peak" }
        ),
        render_time_window_schedule(&schedule),
    ];
    // The next boundary saturates the countdown honestly: while the
    // applied window contains now the tier flips at that window's
    // end; otherwise at the next qualifying day's window start.
    let window_end = if peak {
        schedule
            .windows
            .iter()
            .find(|(start, end)| minutes >= *start && minutes < *end)
            .map(|(_, end)| *end)
    } else {
        None
    };
    match match window_end {
        Some(end) => Some((days, end)),
        None => next_time_window_boundary(&schedule, days, minutes),
    } {
        Some((day, boundary)) => {
            let (by, bm, bd) = civil_from_days(day);
            let delta =
                (day - days) * 1440 + i64::from(boundary) - i64::from(minutes);
            lines.push(format!(
                "switch in {} ({by:04}-{bm:02}-{bd:02} {} UTC)",
                render_countdown(delta),
                render_hhmm(boundary)
            ));
        }
        None => {
            lines.push("switch: none within 400 days".to_owned());
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use siralos_core::provider::{CancellationToken, ToolDefinition};
    use siralos_core::tool::{
        CapabilityId, PermissionPolicy, PermissionRule, PolicyRule,
        RegisteredToolInfo, Tool,
    };
    use siralos_core::ui::UiFeed;

    use super::{
        ContextSummary, FeedContext, FeedError, MAX_FEED_CHECKPOINTS,
        MAX_FEED_HOLIDAYS, ToolKind, civil_from_days, days_from_civil,
        feed_capability, resolve_feed, tool_kind, weekday_index,
    };
    use crate::module::manifest::ModuleRecord;
    use crate::tool::WriteTool;
    use crate::workspace::checkpoint::open_checkpoint_store;

    struct Fixture {
        root: std::path::PathBuf,
        store_root: std::path::PathBuf,
        state_dir: std::path::PathBuf,
        policy: PermissionPolicy,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let tag = format!(
                "{}-{}-{}",
                std::process::id(),
                label,
                // Monotonic per process without a global: micros
                // collide only under same-microsecond spawns, and
                // the tag also carries the label.
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_micros())
                    .unwrap_or(0)
            );
            let root =
                std::env::temp_dir().join(format!("siralos-ui-ws-{tag}"));
            let store_root =
                std::env::temp_dir().join(format!("siralos-ui-st-{tag}"));
            let state_dir =
                std::env::temp_dir().join(format!("siralos-ui-cp-{tag}"));
            std::fs::create_dir_all(&root).expect("fixture root");
            let policy = PermissionPolicy::from_rules([]);
            Self { root, store_root, state_dir, policy }
        }

        fn context(&self) -> FeedContext<'_> {
            FeedContext {
                workspace_root: &self.root,
                tools: &[],
                policy: &self.policy,
                modules: &[],
                context: None,
                checkpoint_state_dir: Some(&self.state_dir),
                now_utc_ms: None,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
            let _ = std::fs::remove_dir_all(&self.store_root);
            let _ = std::fs::remove_dir_all(&self.state_dir);
        }
    }

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn list_feed_lists_sorted_entries() {
        let fixture = Fixture::new("list");
        std::fs::write(fixture.root.join("b.txt"), b"b").expect("seed");
        std::fs::write(fixture.root.join("a.txt"), b"a").expect("seed");
        let context = fixture.context();
        let lines =
            resolve_feed(&context, UiFeed::WorkspaceList, &params(&[]))
                .expect("lists");
        assert_eq!(
            lines,
            vec![
                "./".to_owned(),
                "file a.txt".to_owned(),
                "file b.txt".to_owned()
            ]
        );
    }

    #[test]
    fn list_feed_refuses_traversal_and_protected_paths() {
        let fixture = Fixture::new("list-deny");
        let context = fixture.context();
        for path in ["../escape.txt", ".siralos/notes.txt"] {
            assert!(
                matches!(
                    resolve_feed(
                        &context,
                        UiFeed::WorkspaceList,
                        &params(&[("path", path)])
                    ),
                    Err(FeedError::Denied(_))
                ),
                "{path}"
            );
        }
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::WorkspaceList,
                &params(&[("bogus", "x")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
    }

    #[test]
    fn search_feed_requires_a_query_and_finds_needles() {
        let fixture = Fixture::new("search");
        std::fs::write(fixture.root.join("hay.txt"), b"needle here")
            .expect("seed");
        let context = fixture.context();
        assert!(matches!(
            resolve_feed(&context, UiFeed::WorkspaceSearch, &params(&[])),
            Err(FeedError::InvalidParams(_))
        ));
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::WorkspaceSearch,
                &params(&[("query", "needle"), ("bogus", "x")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        let lines = resolve_feed(
            &context,
            UiFeed::WorkspaceSearch,
            &params(&[("query", "needle")]),
        )
        .expect("searches");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("hay.txt:1:1: "), "{lines:?}");
        assert!(lines[0].contains("needle here"));
    }

    #[test]
    fn paramless_feeds_reject_params() {
        let fixture = Fixture::new("no-params");
        let context = fixture.context();
        for feed in [
            UiFeed::ContextSummary,
            UiFeed::ToolsCatalog,
            UiFeed::ModulesStatus,
        ] {
            assert!(
                matches!(
                    resolve_feed(&context, feed, &params(&[("bogus", "x")])),
                    Err(FeedError::InvalidParams(_))
                ),
                "{feed:?}"
            );
        }
    }

    #[test]
    fn context_summary_renders_numbers_without_content() {
        let summary = ContextSummary {
            mode: "generic".to_owned(),
            stable_bytes: 100,
            contextual_bytes: 200,
            volatile_bytes: 50,
            estimated_tokens: 300,
            working_maximum: 4096,
            pressure_state: "normal".to_owned(),
            pressure_percent: 8,
            tools_available: 4,
            tools_gated: 2,
            tools_hidden: 0,
        };
        let fixture = Fixture::new("summary");
        let context =
            FeedContext { context: Some(&summary), ..fixture.context() };
        let lines =
            resolve_feed(&context, UiFeed::ContextSummary, &params(&[]))
                .expect("summarizes");
        let text = lines.join("\n");
        assert!(text.contains("mode: generic"), "{text:?}");
        assert!(text.contains("8%"), "{text:?}");
        assert!(text.contains("4 available, 2 gated, 0 hidden"), "{text:?}");
        assert!(context.context.is_some());
    }

    fn registered(name: &str, capability: &str) -> RegisteredToolInfo {
        RegisteredToolInfo {
            definition: ToolDefinition {
                name: name.to_owned(),
                description: format!("{name} tool"),
                input_schema: serde_json::json!({"type": "object"}),
            },
            capability: CapabilityId::parse(capability)
                .expect("test capability parses"),
        }
    }

    fn allow_all() -> PermissionPolicy {
        PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("workspace.read")
                    .expect("parses"),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("workspace.write")
                    .expect("parses"),
                rule: PermissionRule::Ask,
            },
        ])
    }

    #[test]
    fn tools_catalog_lists_registered_tools() {
        let tools = [
            registered("workspace.read", "workspace.read"),
            registered("write", "workspace.write"),
        ];
        let policy = allow_all();
        let fixture = Fixture::new("catalog");
        let context = FeedContext {
            tools: &tools,
            policy: &policy,
            ..fixture.context()
        };
        let lines = resolve_feed(&context, UiFeed::ToolsCatalog, &params(&[]))
            .expect("catalogs");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("workspace.read"), "{lines:?}");
        assert!(lines[0].contains("allowed"), "{lines:?}");
        assert!(lines[1].contains("approval required"), "{lines:?}");
    }

    #[test]
    fn tool_kind_maps_the_closed_vocabulary() {
        let kind =
            |id: &str| tool_kind(&CapabilityId::parse(id).expect("parses"));
        assert_eq!(kind("workspace.write"), ToolKind::Write);
        assert_eq!(kind("process.execute"), ToolKind::Execute);
        // Every other capability renders as read-only, including
        // future ids: unknown capabilities must never display as a
        // stronger class.
        for id in [
            "workspace.read",
            "git.inspect",
            "network.outbound",
            "reference.inspect",
            "research.fetch",
            "self.inspect",
            "context.read",
            "future.domain",
        ] {
            assert_eq!(kind(id), ToolKind::ReadOnly, "{id}");
        }
        assert_eq!(ToolKind::Write.as_str(), "write");
        assert_eq!(ToolKind::Execute.as_str(), "execute");
        assert_eq!(ToolKind::ReadOnly.as_str(), "read-only");
    }

    #[test]
    fn modules_status_lists_install_records() {
        let modules = [ModuleRecord {
            id: "demo".to_owned(),
            path: "modules/demo".to_owned(),
            digest: format!("sha256:{}", "a".repeat(64)),
        }];
        let fixture = Fixture::new("modules");
        let context = FeedContext { modules: &modules, ..fixture.context() };
        let lines =
            resolve_feed(&context, UiFeed::ModulesStatus, &params(&[]))
                .expect("lists modules");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("demo"), "{lines:?}");
        assert!(lines[0].contains("modules/demo"), "{lines:?}");
    }

    #[test]
    fn checkpoints_feed_excludes_preimage_bytes() {
        let fixture = Fixture::new("checkpoints");
        // The feed opens its store under the same state dir the
        // tools checkpoint into: one root, one store view.
        let store = open_checkpoint_store(&fixture.root, &fixture.state_dir)
            .expect("store opens");
        let write = WriteTool::new(&fixture.root, store).expect("tool");
        let token = CancellationToken::new();
        let marker = "S3CR3T-PREIMAGE-BYTES-never-rendered";
        let outcome = write.execute(
            &serde_json::json!({
                "path": "secret.txt",
                "content": marker,
            }),
            token.signal(),
        );
        assert!(
            matches!(
                outcome,
                siralos_core::provider::ToolExecutionResult::Success { .. }
            ),
            "{outcome:?}"
        );
        let context = fixture.context();
        let lines =
            resolve_feed(&context, UiFeed::CheckpointsList, &params(&[]))
                .expect("lists checkpoints");
        assert_eq!(lines.len(), 1, "{lines:?}");
        let text = lines.join("\n");
        assert!(!text.contains(marker), "preimage bytes leaked: {text:?}");
        assert!(!text.contains(&"4".repeat(64)), "hash leaked: {text:?}");
        assert!(text.contains("secret.txt"), "{text:?}");
        assert!(text.contains("create"), "{text:?}");
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::CheckpointsList,
                &params(&[("limit", "0")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::CheckpointsList,
                &params(&[("limit", "many")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        assert_eq!(MAX_FEED_CHECKPOINTS, 20);
    }

    /// Unix milliseconds for `HH:MM` on the epoch day `days`.
    fn clock_millis(days: i64, hours: u32, minutes: u32) -> u64 {
        u64::try_from(
            (days * 86_400)
                + i64::from(hours) * 3600
                + i64::from(minutes) * 60,
        )
        .expect("positive clock")
            * 1000
    }

    #[test]
    fn time_window_resolves_the_deepseek_schedule() {
        let fixture = Fixture::new("time-window");
        let days = days_from_civil(2026, 10, 20);
        assert_eq!(weekday_index(days), 1, "2026-10-20 is a Tuesday");
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(days, 2, 23)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
            ]),
        )
        .expect("resolves");
        assert_eq!(lines[0], "now 2026-10-20 02:23 UTC (peak)");
        assert_eq!(lines[1], "peak 01:00-04:00 & 06:00-10:00 UTC mon-fri");
        assert_eq!(lines[2], "switch in 1h 37m (2026-10-20 04:00 UTC)");
    }

    #[test]
    fn time_window_counts_to_the_next_window_across_days_and_weekends() {
        let fixture = Fixture::new("time-window-span");
        // Wednesday late: the next window is Thursday 01:00.
        let wed = days_from_civil(2026, 10, 21);
        assert_eq!(weekday_index(wed), 2, "2026-10-21 is a Wednesday");
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(wed, 18, 23)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
            ]),
        )
        .expect("Wednesday resolves");
        assert_eq!(lines[0], "now 2026-10-21 18:23 UTC (off-peak)");
        assert_eq!(lines[2], "switch in 6h 37m (2026-10-22 01:00 UTC)");
        // Saturday is off-peak all day: the switch is Monday 01:00.
        let sat = days_from_civil(2026, 10, 24);
        assert_eq!(weekday_index(sat), 5, "2026-10-24 is a Saturday");
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(sat, 18, 23)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
            ]),
        )
        .expect("Saturday resolves");
        assert_eq!(lines[0], "now 2026-10-24 18:23 UTC (off-peak)");
        assert_eq!(lines[2], "switch in 1d 6h (2026-10-26 01:00 UTC)");
    }

    #[test]
    fn time_window_treats_holidays_as_off_peak_and_renders_the_label() {
        let fixture = Fixture::new("time-window-holiday");
        // A Chinese public holiday: off-peak all day, windows resume
        // the next qualifying day.
        let tuesday = days_from_civil(2026, 10, 6);
        assert_eq!(weekday_index(tuesday), 1, "2026-10-06 is a Tuesday");
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(tuesday, 2, 23)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
                ("holidays", "2026-10-06"),
                ("label", "DeepSeek peak"),
            ]),
        )
        .expect("holiday resolves");
        assert_eq!(lines[0], "now 2026-10-06 02:23 UTC (off-peak)");
        assert_eq!(
            lines[1],
            "peak 01:00-04:00 & 06:00-10:00 UTC mon-fri (DeepSeek peak)"
        );
        assert_eq!(lines[2], "switch in 22h 37m (2026-10-07 01:00 UTC)");
    }

    #[test]
    fn time_window_boundaries_are_exact() {
        let fixture = Fixture::new("time-window-edge");
        let days = days_from_civil(2026, 10, 20);
        // Exactly at a window start: peak, counting to that window's
        // end.
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(days, 1, 0)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
            ]),
        )
        .expect("start boundary resolves");
        assert_eq!(lines[0], "now 2026-10-20 01:00 UTC (peak)");
        assert_eq!(lines[2], "switch in 3h 0m (2026-10-20 04:00 UTC)");
        // Exactly at a window end: off-peak from that minute on
        // (windows are end-exclusive), counting to the next start.
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(days, 4, 0)),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00,06:00-10:00"),
                ("days", "mon-fri"),
            ]),
        )
        .expect("end boundary resolves");
        assert_eq!(lines[0], "now 2026-10-20 04:00 UTC (off-peak)");
        assert_eq!(lines[2], "switch in 2h 0m (2026-10-20 06:00 UTC)");
    }

    #[test]
    fn time_window_refuses_malformed_params() {
        let fixture = Fixture::new("time-window-bad");
        let context = fixture.context();
        let schedules: &[&[(&str, &str)]] = &[
            &[],
            &[("windows", "")],
            &[("windows", "0100-0400")],
            &[("windows", "04:00-01:00")],
            &[("windows", "01:00-04:00,03:00-05:00")],
            &[("windows", "00:00-24:00")],
            &[("windows", "01:00-04:00"), ("days", "Mon-fri")],
            &[("windows", "01:00-04:00"), ("days", "monday")],
            &[("windows", "01:00-04:00"), ("days", "mon-fri extras")],
            &[("windows", "01:00-04:00"), ("holidays", "2026-02-30")],
            &[("windows", "01:00-04:00"), ("holidays", "2026-1-1")],
            &[("windows", "01:00-04:00"), ("nada", "x")],
        ];
        for schedule in schedules {
            assert!(
                matches!(
                    resolve_feed(
                        &context,
                        UiFeed::TimeWindow,
                        &params(schedule)
                    ),
                    Err(FeedError::InvalidParams(_))
                ),
                "{schedule:?}"
            );
        }
    }

    #[test]
    fn time_window_without_a_wall_clock_is_honest() {
        let fixture = Fixture::new("time-window-clock");
        let context = fixture.context();
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[("windows", "01:00-04:00")]),
        )
        .expect("clockless resolves");
        assert_eq!(lines[0], "now: no wall clock");
        assert_eq!(lines[1], "peak 01:00-04:00 UTC all");
        assert_eq!(lines[2], "switch: no wall clock");
    }

    #[test]
    fn time_window_feed_is_open_to_every_spec() {
        assert_eq!(feed_capability(UiFeed::TimeWindow), None);
    }

    #[test]
    fn time_window_reports_no_boundary_when_holidays_cover_the_scan() {
        let fixture = Fixture::new("time-window-none");
        // Mondays off, every Monday within the scan window a
        // holiday: the countdown says so rather than fabricating a
        // switch.
        let mut holidays = String::new();
        for index in 0..MAX_FEED_HOLIDAYS {
            let day = days_from_civil(2026, 10, 26) + (index as i64) * 7;
            let (y, m, d) = civil_from_days(day);
            holidays.push_str(&format!("{y:04}-{m:02}-{d:02},"));
        }
        let holidays = holidays.trim_end_matches(',');
        let context = FeedContext {
            now_utc_ms: Some(clock_millis(
                days_from_civil(2026, 10, 25),
                23,
                0,
            )),
            ..fixture.context()
        };
        let lines = resolve_feed(
            &context,
            UiFeed::TimeWindow,
            &params(&[
                ("windows", "01:00-04:00"),
                ("days", "mon"),
                ("holidays", holidays),
            ]),
        )
        .expect("exhausted scan resolves");
        assert_eq!(lines[2], "switch: none within 400 days");
    }
}
