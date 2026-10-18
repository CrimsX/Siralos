//! Bottom-row composition, session usage metrics, and timestamps.

use super::{
    CONTEXT_BUDGET_TOKENS, Color, Duration, Line, Span, Style, display_width,
    safe_display_label, truncate_to_width,
};

/// Timestamp helpers (H4 — local timezone via `time` crate).
///
/// `local_timestamp_now` uses `time::OffsetDateTime::now_local()` with the
/// `local-offset` feature. When the platform cannot determine the local
/// offset, `now_local` returns `None` and we fall back to UTC. The formatted
/// shape is `"YYYY-MM-DD HH:MM:SS +HH:MM"` (local) or `"YYYY-MM-DD HH:MM:SS UTC"`
/// (fallback). `utc_timestamp_from_millis` is retained for tests and the
/// fallback path (civil-from-days math, Howard Hinnant).
#[must_use]
pub fn utc_timestamp_from_millis(millis: u64) -> String {
    let secs = millis / 1000;
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
    )
}

/// Civil date from days since Unix epoch (1970-01-01) — Hinnant.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    // Shift to civil epoch (0000-03-01)
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0,399]
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1,12]
    y += i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

/// Session usage observation for the bottom row (display only).
///
/// Every field is host-observed: tokens and cached-token counts as the
/// provider reported them, `provider_millis` as the accumulated wall time of
/// provider rounds, `context_assembled` from the same `ContextMetrics` the
/// pane and `/context` audit use. Defaults mean "not observed yet", and the
/// row renders nothing for an absent value rather than a fabricated number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMetrics {
    /// Input tokens reported by the provider, session total.
    pub input_tokens: u64,
    /// Output tokens reported by the provider, session total.
    pub output_tokens: u64,
    /// Cached input tokens when the provider reports them.
    pub cached_tokens: Option<u64>,
    /// Accumulated provider round time, milliseconds.
    pub provider_millis: u64,
    /// Assembled unique-digest total from the context metrics, `None` when
    /// the context subsystem is off.
    pub context_assembled: Option<usize>,
}

impl SessionMetrics {
    /// Session-average output tokens per second. `None` until both a
    /// reported output count and a measured round time exist.
    #[must_use]
    pub fn tokens_per_second(&self) -> Option<f64> {
        if self.output_tokens == 0 || self.provider_millis == 0 {
            return None;
        }
        Some(
            self.output_tokens as f64 / (self.provider_millis as f64 / 1000.0),
        )
    }

    /// `input/output` token segment, compacted to `k`/`M` suffixes.
    #[must_use]
    pub fn tokens_segment(&self) -> Option<String> {
        if self.input_tokens == 0 && self.output_tokens == 0 {
            return None;
        }
        Some(format!(
            "{}/{}",
            format_token_count(self.input_tokens),
            format_token_count(self.output_tokens)
        ))
    }

    /// Integer cache-hit percentage of reported input tokens. `None` when
    /// the provider does not report cached tokens.
    #[must_use]
    pub fn cache_percent(&self) -> Option<u64> {
        let cached = self.cached_tokens?;
        if self.input_tokens == 0 {
            return None;
        }
        Some((cached.saturating_mul(100) / self.input_tokens).min(100))
    }

    /// Integer context-usage percentage of the fixed budget. `None` when
    /// the context subsystem is off.
    #[must_use]
    pub fn context_percent(&self) -> Option<u64> {
        let assembled = self.context_assembled?;
        Some((assembled.saturating_mul(100) / CONTEXT_BUDGET_TOKENS).min(100)
            as u64)
    }

    /// Add one observed provider round to the session totals.
    ///
    /// Only fields the provider actually reported are added: an absent
    /// token count adds nothing (never a fabricated zero), an absent
    /// cached count leaves the session cached total absent, and an absent
    /// round time adds no milliseconds. Totals saturate.
    pub fn accumulate_round(
        &mut self,
        round_millis: Option<u64>,
        usage: &siralos_core::determinism::provider_replay::ProviderUsageObservation,
    ) {
        if let Some(millis) = round_millis {
            self.provider_millis = self.provider_millis.saturating_add(millis);
        }
        self.input_tokens =
            self.input_tokens.saturating_add(usage.input_tokens.unwrap_or(0));
        self.output_tokens = self
            .output_tokens
            .saturating_add(usage.output_tokens.unwrap_or(0));
        if let Some(cached) = usage.cached_tokens {
            self.cached_tokens =
                Some(self.cached_tokens.unwrap_or(0).saturating_add(cached));
        }
    }
}

