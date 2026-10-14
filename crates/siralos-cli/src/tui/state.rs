//! [`TuiState`]: the pure render model over the live session, its
//! transcript/input/scroll state, and modal-opening transitions.

use super::{
    ApprovalModal, MAX_INPUT_BYTES, MAX_REASONING_BUFFER_BYTES,
    MAX_STREAM_BYTES, MAX_TRANSCRIPT_LINE_BYTES, MAX_TRANSCRIPT_LINES,
    ModelPicker, ProviderAddForm, ProviderEntry, ProviderPicker,
    REASONING_BYTES, REASONING_ROWS, SIRALOS_BANNER, SIRALOS_GREETING,
    THINKING_TAIL_CHARS, WorkspaceProfileWriteToken,
    safe_form_message_for_credential,
};

/// One transcript entry — a sanitized line plus an optional local stamp.
///
/// `I4` (decision 119 H4): transcript entries carry caller-supplied
/// timestamps rendered as a dim line below each message; live sessions stamp
/// with the user's local timezone via `local_timestamp_now` (`time` crate
/// `local-offset`); differential fixtures carry fixed values so pinned frames
/// stay deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    /// Sanitized line text (no embedded newlines — split upstream).
    pub text: String,
    /// Optional local stamp like `"2026-08-31 14:03:22 +02:00"` (caller-supplied).
    pub timestamp: Option<String>,
}

/// Ordered command catalog over the SAME `SlashCommand` vocabulary (I2).
///
/// This is the SINGLE catalog the palette and the unknown-command honesty
/// line derive from — no parallel list. Order matches the
/// `parse_slash_command` arms including the U7/U8 additive commands.
/// Delegates to `crate::interactive::slash_command_catalog` — single source
/// (R3); the owned `String` conversion keeps the TUI palette type stable.
#[must_use]
pub fn command_catalog() -> Vec<(String, String)> {
    crate::interactive::slash_command_catalog()
        .into_iter()
        .map(|(name, desc)| (name.to_owned(), desc.to_owned()))
        .collect()
}

