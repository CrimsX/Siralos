//! The slash-command vocabulary the interactive frontend owns and
//! dispatches: parsing, the dispatch catalog, and its rendering.

/// One parsed slash-command line: the shared vocabulary both loops
/// dispatch on (T4 consolidation — one match, two writers).
///
/// The stdio loop renders through `writer`; the TUI loop renders through
/// the `TuiSink`. The parse is identical; only the sink differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlashCommand<'a> {
    /// `/context` — no arguments.
    Context,
    /// `/tools` — no arguments.
    Tools,
    /// `/modules` — no arguments.
    Modules,
    /// `/exit` — no arguments.
    Exit,
    /// `/modules-add` with optional folder argument (`None` = bare command).
    ModulesAdd(Option<&'a str>),
    /// `/modules-enable` with optional module id (`None` = bare command).
    ModulesEnable(Option<&'a str>),
    /// `/modules-activate` with optional module id (`None` = bare command).
    ModulesActivate(Option<&'a str>),
    /// `/provider` — display-only (U7).
    Provider,
    /// `/provider remove` — remove the configured provider (confirmed).
    ProviderRemove,
    /// `/model` with optional model id (`None` = bare command).
    /// Bare `/model` shows the applied model (U7); `/model <id>` switches
    /// the live session model and persists it to the workspace `[profile]`.
    Model(Option<&'a str>),
    /// `/models` — list provider models (I6, blocking GET).
    Models,
    /// `/reload` — re-read profile and report what WOULD change (safe half:
    /// never mutates live state; a later chunk does the swap).
    Reload,
    /// `/mouse` — flip TUI mouse capture (TUI) / honesty line (stdio).
    Mouse,
    /// `/ui-list` — list UI panel components (ADR 0039).
    UiList,
    /// `/ui-add` — add a UI panel component (TUI form; stdio prints usage).
    UiAdd,
    /// `/ui-edit` with optional component id (`None` = bare command).
    UiEdit(Option<&'a str>),
    /// `/ui-remove` with optional component id (`None` = bare command).
    UiRemove(Option<&'a str>),
    /// `/ui-preview` with optional component id (`None` = bare command).
    UiPreview(Option<&'a str>),
    /// `/undo` with optional checkpoint id (`None` = bare command:
    /// list restorable checkpoints).
    Undo(Option<&'a str>),
    /// `/undo --last` — restore the most recent restorable checkpoint.
    UndoLast,
    /// `/evolve` — display-only (U8).
    Evolve,
    /// Anything else: a prompt for the application.
    Prompt(&'a str),
}

/// Ordered catalog over the SAME `SlashCommand` vocabulary (I2/I6/I7).
///
/// This is the SINGLE vocabulary source the palette and unknown-command
/// honesty derive from — no parallel list. Order is pinned.
#[must_use]
pub fn slash_command_catalog() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/context", "Show context projection"),
        ("/tools", "List available tools"),
        ("/modules", "List installed modules"),
        ("/modules-add", "Add a module"),
        ("/modules-enable", "Enable a module"),
        ("/modules-activate", "Activate a module"),
        ("/provider", "Show applied provider"),
        ("/provider remove", "Remove configured provider"),
        ("/model", "Show applied model"),
        ("/model <id>", "Switch applied model"),
        ("/models", "List available models"),
        ("/reload", "Re-read profile and report what would change"),
        ("/mouse", "Toggle mouse capture (wheel scroll / text select)"),
        ("/ui-list", "List UI panel components"),
        ("/ui-add", "Add a UI panel component"),
        ("/ui-edit", "Edit a UI panel component"),
        ("/ui-remove", "Remove a UI panel component"),
        ("/ui-preview", "Preview a UI panel component"),
        ("/undo", "List restorable checkpoints"),
        ("/undo <id>", "Restore a checkpoint preimage"),
        ("/undo --last", "Restore the most recent restorable checkpoint"),
        ("/evolve", "Show Stage 6 evolution surfaces"),
        ("/exit", "Exit the session"),
    ]
}

/// Host-generated evolve discovery listing (U8) — four bounded Stage 6 surfaces.
pub(super) fn render_evolve_lines() -> String {
    let mut out = String::new();
    out.push_str("Stage 6 evolution surfaces (bounded, host-gated):\n");
    out.push_str("  corpus — evaluation corpus & baselines\n");
    out.push_str(
        "  workflow — baseline → candidate → evaluation → comparison\n",
    );
    out.push_str("  proposal — skill/module/host proposals\n");
    out.push_str("  packaging — release stabilization\n");
    out.push_str("Execution is host-gated (escalation Profile->Host per Stage 6 design).\n");
    out
}

/// Parse one trimmed input line into the shared [`SlashCommand`]
/// vocabulary — the SINGLE parser both loops call (T4 consolidation).
///
/// Empty input is the caller's no-op (both loops skip it before parsing);
/// this function maps every other line, including the bare/prefixed
/// `/modules-*` pairs, to exactly one variant.
pub(super) fn parse_slash_command(input: &str) -> SlashCommand<'_> {
    match input {
        "/context" => SlashCommand::Context,
        "/tools" => SlashCommand::Tools,
        "/modules" => SlashCommand::Modules,
        "/provider" => SlashCommand::Provider,
        "/provider remove" => SlashCommand::ProviderRemove,
        "/mouse" => SlashCommand::Mouse,
        "/ui-list" => SlashCommand::UiList,
        "/ui-add" => SlashCommand::UiAdd,
        "/ui-edit" => SlashCommand::UiEdit(None),
        "/ui-remove" => SlashCommand::UiRemove(None),
        "/ui-preview" => SlashCommand::UiPreview(None),
        "/undo" => SlashCommand::Undo(None),
        "/undo --last" => SlashCommand::UndoLast,
        "/model" => SlashCommand::Model(None),
        "/models" => SlashCommand::Models,
        "/reload" => SlashCommand::Reload,
        "/evolve" => SlashCommand::Evolve,
        "/exit" => SlashCommand::Exit,
        _ => {
            if input == "/modules-add" {
                SlashCommand::ModulesAdd(None)
            } else if let Some(folder) = input.strip_prefix("/modules-add ") {
                SlashCommand::ModulesAdd(Some(folder))
            } else if input == "/modules-enable" {
                SlashCommand::ModulesEnable(None)
            } else if let Some(id) = input.strip_prefix("/modules-enable ") {
                SlashCommand::ModulesEnable(Some(id))
            } else if input == "/modules-activate" {
                SlashCommand::ModulesActivate(None)
            } else if let Some(id) = input.strip_prefix("/modules-activate ") {
                SlashCommand::ModulesActivate(Some(id))
            } else if let Some(id) = input.strip_prefix("/ui-edit ") {
                SlashCommand::UiEdit(Some(id))
            } else if let Some(id) = input.strip_prefix("/ui-remove ") {
                SlashCommand::UiRemove(Some(id))
            } else if let Some(id) = input.strip_prefix("/ui-preview ") {
                SlashCommand::UiPreview(Some(id))
            } else if input.starts_with("/undo --last ") {
                // `--last` takes no arguments: anything beyond the exact
                // form stays an unknown command (honesty gate), matching
                // `/models` and `/reload` above.
                SlashCommand::Prompt(input)
            } else if let Some(id) = input.strip_prefix("/undo ") {
                SlashCommand::Undo(Some(id))
            } else if input.starts_with("/models") {
                // `/models` takes no arguments: anything beyond the exact
                // form stays an unknown command (honesty gate), exactly as
                // before the `/model <id>` form existed.
                SlashCommand::Prompt(input)
            } else if input.starts_with("/reload") {
                // `/reload` takes no arguments: anything beyond the exact
                // form stays an unknown command (honesty gate), matching
                // `/models` above.
                SlashCommand::Prompt(input)
            } else if let Some(id) = input.strip_prefix("/model ") {
                SlashCommand::Model(Some(id))
            } else {
                SlashCommand::Prompt(input)
            }
        }
    }
}

/// Returns `true` when `line` is an unknown slash command.
///
/// A line is unknown when it starts with `/` after trimming and parses to
/// [`SlashCommand::Prompt`] (the audit's chunk-1 M2 minimal fix). This is
/// the SINGLE unknown-command definition both frontends call (decision 114
/// Q3) — the TUI loop keeps its honesty behavior through this helper and
/// the stdio loop gains the same gate instead of falling through to the
/// prompt path. Compatibility note: a stdio user piping `/-prefixed` prompts
/// loses that ability for non-command strings; the user accepted this
/// ruling (decision 114 Q3).
pub fn is_unknown_slash_command(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return false;
    }
    matches!(parse_slash_command(trimmed), SlashCommand::Prompt(_))
}
