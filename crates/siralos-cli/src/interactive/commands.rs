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
    /// `/domains` — no arguments.
    Domains,
    /// `/exit` — no arguments.
    Exit,
    /// `/domains-add` with optional folder argument (`None` = bare command).
    DomainsAdd(Option<&'a str>),
    /// `/domains-enable` with optional plugin id (`None` = bare command).
    DomainsEnable(Option<&'a str>),
    /// `/domains-activate` with optional plugin id (`None` = bare command).
    DomainsActivate(Option<&'a str>),
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
        ("/domains", "List installed domains"),
        ("/domains-add", "Add a domain plugin"),
        ("/domains-enable", "Enable a domain plugin"),
        ("/domains-activate", "Activate a domain plugin"),
        ("/provider", "Show applied provider"),
        ("/provider remove", "Remove configured provider"),
        ("/model", "Show applied model"),
        ("/model <id>", "Switch applied model"),
        ("/models", "List available models"),
        ("/reload", "Re-read profile and report what would change"),
        ("/mouse", "Toggle mouse capture (wheel scroll / text select)"),
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
    out.push_str("  proposal — skill/plugin/host proposals\n");
    out.push_str("  packaging — release stabilization\n");
    out.push_str("Execution is host-gated (escalation Profile->Host per Stage 6 design).\n");
    out
}

/// Parse one trimmed input line into the shared [`SlashCommand`]
/// vocabulary — the SINGLE parser both loops call (T4 consolidation).
///
/// Empty input is the caller's no-op (both loops skip it before parsing);
/// this function maps every other line, including the bare/prefixed
/// `/domains-*` pairs, to exactly one variant.
pub(super) fn parse_slash_command(input: &str) -> SlashCommand<'_> {
    match input {
        "/context" => SlashCommand::Context,
        "/tools" => SlashCommand::Tools,
        "/domains" => SlashCommand::Domains,
        "/provider" => SlashCommand::Provider,
        "/provider remove" => SlashCommand::ProviderRemove,
        "/mouse" => SlashCommand::Mouse,
        "/model" => SlashCommand::Model(None),
        "/models" => SlashCommand::Models,
        "/reload" => SlashCommand::Reload,
        "/evolve" => SlashCommand::Evolve,
        "/exit" => SlashCommand::Exit,
        _ => {
            if input == "/domains-add" {
                SlashCommand::DomainsAdd(None)
            } else if let Some(folder) = input.strip_prefix("/domains-add ") {
                SlashCommand::DomainsAdd(Some(folder))
            } else if input == "/domains-enable" {
                SlashCommand::DomainsEnable(None)
            } else if let Some(id) = input.strip_prefix("/domains-enable ") {
                SlashCommand::DomainsEnable(Some(id))
            } else if input == "/domains-activate" {
                SlashCommand::DomainsActivate(None)
            } else if let Some(id) = input.strip_prefix("/domains-activate ") {
                SlashCommand::DomainsActivate(Some(id))
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