/// Pure render model for the TUI shell.
///
/// `Eq` is deliberately absent: the reveal debt (S3c) is a float, and `f64`
/// has no total equality. `PartialEq` is what the tests compare with.
#[derive(Clone, PartialEq)]
pub struct TuiState {
    /// Transcript entries — bounded ring, oldest dropped when full. Each entry
    /// is a single sanitized line plus an optional local stamp.
    pub transcript: Vec<TranscriptEntry>,
    /// Back-compat accessor: transcript lines as plain strings (tests).
    /// Prefer `transcript` directly; this is kept for harness compat.
    pub transcript_lines: Vec<String>,
    /// Current input line (edited in-place, not yet submitted).
    pub input: String,
    /// Status line text.
    pub status: String,
    /// Scroll offset from the tail (0 = show tail, n = n lines up).
    pub scroll_offset: u16,
    /// Pending approval modal — when `Some`, all non-modal keys are ignored and
    /// the modal renders centered over a dimmed transcript (T2).
    pub pending_approval: Option<ApprovalModal>,
    /// When `true`, the pending approval modal is a provider-removal
    /// confirmation (armed by the "- Remove provider" picker row or
    /// `/provider remove`): `y` removes via `remove_profile_config_at`,
    /// `n`/`Esc` cancels. `false` for ordinary tool approvals.
    pub confirming_provider_removal: bool,
    /// Exchange id of a suspended tool-approval turn (Part B). Set
    /// with the modal when a worker approval request suspends the
    /// turn; the main loop delivers the modal verdict into the
    /// exchange under this id and resumes the turn. `None` for
    /// provider-removal confirmations (which resolve locally) and
    /// whenever no approval suspends a turn.
    pub pending_tool_approval: Option<u64>,
    /// Secret-free profile revision observed when provider removal was armed.
    /// It keeps a confirmation prompt from turning into a stale merge.
    pub pending_profile_write_token: Option<WorkspaceProfileWriteToken>,
    /// Command palette popup (I2): when `Some`, the input starts with `/` and
    /// this holds the filtered catalog entries (case-insensitive prefix filter).
    pub palette: Option<Vec<(String, String)>>,
    /// Selected palette entry index (I2): when `Some`, the highlighted entry
    /// in the filtered palette (arrow-key navigation).
    pub palette_selected: Option<usize>,
    /// Provider for header bar (P2) — same source as status line.
    pub provider: Option<String>,
    /// Model for header bar (P2).
    pub model: Option<String>,
    /// Endpoint the worker composed the session with (C2 step 3, decision 168
    /// R3). Display only: the picker shows it, and `/provider` renders it.
    pub endpoint: Option<String>,
    /// Protocol the worker built the provider with (R3).
    pub protocol: String,
    /// The credential in its ALREADY-REDACTED display form (R2). The raw value
    /// never reaches the frontend, so this is what the provider surfaces show.
    pub credential_display: Option<String>,
    /// Whether the declared credential RESOLVED. The `/models` arm decides on
    /// this today and the frontend cannot recompute it, so the worker answers
    /// it as a fact (never the value).
    pub credential_resolved: bool,
    /// Whether `/model <id>` can move the live model for this composition.
    /// The frontend persists the profile BEFORE the worker applies the switch,
    /// so it needs this fact to refuse a switch it cannot finish -- otherwise
    /// the file would claim a model the session never adopted.
    pub live_model_switchable: bool,
    /// The context-usage suffix the worker's status line carries
    /// (` | ctx N/4096`), empty when the subsystem is off. Cached because a
    /// TRANSIENT status (the add-flow's "fetching models...") must keep the
    /// readout, and the frontend no longer holds the metrics that build it.
    pub context_suffix: String,
    /// Provider picker popup (H6) — when Some, Up/Down + Enter/Esc handle it.
    pub provider_picker: Option<ProviderPicker>,
    /// Provider add-flow form (C1) — sequential modal form; when Some, no other
    /// keys pass (modal discipline).
    pub provider_add_form: Option<ProviderAddForm>,
    /// `/model` switch picker — when Some, Up/Down + Enter/Esc handle it.
    /// Opened by bare `/model` over the provider's fetched models; this is
    /// the same [`ModelPicker`] (+ sliding viewport, decision 138) the
    /// add-flow uses, not a second picker. Enter arms
    /// `pending_model_switch` for the loop to resolve through the same
    /// switch-and-persist as the explicit-argument form.
    pub model_switch_picker: Option<ModelPicker>,
    /// Armed model id from the switch picker — consumed once by the
    /// interactive loop (same switch-and-persist as `/model <id>`).
    pub pending_model_switch: Option<String>,
    /// Submitted prompt history (I4): oldest first, bounded to 100, per-session
    /// in-memory with no persistence.
    pub prompt_history: Vec<String>,
    /// Current history navigation index (I4): `None` means not navigating,
    /// `Some(idx)` indexes into `prompt_history`.
    pub history_index: Option<usize>,
    /// Saved input before history navigation (I4): restored when navigating
    /// past the newest entry.
    pub history_draft: Option<String>,
    /// Mouse capture state: `false` (default) hands the mouse to the
    /// terminal, so click-drag selects text and the terminal's own paste
    /// works. `true` captures it so wheel events scroll the transcript
    /// directly. Render-neutral: no pane, status, or frame change — the
    /// differential frames pin this.
    pub mouse_capture: bool,
    /// Thinking streamed so far (S3): host-accounted model output that is
    /// NOT the answer. Bounded to the last [`REASONING_BYTES`]; rendered as
    /// one collapsed row that Right expands and Left collapses.
    pub reasoning: String,
    /// Whether the thinking block is expanded.
    pub reasoning_expanded: bool,
    /// The stored-transcript index the thinking block renders ABOVE (S3d).
    ///
    /// Thinking is what the model produced FIRST and the answer is what it
    /// produced FROM it, so the block sits above the answer. Appending it
    /// after the transcript instead -- where it used to live -- put it below
    /// every answer line the reveal had already committed, which is the
    /// inversion the owner reported.
    pub reasoning_anchor: usize,
    /// The anchor the CURRENT turn takes when its thinking arrives (S3d).
    ///
    /// [`TuiState::begin_turn`] arms it with the transcript as it stands after
    /// the submitted prompt; the first streamed delta consumes it. It stays
    /// `None` between turns, so a turn that never reasons leaves the block
    /// exactly where the turn that produced it put it.
    pub pending_reasoning_anchor: Option<usize>,
    /// When the current turn started, for the pulsing `working` line.
    pub busy_since: Option<std::time::Instant>,
    /// Answer text received but not yet revealed (S3c).
    pub stream_buffer: String,
    /// Byte cursor into `stream_buffer`; avoids shifting the remaining buffer
    /// once per revealed scalar.
    stream_cursor: usize,
    /// The revealed text of the INCOMPLETE line, rendered as a growing row
    /// so a long answer appears left to right instead of popping in whole.
    pub stream_tail: String,
    /// How much of `reasoning` has been revealed.
    pub reasoning_shown: usize,
    /// Monotonic turn identity used to reset turn-scoped output sanitizers.
    pub turn_epoch: u64,
}

