//! Host-mediated effect boundary (Stage 3R R6).
//!
//! Every effect a domain component requests crosses this adapter. The
//! host validates the request, checks the active capability grant,
//! applies its bounds, and returns the typed answer; the domain
//! receives only the result. The workspace read reuses the production
//! containment-safe bounded reader; process/runtime execution is
//! denied by policy. A denial is typed and never escalates.

use siralos_core::domain::capability::CapabilityGrant;
use siralos_core::domain::failure::ResourceExceededKind;

use crate::workspace::read::{ReadInput, ReadMode, read_file};
use crate::workspace::resolve::resolve_workspace_path;

use siralos_core::workspace::bounds::{WORKSPACE_LIMITS, WorkspaceLimits};

/// Capability ids understood by the effect boundary. The WIT world
/// declares the same vocabulary structurally.
pub const CAPABILITY_WORKSPACE_READ: &str = "workspace-read";
pub const CAPABILITY_PROCESS_EXEC: &str = "process-exec";

/// Bounds applied by the effect mediator.
#[derive(Debug, Clone, Copy)]
pub struct EffectMediationBounds {
    /// Maximum bytes returned in one mediated answer.
    pub max_answer_bytes: usize,
    /// Maximum workspace file size a mediated read may return.
    pub max_workspace_read_bytes: u64,
    /// Maximum host-mediated calls per activation session.
    pub max_host_calls: u32,
}

/// The typed outcome of a mediated effect request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediatedAnswer {
    /// The effect succeeded with bounded content.
    Ok(String),
    /// The Host policy denied the effect.
    Denied(String),
    /// The request was cancelled.
    Cancelled,
    /// The effect failed with a typed reason.
    Error(String),
}

/// The typed outcome of one mediation attempt. The guest-visible answer
/// is separate from the Host-observed resource classification, so a
/// resource failure never degrades into a prose string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectMediation {
    /// A normal guest-visible answer.
    Answer(MediatedAnswer),
    /// A Host resource bound was exceeded (for example the Host-call
    /// budget); the guest still receives a bounded disposition, but the
    /// Host observes the typed resource failure.
    ResourceExceeded(ResourceExceededKind),
}

/// The host-side mediator state for one activation session: the
/// effective grant, the workspace root, and the bounds. The call
/// counter enforces the host-call budget.
pub struct EffectMediator {
    grant: CapabilityGrant,
    root: std::path::PathBuf,
    bounds: EffectMediationBounds,
    remaining_calls: u32,
    cancelled: bool,
}

impl EffectMediator {
    /// Create the mediator for one active grant.
    pub fn new(
        grant: CapabilityGrant,
        root: std::path::PathBuf,
        bounds: EffectMediationBounds,
    ) -> Self {
        Self {
            grant,
            root,
            bounds,
            remaining_calls: bounds.max_host_calls,
            cancelled: false,
        }
    }

