//! Provider-neutral evidence model-view projection.
//!
//! Raw evidence and raw tool results remain authoritative and unchanged.
//! Projection creates a detached provider-facing view only. The transform
//! order is: strip ANSI/control → redact secrets → optionally collapse
//! repeated lines (never-worse, UTF-16 length) → mandatory line bound
//! (scalar-safe, impossible-sub-scalar exception) → final truncation
//! (marker `\\n… [truncated]`, marker-only when marker exceeds the budget,
//! and the terminal marker is the narrow second line-bound exception).

/// Default evidence budget: 32 KiB UTF-8.
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 32 * 1024;
/// Default per-line budget: 1 KiB UTF-8.
pub const DEFAULT_MAX_LINE_BYTES: usize = 1_024;

/// The exact truncation marker (LF + ellipsis + " [truncated]").
pub const TRUNCATION_MARKER: &str = "\n… [truncated]";
/// UTF-8 bytes of the marker (16).
pub const TRUNCATION_MARKER_BYTES: usize = 16; // "\n" 1 + "…" 3 + " [truncated]" 12

/// Transformation labels (ordered, as emitted by the oracle).
pub const TRANSFORM_STRIP_ANSI: &str = "strip-ansi-control";
/// Transformation label for secret redaction.
pub const TRANSFORM_REDACT_SECRETS: &str = "redact-secrets";
/// Transformation label for repeat-line collapse.
pub const TRANSFORM_COLLAPSE: &str = "collapse-repeated-lines";
/// Transformation label for mandatory line bounding.
pub const TRANSFORM_BOUND_LINES: &str = "bound-lines";
/// Transformation label for final truncation.
pub const TRANSFORM_TRUNCATE: &str = "truncate";

/// Fixed secret placeholder (the frozen `███[REDACTED]███` token).
pub const REDACTED_PLACEHOLDER: &str = "███[REDACTED]███";

/// Model-visible detached view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEvidenceView {
    /// Opaque evidence id (never a host path).
    pub evidence_id: Option<String>,
    /// Workspace revision handle when known.
    pub revision: Option<String>,
    /// Projected text.
    pub text: String,
    /// Whether final truncation applied.
    pub truncated: bool,
    /// UTF-8 bytes of `text`.
    pub shown_bytes: usize,
    /// UTF-8 bytes of the original raw text.
    pub original_bytes: usize,
    /// Ordered transformation labels.
    pub transformations: Vec<String>,
}

/// Options for the evidence projector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProjectorOptions {
    /// Ordered secrets (empty strings are skipped).
    pub secrets: Vec<String>,
    /// Hard cap on projected text (UTF-8 bytes).
    pub max_total_bytes: usize,
    /// Hard cap on one LF-delimited line (UTF-8 bytes).
    pub max_line_bytes: usize,
}

impl Default for EvidenceProjectorOptions {
    fn default() -> Self {
        Self {
            secrets: Vec::new(),
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
        }
    }
}

// ---------------------------------------------------------------------------
// Security transforms
// ---------------------------------------------------------------------------

fn is_ansi_escape_at(text: &str, byte_index: usize) -> bool {
    let bytes = text.as_bytes();
    if byte_index + 1 >= bytes.len() {
        return false;
    }
    if bytes[byte_index] != 0x1b || bytes[byte_index + 1] != 0x5b {
        return false;
    }
    // Scan for final byte 0x40..0x7e; a control byte inside makes it malformed.
    for &code in bytes.iter().skip(byte_index + 2) {
        if (0x40..=0x7e).contains(&code) {
            return true;
        }
        if code < 0x20 || code == 0x7f {
            return false;
        }
    }
    false
}

fn is_control(code: u32) -> bool {
    matches!(code, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)
}