impl std::fmt::Debug for TuiState {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("TuiState")
            .field("input", &"[REDACTED]")
            .field("prompt_history_len", &self.prompt_history.len())
            .field(
                "provider",
                &self.provider.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_display.as_deref(),
                    )
                }),
            )
            .field(
                "model",
                &self.model.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_display.as_deref(),
                    )
                }),
            )
            .field("endpoint", &self.endpoint.as_ref().map(|_| "[PROJECTED]"))
            .field("credential_display", &self.credential_display)
            .field("status", &self.status)
            .field("stream_buffer_len", &self.stream_buffer.len())
            .field("reasoning_len", &self.reasoning.len())
            .finish_non_exhaustive()
    }
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            transcript: Vec::new(),
            transcript_lines: Vec::new(),
            input: String::new(),
            status: String::from("ready"),
            scroll_offset: 0,
            pending_approval: None,
            confirming_provider_removal: false,
            pending_tool_approval: None,
            pending_profile_write_token: None,
            palette: None,
            palette_selected: None,
            provider: None,
            model: None,
            endpoint: None,
            protocol: String::new(),
            credential_display: None,
            credential_resolved: false,
            live_model_switchable: true,
            context_suffix: String::new(),
            provider_picker: None,
            provider_add_form: None,
            model_switch_picker: None,
            pending_model_switch: None,
            prompt_history: Vec::new(),
            history_index: None,
            history_draft: None,
            // OFF by default (owner ruling 2026-09-12): native select/copy
            // and paste must work out of the box. With capture off the
            // terminal turns the wheel into arrow keys in the alternate
            // screen, and an empty prompt scrolls the transcript with
            // them; `/mouse` captures the mouse when raw wheel events are
            // wanted.
            mouse_capture: false,
            reasoning: String::new(),
            reasoning_expanded: false,
            reasoning_anchor: 0,
            pending_reasoning_anchor: None,
            busy_since: None,
            stream_buffer: String::new(),
            stream_cursor: 0,
            stream_tail: String::new(),
            reasoning_shown: 0,
            turn_epoch: 0,
        }
    }
}

impl TuiState {
    /// Create an empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sync `transcript` and `transcript_lines` when the caller assigned
    /// directly to one of them (the harness and old tests use
    /// `transcript_lines`). When they diverge, rebuild `transcript` from
    /// `transcript_lines` (with `None` timestamps) — the live path keeps both
    /// in lockstep via `push_*`.
    fn sync_transcript(&mut self) {
        if self.transcript.len() != self.transcript_lines.len() {
            // If `transcript` was built via the new API, `transcript_lines` may
            // lag; if the caller wrote `transcript_lines` directly, `transcript`
            // lags. Prefer the longer/newer `transcript_lines` when `transcript`
            // is empty and `transcript_lines` non-empty (harness path).
            if self.transcript.is_empty() && !self.transcript_lines.is_empty()
            {
                self.transcript = self
                    .transcript_lines
                    .iter()
                    .map(|text| TranscriptEntry {
                        text: text.clone(),
                        timestamp: None,
                    })
                    .collect();
            } else if self.transcript_lines.is_empty()
                && !self.transcript.is_empty()
            {
                self.transcript_lines = self
                    .transcript
                    .iter()
                    .map(|entry| entry.text.clone())
                    .collect();
            } else if self.transcript.len() != self.transcript_lines.len() {
                // Keep both in sync by rebuilding `transcript_lines` from `transcript`.
                self.transcript_lines = self
                    .transcript
                    .iter()
                    .map(|entry| entry.text.clone())
                    .collect();
            }
        }
    }

