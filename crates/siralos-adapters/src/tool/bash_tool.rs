//! The `bash` default Tool adapter over the sandbox backend port.
//!
//! `bash` runs one general shell command under sandbox enforcement:
//! digest-bound per-action approval (decided by the Host application
//! executor, never here), an explicit environment allowlist (never
//! the parent environment verbatim), network denial, closed stdin,
//! process-tree confinement, wall-clock timeout kill, bounded
//! stdout/stderr with explicit truncation flags, cancellation
//! mid-run, and all output through the terminal sanitizer at the
//! display boundary. The tool launches nothing itself: it probes
//! the backend first and reports the typed refusal wherever a live
//! probe does not prove enforcement — today that is every platform,
//! because no enforcing backend is pinned in this repository.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use siralos_core::identity::sha256_hex;
use siralos_core::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use siralos_core::runtime::sandbox::{SandboxAvailability, SandboxBackend};
use siralos_core::tool::{
    ApprovalPreview, CapabilityId, MAX_APPROVAL_PREVIEW_LINE_CHARS, Tool,
    action_digest, canonical_input_json,
};

use crate::process::sandbox_backend::{
    HostSandboxBackend, build_sandbox_env, truncate_bounded,
};
use crate::workspace::root::{WorkspaceRootError, resolve_workspace_root};

/// Tool name bound into approvals, digests, and checkpoints.
pub const BASH_TOOL_NAME: &str = "bash";
/// Capability guarding command execution.
pub const BASH_CAPABILITY: &str = "process.execute";
/// Maximum accepted command bytes: proposals are bounded before any
/// approval, probe, or execution.
pub const MAX_BASH_COMMAND_BYTES: usize = 32_768;
/// Fixed shell program: the model supplies the command word, never
/// the executable. The backend resolves the program; it never
/// re-splits a shell string.
#[cfg(windows)]
const SHELL_PROGRAM: &str = "cmd";
/// Shell flag taking the single command word.
#[cfg(windows)]
const SHELL_FLAG: &str = "/c";
/// Fixed shell program: the model supplies the command word, never
/// the executable. The backend resolves the program; it never
/// re-splits a shell string.
#[cfg(not(windows))]
const SHELL_PROGRAM: &str = "sh";
/// Shell flag taking the single command word.
#[cfg(not(windows))]
const SHELL_FLAG: &str = "-c";

fn bash_capability() -> CapabilityId {
    CapabilityId::parse(BASH_CAPABILITY)
        .expect("process.execute is a valid capability id")
}

/// The exact typed refusal when enforcement is unproven. One
/// platform-stable text on every host: the differential corpus pins
/// it byte-exactly, so per-platform detail lives in the unit tests
/// and ADR 0038 §2, never in the refusal string.
fn unavailable_message(reason: &str) -> String {
    format!(
        "bash execution is unavailable: {reason}. No enforcing \
        backend is pinned in this repository (ADR 0038 §2)."
    )
}

fn parse_command(input: &Value) -> Result<String, ToolExecutionResult> {
    let object = input.as_object().ok_or_else(|| {
        ToolExecutionResult::InvalidInput {
            message: "Input must be an object.".to_owned(),
        }
    })?;
    match object.get("command") {
        Some(Value::String(command)) => Ok(command.clone()),
        Some(_) => Err(ToolExecutionResult::InvalidInput {
            message: "command must be a string.".to_owned(),
        }),
        None => Err(ToolExecutionResult::InvalidInput {
            message: "command is required.".to_owned(),
        }),
    }
}