/// Strip ANSI CSI sequences and C0/DEL controls (preserving tab, LF, CR).
pub fn strip_ansi_and_control(text: &str) -> String {
    // Work on chars to preserve Unicode, but detect CSI via byte pattern
    // on the UTF-8 representation which is valid since ESC and '[' are ASCII.
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut byte_index = 0usize;
    let char_indices: Vec<(usize, char)> = text.char_indices().collect();
    // Map byte_index to char position for CSI detection
    let mut char_pos = 0usize;
    while char_pos < char_indices.len() {
        let (b, ch) = char_indices[char_pos];
        // Check CSI at this byte position
        if is_ansi_escape_at(text, b) {
            // Consume ESC '[' plus params/intermediates up to final byte.
            // Final byte is 0x40..0x7e; control inside terminates as malformed.
            // We already validated `is_ansi_escape_at`, so skip to final.
            let mut consumed_bytes = 2; // ESC '['
            let mut scan = b + 2;
            while scan < bytes.len() {
                let code = bytes[scan];
                consumed_bytes += 1;
                scan += 1;
                if (0x40..=0x7e).contains(&code) {
                    break;
                }
                if code < 0x20 || code == 0x7f {
                    break;
                }
            }
            // Advance char_pos past the consumed bytes.
            let end_byte = b + consumed_bytes;
            while char_pos < char_indices.len()
                && char_indices[char_pos].0 < end_byte
            {
                char_pos += 1;
            }
            byte_index = end_byte;
            continue;
        }
        if is_control(ch as u32) {
            byte_index = b + ch.len_utf8();
            char_pos += 1;
            continue;
        }
        out.push(ch);
        byte_index = b + ch.len_utf8();
        char_pos += 1;
    }
    // Silence unused
    let _ = byte_index;
    out
}

/// Collapse 3+ consecutive equal LF-delimited lines into `line ×N`.
pub fn collapse_repeated_lines(text: &str) -> String {
    // Use JS semantics: split on "\n" keeping empty trailing parts.
    // Rust `split('\n')` already preserves trailing empties in the slice
    // length, but joining with "\n" must reconstruct them.
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let mut run = 1usize;
        while index + run < lines.len() && lines[index + run] == line {
            run += 1;
        }
        if run >= 3 {
            out.push(format!("{line} \u{00D7}{run}"));
        } else {
            for offset in 0..run {
                out.push(lines[index + offset].to_owned());
            }
        }
        index += run;
    }
    out.join("\n")
}

/// Replace ordered non-empty secrets with the fixed placeholder.
pub fn redact_secrets(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        out = out.replace(secret.as_str(), REDACTED_PLACEHOLDER);
    }
    out
}

// ---------------------------------------------------------------------------
// Unicode-safe scalar slicing
// ---------------------------------------------------------------------------

/// All valid UTF-8 scalar boundaries (byte offsets) in `text`, including 0 and len.
/// Since Rust `&str` is always valid UTF-8, every `char` boundary is a scalar
/// boundary; no lone surrogates exist. This matches the JS oracle's
/// `unicodeBoundaries` for well-formed strings, and lone surrogates in JS
/// source would be represented as replacement characters in Rust input (which
/// is valid UTF-8 and never split by our logic).
fn scalar_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
    boundaries.push(0);
    let mut offset = 0usize;
    for ch in text.chars() {
        offset += ch.len_utf8();
        boundaries.push(offset);
    }
    boundaries
}

/// Split LF-delimited lines that exceed `max_line_bytes` UTF-8, respecting
/// scalar boundaries and the impossible-sub-scalar exception.
pub fn bound_line_length(text: &str, max_line_bytes: usize) -> String {
    let mut out_lines: Vec<String> = Vec::new();
    // Split on '\n' preserving empty trailing parts exactly like JS `split("\n")`.
    // `split('\n')` yields N+1 parts for N separators; that's the desired JS behavior.
    let lines: Vec<&str> = text.split('\n').collect();
    for line in lines {
        if line.len() <= max_line_bytes {
            out_lines.push(line.to_owned());
            continue;
        }
        let mut remaining = line;
        while remaining.len() > max_line_bytes {
            let boundaries = scalar_boundaries(remaining);
            // First scalar boundary after 0
            let first = boundaries.get(1).copied();
            let Some(first_end) = first else {
                break;
            };
            if remaining.as_bytes()[..first_end].len() > max_line_bytes {
                // Impossible bound for this scalar: retain it whole.
                out_lines.push(remaining[..first_end].to_owned());
                remaining = &remaining[first_end..];
                continue;
            }
            // Binary search for largest scalar prefix fitting.
            let mut low = 1usize;
            let mut high = boundaries.len() - 1;
            while low < high {
                let mid = (low + high).div_ceil(2);
                let end = boundaries[mid];
                if remaining.as_bytes()[..end].len() <= max_line_bytes {
                    low = mid;
                } else {
                    high = mid - 1;
                }
            }
            let end = boundaries[low];
            out_lines.push(remaining[..end].to_owned());
            remaining = &remaining[end..];
        }
        if !remaining.is_empty() {
            out_lines.push(remaining.to_owned());
        }
    }
    out_lines.join("\n")
}