    /// Effective transcript slice after syncing the two storages.
    fn effective_transcript_len(&self) -> usize {
        // `transcript` is authoritative when non-empty; otherwise fall back to
        // `transcript_lines` (harness direct assignment).
        if !self.transcript.is_empty() || self.transcript_lines.is_empty() {
            self.transcript.len()
        } else {
            self.transcript_lines.len()
        }
    }

    /// Open a turn (S1/S3d): start the `working` clock and ARM the thinking
    /// block's anchor.
    ///
    /// The anchor is the transcript as it stands AFTER the submitted prompt,
    /// and the turn's first streamed thinking takes it. Anchoring at the turn's
    /// start -- rather than at that first delta -- is what keeps the block
    /// above the answer's FIRST line: the reveal commits answer lines into the
    /// transcript while it runs, so a later anchor would leave the earliest of
    /// them above the block, which is the inversion the owner reported.
    pub fn begin_turn(&mut self, now: std::time::Instant) {
        self.turn_epoch = self.turn_epoch.wrapping_add(1);
        self.busy_since = Some(now);
        self.pending_reasoning_anchor = Some(self.effective_transcript_len());
    }

    /// Close a turn (S3d): stop the `working` clock and disarm the anchor.
    ///
    /// Disarming -- not re-anchoring -- is what keeps the block with the turn
    /// that produced it: a later turn that streams no thinking leaves it
    /// exactly where it was, instead of dragging an older trace down the
    /// conversation.
    pub fn end_turn(&mut self) {
        self.busy_since = None;
        self.pending_reasoning_anchor = None;
    }

    /// Current turn identity for turn-scoped stream sanitizers.
    #[must_use]
    pub fn turn_epoch(&self) -> u64 {
        self.turn_epoch
    }

    /// Release the next character owed to the reader, if any (S3c).
    ///
    /// ONE character per call, by construction: the text is rendered a
    /// character at a time, so the character rate IS the frame rate. The paint
    /// path is the only caller -- one call, one painted frame, one character --
    /// which is why there is no timer, no rate budget and no catch-up here. The
    /// painters own the cadence ([`paint_interval`](super::limits::paint_interval)); this owns the ORDER.
    ///
    /// S3d: the THINKING is released first, because the block renders above the
    /// answer -- a reader meets the model's output in the order the model
    /// produced it. That costs the answer nothing in practice: the reveal is
    /// limited by the frame cost, several times faster than a reasoning stream,
    /// so it has already drained the thinking by the time answer text arrives.
    pub fn reveal_char(&mut self) {
        if self.reasoning_shown < self.reasoning.len() {
            let ch = self.reasoning[self.reasoning_shown..]
                .chars()
                .next()
                .expect("an index below the length starts a character");
            self.reasoning_shown += ch.len_utf8();
            return;
        }
        if self.stream_cursor > self.stream_buffer.len() {
            self.stream_cursor = 0;
        }
        if let Some(ch) =
            self.stream_buffer[self.stream_cursor..].chars().next()
        {
            self.stream_cursor += ch.len_utf8();
            if self.stream_cursor >= self.stream_buffer.len() {
                self.stream_buffer.clear();
                self.stream_cursor = 0;
            } else if self.stream_cursor > 64 * 1024 {
                // Split rather than `drain`: draining shifts the entire
                // remaining buffer for every character after the threshold.
                let remainder =
                    self.stream_buffer.split_off(self.stream_cursor);
                self.stream_buffer = remainder;
                self.stream_cursor = 0;
            }
            if ch == '\n' {
                let line = std::mem::take(&mut self.stream_tail);
                self.push_line(line);
            } else {
                const TAIL_MARKER: &str = "...[truncated]";
                if self.stream_tail.len() >= MAX_STREAM_BYTES {
                    if !self.stream_tail.ends_with(TAIL_MARKER) {
                        let mut keep = MAX_STREAM_BYTES - TAIL_MARKER.len();
                        while keep > 0
                            && !self.stream_tail.is_char_boundary(keep)
                        {
                            keep -= 1;
                        }
                        self.stream_tail.truncate(keep);
                        self.stream_tail.push_str(TAIL_MARKER);
                    }
                } else if ch.len_utf8()
                    <= MAX_STREAM_BYTES - self.stream_tail.len()
                {
                    self.stream_tail.push(ch);
                } else if !self.stream_tail.ends_with(TAIL_MARKER) {
                    let mut keep = MAX_STREAM_BYTES - TAIL_MARKER.len();
                    while keep > 0 && !self.stream_tail.is_char_boundary(keep)
                    {
                        keep -= 1;
                    }
                    self.stream_tail.truncate(keep);
                    self.stream_tail.push_str(TAIL_MARKER);
                }
            }
        }
    }