/// Reviewable preview of a `bash` proposal: the exact command, its
/// content identity, and the enforcement the backend's probe
/// reports today. Read-only over the host backend; the enforcing
/// backend's probe runs its live canary (a sandboxed no-op), never
/// the proposed command.
fn bash_preview(
    backend: &dyn SandboxBackend,
    command: &str,
) -> ApprovalPreview {
    let hex = sha256_hex(command.as_bytes());
    // Character bound is the shared core approval bound; the marker
    // wording stays unit-accurate per tool (characters here, lines
    // in the mutation previews).
    let shown = command
        .chars()
        .take(MAX_APPROVAL_PREVIEW_LINE_CHARS)
        .collect::<String>();
    let mut lines = vec![format!("command: {shown}"), format!("sha256 {hex}")];
    if command.chars().count() > MAX_APPROVAL_PREVIEW_LINE_CHARS {
        lines.push(format!(
            "({} more characters)",
            command.chars().count() - MAX_APPROVAL_PREVIEW_LINE_CHARS
        ));
    }
    match backend.probe() {
        SandboxAvailability::Available(_) => {
            lines.push("enforcement: live probe passed".to_owned());
        }
        SandboxAvailability::Unavailable(reason) => {
            lines.push(format!("enforcement: {reason}"));
        }
    }
    ApprovalPreview { title: format!("bash {}", &hex[..8]), lines }
}

/// The default `bash` Tool: one sandboxed shell command.
///
/// Requires a one-time host approval for the exact command digest;
/// reports typed `SandboxUnavailable` wherever enforcement is
/// unproven. Registered in the interactive session tool set so the
/// refusal is reachable; execution runs only under a backend whose
/// live probe passes.
pub struct BashTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    backend: Box<dyn SandboxBackend>,
    workdir: PathBuf,
}

impl BashTool {
    /// Construct the adapter over a canonicalized workspace root,
    /// which becomes the confined working directory for commands
    /// run under an enforcing backend. The host backend answers
    /// every probe honestly negative, so this posture refuses
    /// without spawning on every platform: the differential corpus
    /// pins it byte-exactly through this constructor. Infallible
    /// execution posture is unchanged: the backend probes lazily at
    /// execution, never at construction.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceRootError`] when the root cannot be
    /// canonicalized.
    pub fn new(root: &Path) -> Result<Self, WorkspaceRootError> {
        Self::with_backend(root, Box::new(HostSandboxBackend))
    }

    /// Construct the adapter over an explicit backend: the
    /// production session selects the enforcing backend
    /// (`crate::process::enforcing_backend::SrtEnforcingBackend`)
    /// where its live probe passes (falling back to the host
    /// refusal), and the differential harness drives scripted
    /// backends through this constructor to pin the runner mapping
    /// deterministically. The backend probes lazily at execution,
    /// never at construction.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceRootError`] when the root cannot be
    /// canonicalized.
    pub fn with_backend(
        root: &Path,
        backend: Box<dyn SandboxBackend>,
    ) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: BASH_TOOL_NAME.to_owned(),
                description:
                    "Run one shell command under sandbox enforcement. Requires a one-time host approval for the exact command; reports typed unavailable where enforcement is unproven."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Exact shell command to run under sandbox enforcement."
                        }
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }),
            },
            capability: bash_capability(),
            backend,
            workdir: resolve_workspace_root(root)?,
        })
    }
}

impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn approval_preview(&self, input: &Value) -> Option<ApprovalPreview> {
        let command = parse_command(input).ok()?;
        if command.len() > MAX_BASH_COMMAND_BYTES {
            return None;
        }
        Some(bash_preview(&*self.backend, &command))
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        execute_bash(&*self.backend, &self.workdir, input, cancellation)
    }
}