/// Truncate to `max_bytes` UTF-8 with the exact marker `\\n… [truncated]`,
/// preserving scalar boundaries and the marker-only fallback.
pub fn truncate_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let boundaries = scalar_boundaries(text);
    // Binary search for largest scalar prefix where prefix+marker fits.
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    while low < high {
        let mid = (low + high).div_ceil(2);
        let end = boundaries[mid];
        if text.as_bytes()[..end].len() + TRUNCATION_MARKER.len() <= max_bytes
        {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let end = boundaries[low];
    let truncated = format!("{}{TRUNCATION_MARKER}", &text[..end]);
    // If marker itself exceeds the budget, marker-only is the contract.
    // Our search leaves `low==0` and `truncated==marker` when even empty
    // prefix + marker exceeds max_bytes but we still return marker (the
    // oracle's behavior for tiny budgets).
    if truncated.len() > max_bytes && TRUNCATION_MARKER.len() > max_bytes {
        return (TRUNCATION_MARKER.to_owned(), true);
    }
    (truncated, true)
}

// ---------------------------------------------------------------------------
// Orchestration (projectForModel)
// ---------------------------------------------------------------------------

/// Project `raw_text` to a detached model view with ordered transformations.
pub fn project_for_model(
    evidence_id: Option<String>,
    revision: Option<String>,
    raw_text: &str,
    options: &EvidenceProjectorOptions,
) -> ModelEvidenceView {
    let original_bytes = raw_text.len();
    let mut transformations: Vec<String> = Vec::new();
    let mut text = raw_text.to_owned();

    // 1. Security: strip ANSI/control
    let stripped = strip_ansi_and_control(&text);
    if stripped != text {
        text = stripped;
        transformations.push(TRANSFORM_STRIP_ANSI.to_owned());
    }
    // 2. Security: redact secrets
    let has_secret = options
        .secrets
        .iter()
        .any(|s| !s.is_empty() && text.contains(s.as_str()));
    if has_secret {
        text = redact_secrets(&text, &options.secrets);
        transformations.push(TRANSFORM_REDACT_SECRETS.to_owned());
    }
    // 3. Optional collapse (never-worse: UTF-16 length)
    let pre_reduction = text.clone();
    let collapsed = collapse_repeated_lines(&text);
    let mut collapse_applied = false;
    if collapsed != text {
        // Never-worse uses JS UTF-16 .length
        let collapsed_utf16_len: usize = collapsed.encode_utf16().count();
        let text_utf16_len: usize = text.encode_utf16().count();
        if collapsed_utf16_len <= text_utf16_len {
            text = collapsed;
            transformations.push(TRANSFORM_COLLAPSE.to_owned());
            collapse_applied = true;
        }
    }
    // 4. Mandatory line bound
    let bounded = bound_line_length(&text, options.max_line_bytes);
    if bounded != text {
        text = bounded;
        transformations.push(TRANSFORM_BOUND_LINES.to_owned());
    }
    if collapse_applied && text.len() > original_bytes {
        // Discard only the optional collapse; reapply mandatory bound to post-security text.
        text = bound_line_length(&pre_reduction, options.max_line_bytes);
        transformations
            .retain(|t| t != TRANSFORM_COLLAPSE && t != TRANSFORM_BOUND_LINES);
        if text != pre_reduction {
            transformations.push(TRANSFORM_BOUND_LINES.to_owned());
        }
    }
    // 5. Final truncation (terminal)
    let (truncated_text, did_truncate) =
        truncate_text(&text, options.max_total_bytes);
    if did_truncate {
        text = truncated_text;
        transformations.push(TRANSFORM_TRUNCATE.to_owned());
    }
    let shown_bytes = text.len();
    ModelEvidenceView {
        evidence_id,
        revision,
        text,
        truncated: did_truncate,
        shown_bytes,
        original_bytes,
        transformations,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EvidenceProjectorOptions, TRUNCATION_MARKER, collapse_repeated_lines,
        project_for_model, redact_secrets, strip_ansi_and_control,
        truncate_text,
    };

    fn opts(max_line: usize, max_total: usize) -> EvidenceProjectorOptions {
        EvidenceProjectorOptions {
            secrets: Vec::new(),
            max_total_bytes: max_total,
            max_line_bytes: max_line,
        }
    }

    #[test]
    fn redact() {
        let out = redact_secrets(
            "connect with sk-live-1234 twice sk-live-1234",
            &["sk-live-1234".to_owned()],
        );
        assert!(!out.contains("sk-live-1234"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn strip_controls() {
        let out =
            strip_ansi_and_control("\u{001b}[31mred\u{001b}[0m line\u{0007}");
        assert!(out.contains("red line"));
        assert!(!out.contains('\u{001b}'));
    }

    #[test]
    fn collapse() {
        let raw = "progress 1/10\nprogress 1/10\nprogress 1/10\n";
        let out = collapse_repeated_lines(raw);
        assert!(out.contains('\u{00D7}'));
        assert_eq!(
            out.split('\n').filter(|l| l.contains("progress")).count(),
            1
        );
    }

    #[test]
    fn truncation_marker() {
        let (text, truncated) = truncate_text(&"x".repeat(10000), 64);
        assert!(truncated);
        assert!(text.contains("[truncated]"));
        assert!(text.len() <= 64 || text == TRUNCATION_MARKER);
    }

    #[test]
    fn ascii_line_bound_via_project_for_model() {
        let raw = "a".repeat(2048);
        let view = project_for_model(None, None, &raw, &opts(1024, 32768));
        let lens: Vec<usize> =
            view.text.split('\n').map(|l| l.len()).collect();
        assert_eq!(lens, vec![1024, 1024]);
        assert!(view.transformations.contains(&"bound-lines".to_owned()));
    }

    #[test]
    fn exact_boundary_unchanged() {
        let raw = "a".repeat(1024);
        let view = project_for_model(None, None, &raw, &opts(1024, 32768));
        assert_eq!(view.text, raw);
        assert!(view.transformations.is_empty());
    }

    #[test]
    fn supplementary_scalar_at_boundary() {
        let raw = format!("{}{}", "a".repeat(1021), "\u{1F600}");
        let view = project_for_model(None, None, &raw, &opts(1024, 32768));
        assert_eq!(view.text, format!("{}\n\u{1F600}", "a".repeat(1021)));
        assert!(!view.text.contains('\u{FFFD}'));
    }

    #[test]
    fn impossible_sub_scalar() {
        let view = project_for_model(
            None,
            None,
            "\u{1F600}\u{1F600}",
            &opts(3, 32768),
        );
        assert_eq!(view.text, "\u{1F600}\n\u{1F600}");
        assert!(view.text.contains("\u{1F600}"));
    }

    #[test]
    fn never_worse_discards_collapse_but_keeps_line_bound() {
        let view = project_for_model(
            None,
            None,
            "s\ns\ns",
            &EvidenceProjectorOptions {
                secrets: vec!["s".to_owned()],
                max_total_bytes: 1000,
                max_line_bytes: 20,
            },
        );
        assert!(!view.text.contains('s') || view.text.contains("[REDACTED]"));
        assert!(view.transformations.contains(&"bound-lines".to_owned()));
        assert!(
            !view
                .transformations
                .contains(&"collapse-repeated-lines".to_owned())
        );
    }

    #[test]
    fn terminal_marker_tiny_line_bound() {
        // Total truncation marker must remain whole even when line bound is tiny.
        let view = project_for_model(
            None,
            None,
            &"x".repeat(5000),
            &EvidenceProjectorOptions {
                secrets: Vec::new(),
                max_total_bytes: 32,
                max_line_bytes: 4,
            },
        );
        assert!(view.truncated);
        assert!(view.text.contains("[truncated]"));
        // No second line-bound pass after marker: the marker line may exceed max_line_bytes.
        assert!(view.text.ends_with("[truncated]"));
    }
}