    /// Whether the reader is still owed text.
    ///
    /// The character cadence applies only while something is pending; an idle
    /// UI keeps the ordinary frame cadence.
    #[must_use]
    pub fn reveal_pending(&self) -> bool {
        !self.stream_buffer.is_empty()
            || self.reasoning_shown < self.reasoning.len()
    }

    /// Append streamed thinking, bounded to [`REASONING_BYTES`] of ALREADY
    /// REVEALED text.
    ///
    /// The bound keeps a long trace from growing the state without end. What the
    /// reader is still owed is NEVER dropped: with a per-character reveal a fast
    /// trace can be thousands of characters ahead, and trimming that away would
    /// skip text the reader never saw (and jump the visible text forward). The
    /// buffer therefore exceeds the bound while a backlog exists and settles
    /// back to it once the reveal has caught up.
    pub fn push_reasoning(&mut self, text: &str) {
        if let Some(at) = self.pending_reasoning_anchor.take() {
            // The turn's thinking has arrived: take the anchor the turn opened
            // with. Clamped, because the transcript can only have grown since
            // then and a stale index must never point past the end.
            self.reasoning_anchor = at.min(self.effective_transcript_len());
        }
        let remaining =
            MAX_REASONING_BUFFER_BYTES.saturating_sub(self.reasoning.len());
        if text.len() <= remaining {
            self.reasoning.push_str(text);
        } else {
            self.reasoning.push_str(
                &siralos_core::language::truncate_utf8_bytes(text, remaining),
            );
            self.status =
                "reasoning truncated at the TUI buffer bound".to_owned();
        }
        let excess = self.reasoning.len().saturating_sub(REASONING_BYTES);
        let droppable = excess.min(self.reasoning_shown);
        if droppable == 0 {
            return;
        }
        // The reveal offset is a byte index into THIS buffer, so the cut has to
        // land on a character boundary or the next slice panics.
        let boundary = self
            .reasoning
            .char_indices()
            .map(|(index, _)| index)
            .find(|index| *index >= droppable)
            .unwrap_or(self.reasoning.len());
        self.reasoning.drain(..boundary);
        self.reasoning_shown = self.reasoning_shown.saturating_sub(boundary);
    }
    /// The thinking block as transcript rows (S3b).
    ///
    /// Empty when nothing was streamed, so a route that never reasons is
    /// byte-identical to before. Collapsed it is ONE row; expanded it is a
    /// bounded tail of the thinking, so a long trace cannot push the
    /// conversation off screen.
    #[must_use]
    pub fn reasoning_block_lines(&self) -> Vec<String> {
        // Only what has been REVEALED renders (S3c): thinking grows left to
        // right like the answer, whatever chunk the provider sent -- so the
        // emptiness test is on the revealed slice, not the raw buffer.
        let shown = self.reasoning_shown.min(self.reasoning.len());
        let visible = &self.reasoning[..shown];
        if visible.trim().is_empty() {
            return Vec::new();
        }
        let lines: Vec<&str> = visible.lines().collect();
        if !self.reasoning_expanded {
            // Show a LIVE tail, not only a line count: a count changes when
            // a line COMPLETES, which made the block look like it arrived
            // line by line instead of streaming left to right.
            let newest = lines.last().copied().unwrap_or_default().trim_end();
            let total = newest.chars().count();
            let mut tail: String = newest
                .chars()
                .skip(total.saturating_sub(THINKING_TAIL_CHARS))
                .collect();
            if total > THINKING_TAIL_CHARS {
                tail.insert_str(0, "...");
            }
            return vec![format!(
                "\u{25b8} thinking ({} lines) {tail} - press Right to expand",
                lines.len()
            )];
        }
        let mut out =
            vec![format!("\u{25be} thinking - press Left to collapse")];
        for line in lines.iter().rev().take(REASONING_ROWS).rev() {
            out.push(format!("  {line}"));
        }
        out
    }

