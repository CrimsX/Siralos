//! Status line, header, and timestamp composition for the frame.

use super::{CONTEXT_BUDGET_TOKENS, Duration, safe_display_label};

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

/// Compose the status line prefix from the composed profile (I5).
#[must_use]
pub fn compose_status_line(
    base_status: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> String {
    let prefix = match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => {
            let sp = safe_display_label(p);
            let sm = safe_display_label(m);
            format!("{sp} / {sm}")
        }
        (Some(p), _) if !p.is_empty() => safe_display_label(p),
        _ => "no provider configured".to_owned(),
    };
    if base_status.is_empty() {
        prefix
    } else {
        format!("{prefix} | {base_status}")
    }
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

/// Append the context-usage readout `ctx <assembled>/4096` when opted-in AND built (P5).
/// When `metrics` is `None`, returns `status` unchanged (OFF → byte-identical).
#[must_use]
pub fn append_context_usage(
    status: String,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
) -> String {
    if let Some(m) = metrics {
        let assembled = context_assembled_total(m);
        format!("{status} | ctx {assembled}/{CONTEXT_BUDGET_TOKENS}")
    } else {
        status
    }
}

/// Compose the status line with optional context-usage appended (P5).
#[must_use]
pub fn compose_status_line_with_context(
    base_status: &str,
    provider: Option<&str>,
    model: Option<&str>,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
) -> String {
    let base = compose_status_line(base_status, provider, model);
    append_context_usage(base, metrics)
}

/// Returns true when the base status indicates a working/loading state (H5).
/// The synchronous architecture freezes redraw during blocking dispatch, so the
/// working marker is styled distinctly to make the frozen state unmistakable.
#[must_use]
pub fn is_working_status(base_status: &str) -> bool {
    let lower = base_status.to_ascii_lowercase();
    lower.contains("working")
}

/// Header bar content helper (H1 — deduped: provider/model ONLY when configured).
/// When `provider` is absent, returns only `" Siralos "` (the "no provider
/// configured" text lives only in the bottom status line via `compose_status_line`).
#[must_use]
pub fn header_text(provider: Option<&str>, model: Option<&str>) -> String {
    match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => {
            let sp = safe_display_label(p);
            let sm = safe_display_label(m);
            format!(" Siralos  {sp} / {sm}")
        }
        (Some(p), _) if !p.is_empty() => {
            let sp = safe_display_label(p);
            format!(" Siralos  {sp}")
        }
        _ => " Siralos ".to_owned(),
    }
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