/// Shared execution behind `bash`: validate, honor cancellation,
/// probe enforcement, and only then run bounded.
fn execute_bash(
    backend: &dyn SandboxBackend,
    workdir: &Path,
    input: &Value,
    cancellation: CancellationSignal<'_>,
) -> ToolExecutionResult {
    let command = match parse_command(input) {
        Ok(command) => command,
        Err(result) => return result,
    };
    if command.len() > MAX_BASH_COMMAND_BYTES {
        return ToolExecutionResult::InvalidInput {
            message: format!(
                "Command is too large ({} bytes; limit {}).",
                command.len(),
                MAX_BASH_COMMAND_BYTES
            ),
        };
    }
    if cancellation.is_cancelled() {
        return ToolExecutionResult::Cancelled {
            message: "Command was cancelled.".to_owned(),
        };
    }
    let availability = backend.probe();
    match availability {
        SandboxAvailability::Available(_) => {
            // Unreachable while no probe passes: the arm below runs
            // only under proven enforcement. It is implemented (not
            // stubbed) so the day a backend exists, the bounds
            // already hold.
            run_bounded(backend, workdir, input, &command, cancellation)
        }
        SandboxAvailability::Unavailable(reason) => {
            ToolExecutionResult::SandboxUnavailable {
                message: unavailable_message(reason.message()),
            }
        }
    }
}