    /// Append a sanitized line verbatim (no sanitization, no unsanitized
    /// injection). Oldest lines are dropped when the bound is exceeded.
    /// Keeps `None` timestamp (static host lines).
    pub fn push_line(&mut self, line: String) {
        self.push_line_stamped(line, None);
    }

    /// Append a sanitized line with an optional local timestamp (H4).
    /// `timestamp` like `"2026-08-31 14:03:22 +02:00"` or `"UTC"` fallback or `None` for static lines.
    pub fn push_line_stamped(
        &mut self,
        line: String,
        timestamp: Option<String>,
    ) {
        if line.contains('\n') {
            for part in line.split('\n') {
                self.push_single_stamped(part.to_owned(), timestamp.clone());
            }
        } else {
            self.push_single_stamped(line, timestamp);
        }
    }

    fn push_single_stamped(
        &mut self,
        line: String,
        timestamp: Option<String>,
    ) {
        let line = if line.len() > MAX_TRANSCRIPT_LINE_BYTES {
            let marker = "... (line truncated)";
            let content_limit =
                MAX_TRANSCRIPT_LINE_BYTES.saturating_sub(marker.len());
            format!(
                "{}{}",
                siralos_core::language::truncate_utf8_bytes(
                    &line,
                    content_limit
                ),
                marker
            )
        } else {
            line
        };
        // Keep both storages in lockstep.
        if self.transcript.len() >= MAX_TRANSCRIPT_LINES {
            let drain = self.transcript.len() - MAX_TRANSCRIPT_LINES + 1;
            self.transcript.drain(0..drain);
        }
        if self.transcript_lines.len() >= MAX_TRANSCRIPT_LINES {
            let drain = self.transcript_lines.len() - MAX_TRANSCRIPT_LINES + 1;
            self.transcript_lines.drain(0..drain);
        }
        self.transcript
            .push(TranscriptEntry { text: line.clone(), timestamp });
        self.transcript_lines.push(line);
    }

    /// Maximum scroll offset for the current transcript and viewport height.
    /// Scroll operates over entries (one entry = one row; timestamp is a dim
    /// follow-up line rendered below when present but does not affect scroll
    /// count — the viewport height accounts for lines, timestamps render as
    /// separate dim lines within the same entry height for simplicity).
    pub fn max_scroll(&self, viewport_height: u16) -> u16 {
        let total = self.effective_transcript_len() as u16;
        total.saturating_sub(viewport_height)
    }

    /// Clamp scroll_offset to the viewport.
    pub fn clamp_scroll(&mut self, viewport_height: u16) {
        let max = self.max_scroll(viewport_height);
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }

    /// Update palette based on current input (I2). Call after every key edit:
    /// when `input` starts with `/`, filter the single catalog by the typed
    /// prefix (case-insensitive, prefix match); otherwise clear the palette.
    /// Resets `palette_selected` to `None` on every recompute; clears history
    /// navigation while the palette is visible (arrow keys route to palette).
    pub fn update_palette(&mut self) {
        if self.input.starts_with('/') {
            let prefix = self.input.to_ascii_lowercase();
            let filtered: Vec<(String, String)> = command_catalog()
                .into_iter()
                .filter(|(name, _)| {
                    name.to_ascii_lowercase().starts_with(&prefix)
                })
                .collect();
            if filtered.is_empty() {
                self.palette = Some(vec![]);
            } else {
                self.palette = Some(filtered);
            }
            self.palette_selected = None;
            // While palette is visible, history navigation is dormant.
            self.history_index = None;
            self.history_draft = None;
        } else {
            self.palette = None;
            self.palette_selected = None;
        }
    }

    /// Push a submitted prompt into history (I4), bounded to 100, oldest first.
    /// Empty or whitespace-only prompts are ignored.
    pub fn push_history(&mut self, prompt: String) {
        if prompt.trim().is_empty() {
            return;
        }
        // Avoid consecutive duplicates (optional but keeps stack clean).
        if self.prompt_history.last().is_some_and(|last| last == &prompt) {
            return;
        }
        let prompt = siralos_core::language::truncate_utf8_bytes(
            &prompt,
            MAX_INPUT_BYTES,
        );
        self.prompt_history.push(prompt);
        while self.prompt_history.len() > 100
            || self.prompt_history.iter().map(String::len).sum::<usize>()
                > MAX_INPUT_BYTES
        {
            self.prompt_history.remove(0);
        }
        self.history_index = None;
        self.history_draft = None;
    }