/// Compact a token count for the metrics row (`1234` -> `1.2k`).
#[must_use]
fn format_token_count(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

/// Push the status text into `spans`, styling a `working` word (any case)
/// yellow bold — the frozen-state marker stays unmistakable — and the rest
/// cyan.
fn push_status_spans(spans: &mut Vec<Span<'static>>, status: &str) {
    let lower = status.to_ascii_lowercase();
    if let Some(pos) = lower.find("working") {
        let end = pos + "working".len();
        if pos > 0 {
            spans.push(Span::styled(
                status[..pos].to_owned(),
                Style::default().fg(Color::Cyan),
            ));
        }
        spans.push(Span::styled(
            status[pos..end].to_owned(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
        ));
        if end < status.len() {
            spans.push(Span::styled(
                status[end..].to_owned(),
                Style::default().fg(Color::Cyan),
            ));
        }
    } else {
        spans.push(Span::styled(
            status.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }
}

/// Fit the status text to `budget` cells: whole when it fits, otherwise
/// truncated on a scalar boundary with a `…` marker.
#[must_use]
fn fit_status_to_budget(status: &str, budget: usize) -> String {
    if display_width(status) <= budget {
        return status.to_owned();
    }
    if budget == 0 {
        return String::new();
    }
    format!("{}…", truncate_to_width(status, budget.saturating_sub(1)))
}

/// Compose the single bottom row: status at the left, usage metrics in the
/// middle, provider/model flush right.
///
/// The row is `status | tps … · tok in/out · cache % · ctx % | provider / model`.
/// When the frame is too narrow the metrics segments drop in order
/// `tps` → `tok` → `cache` (`ctx` is kept while it exists) so the whole
/// status survives; only when even the surviving middle leaves no room
/// does the status truncate with `…`. The provider/model block is never
/// dropped except on absurdly narrow frames, where it is truncated in
/// place.
#[must_use]
pub fn compose_bottom_row(
    width: u16,
    status: &str,
    provider: Option<&str>,
    model: Option<&str>,
    metrics: &SessionMetrics,
) -> Line<'static> {
    let width = width as usize;
    let right = match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => {
            let sp = safe_display_label(p);
            let sm = safe_display_label(m);
            format!("{sp} / {sm}")
        }
        (Some(p), _) if !p.is_empty() => safe_display_label(p),
        _ => "no provider configured".to_owned(),
    };
    let right_len = display_width(&right);
    if width <= right_len + 3 {
        return Line::from(Span::styled(
            truncate_to_width(&right, width),
            Style::default().fg(Color::Green),
        ));
    }
    // Droppable metric segments, in drop order; `ctx` sits outside them.
    let mut droppable: Vec<String> = Vec::new();
    if let Some(tps) = metrics.tokens_per_second() {
        droppable.push(format!("tps {tps:.1}"));
    }
    if let Some(tokens) = metrics.tokens_segment() {
        droppable.push(format!("tok {tokens}"));
    }
    if let Some(cache) = metrics.cache_percent() {
        droppable.push(format!("cache {cache}%"));
    }
    let ctx = metrics.context_percent().map(|pct| format!("ctx {pct}%"));
    let status_len = display_width(status);
    let mid = loop {
        let mut segments = droppable.clone();
        if let Some(ctx) = &ctx {
            segments.push(ctx.clone());
        }
        let mid = segments.join(" · ");
        let mid_len = display_width(&mid);
        let fixed = if mid.is_empty() {
            3 + right_len
        } else {
            3 + mid_len + 3 + right_len
        };
        // Room for the whole status wins over decorative metrics: keep
        // dropping until the row can hold status + gap + middle + right.
        if width >= fixed + 1 + status_len || droppable.is_empty() {
            break mid;
        }
        droppable.remove(0);
    };
    let mid_len = display_width(&mid);
    // Each present block is preceded by `" | "`; the gap pads only between
    // the middle block and the right block, so provider/model stays flush
    // right. An absent middle prints no separators of its own.
    let fixed = if mid.is_empty() {
        3 + right_len
    } else {
        mid_len + 3 + 3 + right_len
    };
    let left_budget = width.saturating_sub(fixed + 1);
    let left = fit_status_to_budget(status, left_budget);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    if !left.is_empty() {
        push_status_spans(&mut spans, &left);
        used += display_width(&left);
    }
    if !mid.is_empty() {
        if used > 0 {
            spans.push(Span::styled(
                " | ",
                Style::default().fg(Color::DarkGray),
            ));
            used += 3;
        }
        spans.push(Span::styled(
            mid.clone(),
            Style::default().fg(Color::DarkGray),
        ));
        used += mid_len;
    }
    let trailing_sep = if used > 0 { 3 } else { 0 };
    let gap = width.saturating_sub(used + trailing_sep + right_len).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    if used > 0 {
        spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
    }
    spans.push(Span::styled(right, Style::default().fg(Color::Green)));
    Line::from(spans)
}

/// How long each `working` dot phase lasts.
pub const WORKING_PULSE: Duration = Duration::from_secs(1);

/// The liveness line rendered directly ABOVE the input (owner QoL
/// 2026-09-12): the working state moved out of the status row and into the
/// conversation flow, with dots that pulse once a second.
///
/// Pure in `elapsed`, so the animation is testable without a clock: the
/// render path passes how long the current turn has been running.
#[must_use]
pub fn working_line(elapsed: Duration) -> String {
    let phase = (elapsed.as_millis() / WORKING_PULSE.as_millis().max(1)) % 3;
    let dots = ".".repeat(1 + phase as usize);
    format!("working{dots}")
}

/// Assembled unique-digest total from the same `ContextMetrics` the pane uses (P5 — single source).
#[must_use]
pub fn context_assembled_total(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> usize {
    metrics.records().last().map(|r| r.assembled_unique_total).unwrap_or(0)
}

/// Returns true when the base status indicates a working/loading state (H5).
/// The synchronous architecture freezes redraw during blocking dispatch, so the
/// working marker is styled distinctly to make the frozen state unmistakable.
#[must_use]
pub fn is_working_status(base_status: &str) -> bool {
    let lower = base_status.to_ascii_lowercase();
    lower.contains("working")
}

/// Current local timestamp for live sessions (H4) — `time` crate `local-offset`.
/// Falls back to UTC when the platform cannot determine the local offset.
#[must_use]
pub fn local_timestamp_now() -> String {
    // Use `time` crate's `OffsetDateTime::now_local()` when available.
    // `now_local` attempts to read the system's local offset; on platforms
    // where the tz database is unavailable it returns an error.
    if let Ok(dt) = time::OffsetDateTime::now_local() {
        #[allow(deprecated)]
        let desc = time::format_description::parse(
            "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory]:[offset_minute]",
        )
        .expect("valid format");
        if let Ok(fmt) = dt.format(&desc) {
            return fmt;
        }
        // Fallback to manual formatting if `format` fails.
        let offset = dt.offset();
        let total_min = offset.whole_seconds() / 60;
        let sign = if total_min >= 0 { '+' } else { '-' };
        let abs = total_min.unsigned_abs();
        let oh = abs / 60;
        let om = abs % 60;
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}{:02}:{:02}",
            dt.year(),
            dt.month() as u8,
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
            sign,
            oh,
            om
        );
    }
    // Fallback to UTC when local offset cannot be determined.
    utc_timestamp_now_fallback()
}

/// UTC fallback when `now_local` fails — std-only civil math, labeled `UTC`.
#[must_use]
fn utc_timestamp_now_fallback() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    utc_timestamp_from_millis(millis)
}

/// Backward-compat alias (deprecated): use `local_timestamp_now`.
#[must_use]
pub fn utc_timestamp_now() -> String {
    local_timestamp_now()
}

/// Local timestamp from millis with the given offset (for tests).
#[must_use]
pub fn local_timestamp_from_millis_with_offset(
    millis: u64,
    offset_minutes: i32,
) -> String {
    let secs = millis / 1000;
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let abs = offset_minutes.unsigned_abs();
    let oh = abs / 60;
    let om = abs % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {sign}{oh:02}:{om:02}"
    )
}