/// Run under proven enforcement with explicit environment, limits,
/// and truncation flags. Cancellation observed before the outcome
/// is recorded wins over completion.
fn run_bounded(
    backend: &dyn SandboxBackend,
    workdir: &Path,
    input: &Value,
    command: &str,
    cancellation: CancellationSignal<'_>,
) -> ToolExecutionResult {
    use siralos_core::runtime::sandbox::SandboxLimits;
    let limits = SandboxLimits::default_tool_limits();
    let parent: Vec<(String, String)> = std::env::vars().collect();
    // No private run directories exist yet, so no controlled
    // home/temp overrides are supplied: the fixed values still
    // apply, and the parent contributes allowlist names only.
    let env = build_sandbox_env(&parent, &[]);
    let outcome = match backend.execute(
        &siralos_core::runtime::sandbox::SandboxCommand {
            program: SHELL_PROGRAM.to_owned(),
            args: vec![SHELL_FLAG.to_owned(), command.to_owned()],
            env,
            workdir: workdir.to_path_buf(),
        },
        &limits,
        cancellation,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return ToolExecutionResult::Failed {
                message: error.message().to_owned(),
            };
        }
    };
    if outcome.cancelled || cancellation.is_cancelled() {
        return ToolExecutionResult::Cancelled {
            message: "Command was cancelled.".to_owned(),
        };
    }
    if outcome.timed_out {
        return ToolExecutionResult::TimedOut {
            message: "Command exceeded its wall-clock limit.".to_owned(),
        };
    }
    let (stdout, stdout_truncated) =
        truncate_bounded(&outcome.stdout, limits.stdout_max_bytes);
    let (stderr, stderr_truncated) =
        truncate_bounded(&outcome.stderr, limits.stderr_max_bytes);
    let digest =
        action_digest(BASH_TOOL_NAME, &canonical_input_json(input), None);
    let status = outcome.status_code;
    let mut summary = match status {
        Some(0) => "bash exited with status 0".to_owned(),
        Some(code) => format!("bash exited with status {code}"),
        None => "bash ended without a status code".to_owned(),
    };
    if stdout_truncated || stderr_truncated {
        summary.push_str(" (output truncated)");
    }
    let output = json!({
        "command_digest": digest.as_str(),
        "status": status,
        "timed_out": outcome.timed_out,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": String::from_utf8_lossy(&stderr),
    });
    match status {
        Some(0) => ToolExecutionResult::Success { output, summary },
        _ => ToolExecutionResult::Failed { message: summary },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BASH_TOOL_NAME, BashTool, MAX_BASH_COMMAND_BYTES, unavailable_message,
    };
    use serde_json::json;
    use siralos_core::provider::{CancellationToken, ToolExecutionResult};
    use siralos_core::tool::Tool;

    fn tool() -> BashTool {
        BashTool::new(&std::env::temp_dir()).expect("test root resolves")
    }

    #[test]
    fn bash_requires_approval_and_advertises_the_contract() {
        use siralos_core::tool::CapabilityId;
        let tool = tool();
        assert!(tool.requires_approval());
        assert_eq!(tool.definition().name, BASH_TOOL_NAME);
        assert_eq!(
            tool.capability(),
            &CapabilityId::parse(super::BASH_CAPABILITY)
                .expect("test capability parses")
        );
    }

    #[test]
    fn malformed_inputs_are_invalid_before_any_probe() {
        let tool = tool();
        let token = CancellationToken::new();
        for input in [json!({}), json!({"command": 42}), json!("bash")] {
            assert!(
                matches!(
                    tool.execute(&input, token.signal()),
                    ToolExecutionResult::InvalidInput { .. }
                ),
                "{input}"
            );
            assert!(tool.approval_preview(&input).is_none());
        }
        // Unknown fields are ignored, exactly like the mutation
        // tools: the schema documents the contract, the parser reads
        // the known fields, and nothing unknown influences execution.
        let with_extra = json!({"command": "ok", "extra": true});
        assert!(
            matches!(
                tool.execute(&with_extra, token.signal()),
                ToolExecutionResult::SandboxUnavailable { .. }
            ),
            "{with_extra}"
        );
        let oversize = "x".repeat(MAX_BASH_COMMAND_BYTES + 1);
        assert!(matches!(
            tool.execute(&json!({"command": oversize}), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(
            tool.approval_preview(&json!({"command": oversize})).is_none()
        );
    }

    #[test]
    fn preview_binds_the_exact_command_bytes() {
        let tool = tool();
        let preview = tool
            .approval_preview(&json!({"command": "echo hi"}))
            .expect("reviewable");
        let text = preview.lines.join("\n");
        assert!(text.contains("echo hi"), "{text:?}");
        assert!(text.contains("sha256"), "{text:?}");
        assert!(text.contains("enforcement:"), "{text:?}");
    }

    #[test]
    fn cancelled_calls_refuse_before_any_probe() {
        use siralos_core::provider::CancellationToken;
        let tool = tool();
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            tool.execute(
                &json!({"command": "echo never-runs"}),
                token.signal()
            ),
            ToolExecutionResult::Cancelled { .. }
        ));
    }

    #[test]
    fn execution_without_enforcement_is_typed_unavailable() {
        let tool = tool();
        let token = CancellationToken::new();
        let refused = tool
            .execute(&json!({"command": "echo never-runs"}), token.signal());
        assert!(
            matches!(refused, ToolExecutionResult::SandboxUnavailable { .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn unavailability_names_the_absent_backend() {
        let message = unavailable_message(
            "the backend cannot express the required enforcement",
        );
        assert!(
            message.contains("bash execution is unavailable")
                && message.contains("No enforcing backend is pinned"),
            "{message:?}"
        );
    }

    #[test]
    fn outcome_mapping_flags_truncation_and_timeout() {
        use siralos_core::runtime::sandbox::{
            SandboxBackend, SandboxCommand, SandboxLimits, SandboxOutput,
        };
        struct Scripted(SandboxOutput);
        impl SandboxBackend for Scripted {
            fn probe(
                &self,
            ) -> siralos_core::runtime::sandbox::SandboxAvailability
            {
                siralos_core::runtime::sandbox::SandboxAvailability::Available(
                    siralos_core::runtime::sandbox::SandboxEnforcement::full(),
                )
            }
            fn execute(
                &self,
                _command: &SandboxCommand,
                _limits: &SandboxLimits,
                _cancellation: siralos_core::provider::CancellationSignal<'_>,
            ) -> Result<
                SandboxOutput,
                siralos_core::runtime::sandbox::SandboxError,
            > {
                Ok(SandboxOutput {
                    status_code: self.0.status_code,
                    stdout: self.0.stdout.clone(),
                    stderr: self.0.stderr.clone(),
                    stdout_truncated: self.0.stdout_truncated,
                    stderr_truncated: self.0.stderr_truncated,
                    timed_out: self.0.timed_out,
                    cancelled: self.0.cancelled,
                })
            }
        }
        let token = CancellationToken::new();
        let input = json!({"command": "echo hi"});
        // Timeout wins over status.
        let timed = super::run_bounded(
            &Scripted(SandboxOutput {
                status_code: Some(0),
                stdout: b"partial".to_vec(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: true,
                cancelled: false,
            }),
            &std::env::temp_dir(),
            &input,
            "echo hi",
            token.signal(),
        );
        assert!(
            matches!(timed, ToolExecutionResult::TimedOut { .. }),
            "{timed:?}"
        );
        // Nonzero status fails even with exact bytes.
        let failed = super::run_bounded(
            &Scripted(SandboxOutput {
                status_code: Some(3),
                stdout: b"out".to_vec(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: false,
                cancelled: false,
            }),
            &std::env::temp_dir(),
            &input,
            "echo hi",
            token.signal(),
        );
        assert!(
            matches!(failed, ToolExecutionResult::Failed { .. }),
            "{failed:?}"
        );
        // Cancellation observed at the boundary wins.
        let cancelled = super::run_bounded(
            &Scripted(SandboxOutput {
                status_code: Some(0),
                stdout: b"out".to_vec(),
                stderr: Vec::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                timed_out: false,
                cancelled: true,
            }),
            &std::env::temp_dir(),
            &input,
            "echo hi",
            token.signal(),
        );
        assert!(
            matches!(cancelled, ToolExecutionResult::Cancelled { .. }),
            "{cancelled:?}"
        );
    }

    #[test]
    fn constructed_command_is_shell_shaped_with_confined_workdir() {
        // The contract shape, asserted on the real construction
        // path: the fixed shell is the program, the model command
        // is one argument word (never a re-split shell string), and
        // the workdir is the workspace root (never an unconfined
        // host default). This code runs at the enforcement
        // milestone; it must be right before then.
        use std::cell::RefCell;
        use std::rc::Rc;

        use siralos_core::runtime::sandbox::{
            SandboxBackend, SandboxCommand, SandboxEnforcement, SandboxLimits,
            SandboxOutput,
        };
        struct Capturing {
            seen: Rc<RefCell<Option<SandboxCommand>>>,
        }
        impl SandboxBackend for Capturing {
            fn probe(
                &self,
            ) -> siralos_core::runtime::sandbox::SandboxAvailability
            {
                siralos_core::runtime::sandbox::SandboxAvailability::Available(
                    SandboxEnforcement::full(),
                )
            }
            fn execute(
                &self,
                command: &SandboxCommand,
                _limits: &SandboxLimits,
                _cancellation: siralos_core::provider::CancellationSignal<'_>,
            ) -> Result<
                SandboxOutput,
                siralos_core::runtime::sandbox::SandboxError,
            > {
                *self.seen.borrow_mut() = Some(command.clone());
                Ok(SandboxOutput {
                    status_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    timed_out: false,
                    cancelled: false,
                })
            }
        }
        let seen: Rc<RefCell<Option<SandboxCommand>>> =
            Rc::new(RefCell::new(None));
        let backend = Capturing { seen: Rc::clone(&seen) };
        let root = std::env::temp_dir().join("siralos-bash-shape");
        std::fs::create_dir_all(&root).expect("shape fixture");
        let canonical =
            std::fs::canonicalize(&root).expect("shape root resolves");
        let token = CancellationToken::new();
        let input = json!({"command": "echo hi"});
        let outcome = super::run_bounded(
            &backend,
            &canonical,
            &input,
            "echo hi",
            token.signal(),
        );
        assert!(
            matches!(outcome, ToolExecutionResult::Success { .. }),
            "{outcome:?}"
        );
        let command = seen.borrow().clone().expect("backend ran");
        assert_eq!(command.program, super::SHELL_PROGRAM);
        assert_eq!(
            command.args,
            vec![super::SHELL_FLAG.to_owned(), "echo hi".to_owned()]
        );
        assert_ne!(command.program, "echo hi");
        assert_eq!(command.workdir, canonical);
        assert_ne!(command.workdir, std::env::temp_dir());
        let _ = std::fs::remove_dir_all(&root);
    }
}