    /// Returns the effective transcript entries for rendering (syncs first).
    pub fn effective_entries(&mut self) -> Vec<TranscriptEntry> {
        self.sync_transcript();
        if !self.transcript.is_empty() {
            self.transcript.clone()
        } else {
            self.transcript_lines
                .iter()
                .map(|text| TranscriptEntry {
                    text: text.clone(),
                    timestamp: None,
                })
                .collect()
        }
    }
}

/// Open the provider picker (H6) — caller supplies entries from workspace config.
/// Read-only: no config write; selection echo handled in `handle_key`.
/// When entries is non-empty an extra "Add provider" entry is appended whose
/// selection opens the add-flow form (C1), followed by a "- Remove provider"
/// entry whose selection arms the y/N removal confirmation. An empty
/// configuration shows only the add entry (there is nothing to remove).
pub fn open_provider_picker(
    state: &mut TuiState,
    mut entries: Vec<ProviderEntry>,
) {
    // C1: add entry — present even when a provider exists so the user can
    // re-configure; when no provider exists the picker shows only this entry.
    if entries.is_empty() {
        entries.push(ProviderEntry {
            name: "+ Add provider".to_owned(),
            host: "—".to_owned(),
            model: "—".to_owned(),
        });
    } else {
        entries.push(ProviderEntry {
            name: "+ Add provider".to_owned(),
            host: "add".to_owned(),
            model: "new".to_owned(),
        });
        entries.push(ProviderEntry {
            name: "- Remove provider".to_owned(),
            host: "remove".to_owned(),
            model: "profile".to_owned(),
        });
    }
    state.provider_picker = Some(ProviderPicker::new(entries));
    state.input.clear();
    state.update_palette();
}

/// Open the provider-removal confirmation (y/N modal over the shared
/// approval gate): `y` removes the `[profile]` section, `n`/`Esc` cancels.
/// Static host strings (sanitizer-clean); the decision resolves in the
/// interactive loop through the single removal outcome.
pub fn open_provider_remove_confirm(state: &mut TuiState) {
    state.provider_picker = None;
    state.pending_approval = Some(ApprovalModal::new(vec![
        "Remove the configured provider from siralos.toml?".to_owned(),
        "This deletes the [profile] section. Restart the session to apply."
            .to_owned(),
    ]));
    state.confirming_provider_removal = true;
    state.input.clear();
    state.update_palette();
}

/// Open the provider add-flow form (C1) — sequential modal form.
pub fn open_provider_add_form(state: &mut TuiState) {
    state.provider_picker = None;
    state.provider_add_form = Some(ProviderAddForm::new());
    state.input.clear();
    state.update_palette();
}

/// Open the `/model` switch picker over the provider's fetched models.
///
/// Reuses the add-flow [`ModelPicker`] (decision 138 sliding viewport),
/// not a second picker. Selecting an entry arms `pending_model_switch`,
/// which the interactive loop resolves through the same switch-and-persist
/// as `/model <id>`. An empty fetch never opens (the caller reports it
/// truthfully instead). Fetched ids are retained byte-for-byte in the
/// picker; `model_picker_lines` applies the display mask only at render
/// time. The switch itself validates the id against the core model rule
/// before anything is written or messaged.
pub fn open_model_switch_picker(state: &mut TuiState, items: Vec<String>) {
    let items: Vec<String> = items
        .into_iter()
        .filter(|item| siralos_core::composition::is_model_id(item))
        .take(1024)
        .collect();
    if items.is_empty() {
        return;
    }
    state.model_switch_picker = Some(ModelPicker { items, selected: 0 });
    state.pending_model_switch = None;
    state.input.clear();
    state.update_palette();
}

/// Push the SIRALOS banner + greeting into the transcript at session start (H2).
/// TUI-only; stdio path unchanged. Sanitized-safe static host strings.
/// A blank line separates the banner block from the greeting (C6).
pub fn push_banner_and_greeting(state: &mut TuiState) {
    for &line in SIRALOS_BANNER {
        state.push_line(line.to_owned());
    }
    state.push_line(String::new());
    state.push_line(SIRALOS_GREETING.to_owned());
}