    /// Cancel further mediated effects.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Mediate one effect request: capability check, bounds, and the
    /// production workspace read where granted. The domain receives
    /// only the typed answer; the Host observes the typed outcome,
    /// including resource exhaustion.
    pub fn mediate(
        &mut self,
        request: &crate::domain::host::EffectRequest,
    ) -> EffectMediation {
        if self.cancelled {
            return EffectMediation::Answer(MediatedAnswer::Cancelled);
        }
        if self.remaining_calls == 0 {
            return EffectMediation::ResourceExceeded(
                ResourceExceededKind::HostCalls,
            );
        }
        self.remaining_calls -= 1;
        match request {
            crate::domain::host::EffectRequest::WorkspaceRead((
                path,
                max_bytes,
            )) => {
                let capability =
                    match siralos_core::domain::capability::CapabilityId::parse(
                        CAPABILITY_WORKSPACE_READ,
                    ) {
                        Ok(capability) => capability,
                        Err(_) => {
                            return EffectMediation::Answer(
                                MediatedAnswer::Error(
                                    "invalid capability id".to_owned(),
                                ),
                            );
                        }
                    };
                if !self.grant.contains(&capability) {
                    return EffectMediation::Answer(MediatedAnswer::Denied(
                        "workspace-read is not granted".to_owned(),
                    ));
                }
                if path.is_empty() || *max_bytes == 0 {
                    return EffectMediation::Answer(MediatedAnswer::Error(
                        "invalid workspace-read request".to_owned(),
                    ));
                }
                let max_bytes = usize::try_from(
                    u64::from(*max_bytes)
                        .min(self.bounds.max_workspace_read_bytes),
                )
                .unwrap_or(usize::MAX);
                if resolve_workspace_path(&self.root, path).is_err() {
                    return EffectMediation::Answer(MediatedAnswer::Denied(
                        "path is outside the workspace".to_owned(),
                    ));
                }
                let limits = WorkspaceLimits {
                    max_read_file_size_bytes: max_bytes,
                    ..WORKSPACE_LIMITS
                };
                match read_file(
                    &self.root,
                    &ReadInput {
                        path: path.clone(),
                        start_line: 1,
                        end_line: None,
                        mode: ReadMode::Exact,
                    },
                    &limits,
                    None,
                    self.cancelled,
                ) {
                    crate::workspace::read::ReadOutcome::Success {
                        content,
                        ..
                    } => {
                        let mut answer = content;
                        if answer.len() > self.bounds.max_answer_bytes {
                            answer.truncate(self.bounds.max_answer_bytes);
                        }
                        EffectMediation::Answer(MediatedAnswer::Ok(answer))
                    }
                    crate::workspace::read::ReadOutcome::Denied {
                        message,
                    } => EffectMediation::Answer(MediatedAnswer::Denied(
                        message,
                    )),
                    crate::workspace::read::ReadOutcome::Cancelled => {
                        EffectMediation::Answer(MediatedAnswer::Cancelled)
                    }
                    crate::workspace::read::ReadOutcome::Failed {
                        message,
                    } => {
                        EffectMediation::Answer(MediatedAnswer::Error(message))
                    }
                    crate::workspace::read::ReadOutcome::Unsupported {
                        ..
                    } => EffectMediation::Answer(MediatedAnswer::Error(
                        "unsupported read mode".to_owned(),
                    )),
                    crate::workspace::read::ReadOutcome::InvalidInput {
                        ..
                    } => EffectMediation::Answer(MediatedAnswer::Error(
                        "invalid workspace-read request".to_owned(),
                    )),
                }
            }
            crate::domain::host::EffectRequest::ProcessExec(_command) => {
                // Process/runtime execution: the capability check runs
                // exactly like any other effect, and even a granted
                // process-exec cannot execute because no production
                // launcher exists in R6. The denial is typed and never
                // escalates.
                let capability =
                    match siralos_core::domain::capability::CapabilityId::parse(
                        CAPABILITY_PROCESS_EXEC,
                    ) {
                        Ok(capability) => capability,
                        Err(_) => {
                            return EffectMediation::Answer(
                                MediatedAnswer::Error(
                                    "invalid capability id".to_owned(),
                                ),
                            );
                        }
                    };
                if !self.grant.contains(&capability) {
                    return EffectMediation::Answer(MediatedAnswer::Denied(
                        "process-exec is not granted by Host policy"
                            .to_owned(),
                    ));
                }
                EffectMediation::Answer(MediatedAnswer::Error(
                    "process execution is unavailable in this host".to_owned(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CAPABILITY_WORKSPACE_READ, EffectMediation, EffectMediationBounds,
        EffectMediator, MediatedAnswer,
    };
    use siralos_core::domain::capability::HostAuthority;
    use siralos_core::domain::failure::ResourceExceededKind;

    #[test]
    fn process_exec_is_denied_by_policy() {
        let authority =
            HostAuthority::parse(&[CAPABILITY_WORKSPACE_READ.to_owned()])
                .unwrap();
        let request =
            siralos_core::domain::capability::CapabilityRequest::parse(&[
                CAPABILITY_WORKSPACE_READ.to_owned(),
            ])
            .unwrap();
        let grant = match siralos_core::domain::capability::decide_grant(
            &request, &authority,
        ) {
            siralos_core::domain::capability::GrantDecision::Granted(
                grant,
            ) => grant,
            siralos_core::domain::capability::GrantDecision::Denied {
                ..
            } => {
                panic!("fixture grant must succeed");
            }
        };
        let mut mediator = EffectMediator::new(
            grant,
            std::env::temp_dir(),
            EffectMediationBounds {
                max_answer_bytes: 4096,
                max_workspace_read_bytes: 4096,
                max_host_calls: 8,
            },
        );
        let answer = mediator.mediate(
            &crate::domain::host::EffectRequest::ProcessExec(
                "whoami".to_owned(),
            ),
        );
        assert!(matches!(
            answer,
            EffectMediation::Answer(MediatedAnswer::Denied(_))
        ));
    }

    #[test]
    fn host_call_budget_is_bounded() {
        let authority =
            HostAuthority::parse(&[CAPABILITY_WORKSPACE_READ.to_owned()])
                .unwrap();
        let request =
            siralos_core::domain::capability::CapabilityRequest::parse(&[
                CAPABILITY_WORKSPACE_READ.to_owned(),
            ])
            .unwrap();
        let grant = match siralos_core::domain::capability::decide_grant(
            &request, &authority,
        ) {
            siralos_core::domain::capability::GrantDecision::Granted(
                grant,
            ) => grant,
            siralos_core::domain::capability::GrantDecision::Denied {
                ..
            } => {
                panic!("fixture grant must succeed");
            }
        };
        let mut mediator = EffectMediator::new(
            grant,
            std::env::temp_dir(),
            EffectMediationBounds {
                max_answer_bytes: 4096,
                max_workspace_read_bytes: 4096,
                max_host_calls: 2,
            },
        );
        for _ in 0..2 {
            let answer = mediator.mediate(
                &crate::domain::host::EffectRequest::ProcessExec(
                    "x".to_owned(),
                ),
            );
            assert!(matches!(answer, EffectMediation::Answer(_)));
        }
        let exhausted = mediator.mediate(
            &crate::domain::host::EffectRequest::ProcessExec("x".to_owned()),
        );
        // Host-call exhaustion is a typed Host-observed resource
        // failure, never a prose string.
        assert!(matches!(
            exhausted,
            EffectMediation::ResourceExceeded(ResourceExceededKind::HostCalls)
        ));
    }

    #[test]
    fn cancelled_mediation_returns_cancelled() {
        let authority = HostAuthority::parse(&[]).unwrap();
        let request =
            siralos_core::domain::capability::CapabilityRequest::parse(&[])
                .unwrap();
        let grant = match siralos_core::domain::capability::decide_grant(
            &request, &authority,
        ) {
            siralos_core::domain::capability::GrantDecision::Granted(
                grant,
            ) => grant,
            siralos_core::domain::capability::GrantDecision::Denied {
                ..
            } => {
                panic!("empty grant must succeed");
            }
        };
        let mut mediator = EffectMediator::new(
            grant,
            std::env::temp_dir(),
            EffectMediationBounds {
                max_answer_bytes: 4096,
                max_workspace_read_bytes: 4096,
                max_host_calls: 8,
            },
        );
        mediator.cancel();
        let answer = mediator.mediate(
            &crate::domain::host::EffectRequest::ProcessExec("x".to_owned()),
        );
        assert_eq!(answer, EffectMediation::Answer(MediatedAnswer::Cancelled));
    }
}
