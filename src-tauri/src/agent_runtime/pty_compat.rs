//! Existing portable-pty sessions exposed through AgentRuntimeAdapter.

use super::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability,
    RuntimeDeliveryFuture, RuntimeDeliveryRequest, RuntimeSessionSpawnRequest,
    RuntimeTurnSpawnRequest,
};
use crate::pty::{session::TerminationReason, SessionRegistry, UserWriteOutcome};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Ownership contract: PtyCompatAdapter does not own the session; the terminal layer owns the
/// session lifetime. Runtime endpoint disposal only detaches this adapter and must never remove
/// the wrapped session from [`SessionRegistry`].
pub struct PtyCompatAdapter {
    registry: Arc<SessionRegistry>,
    target: PtyTarget,
    disposed: AtomicBool,
}

enum PtyTarget {
    Session(String),
    TeamAgent { agent_id: String },
}

impl PtyCompatAdapter {
    pub fn new(registry: Arc<SessionRegistry>, session_id: impl Into<String>) -> Self {
        Self {
            registry,
            target: PtyTarget::Session(session_id.into()),
            disposed: AtomicBool::new(false),
        }
    }

    pub fn for_team_agent(registry: Arc<SessionRegistry>, agent_id: impl Into<String>) -> Self {
        Self {
            registry,
            target: PtyTarget::TeamAgent {
                agent_id: agent_id.into(),
            },
            disposed: AtomicBool::new(false),
        }
    }

    fn session(&self) -> Result<Arc<crate::pty::session::SessionHandle>, RuntimeAdapterError> {
        if self.disposed.load(Ordering::Acquire) {
            return Err(RuntimeAdapterError::new(
                "runtime_endpoint_disposed",
                "runtime endpoint has been disposed",
                false,
            ));
        }
        let session = match &self.target {
            PtyTarget::Session(session_id) => self.registry.get(session_id),
            PtyTarget::TeamAgent { agent_id } => self.registry.get_by_agent(agent_id),
        };
        session.ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_pty_session_not_found",
                "PTY session was not found",
                false,
            )
        })
    }

    fn write_bytes(&self, data: &[u8]) -> Result<(), RuntimeAdapterError> {
        let outcome = self.session()?.user_write(data).map_err(|error| {
            RuntimeAdapterError::new("runtime_pty_write_failed", error.to_string(), true)
        })?;
        match outcome {
            UserWriteOutcome::Written => Ok(()),
            UserWriteOutcome::SuppressedInjecting => Err(RuntimeAdapterError::new(
                "runtime_pty_write_suppressed",
                "PTY input is temporarily suppressed while an inject is active",
                true,
            )),
            UserWriteOutcome::DroppedTooLarge => Err(RuntimeAdapterError::new(
                "runtime_pty_write_too_large",
                "PTY input exceeded the per-call size limit",
                true,
            )),
            UserWriteOutcome::DroppedRateLimited => Err(RuntimeAdapterError::new(
                "runtime_pty_write_rate_limited",
                "PTY input exceeded the per-session rate limit",
                true,
            )),
        }
    }
}

impl AgentRuntimeAdapter for PtyCompatAdapter {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Pty
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        vec![RuntimeCapability::PtyExecution]
    }

    fn spawn_session(
        &self,
        _request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        match &self.target {
            PtyTarget::Session(_) => self.session().map(|_| ()),
            PtyTarget::TeamAgent { .. } => Ok(()),
        }
    }

    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        if request.submit {
            let mut input = request.input.as_bytes().to_vec();
            input.push(b'\r');
            self.write_bytes(&input)
        } else {
            self.write(&request.input)
        }
    }

    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.write_bytes(data.as_bytes())
    }

    fn deliver<'a>(&'a self, request: &'a RuntimeDeliveryRequest) -> RuntimeDeliveryFuture<'a> {
        Box::pin(async move {
            match &self.target {
                PtyTarget::Session(_) => self.write(&request.data),
                PtyTarget::TeamAgent { agent_id, .. } => crate::team_hub::inject::inject(
                    self.registry.clone(),
                    agent_id,
                    &request.from_role,
                    &request.data,
                )
                .await
                .map_err(|error| {
                    // NoSession / SessionReplaced は endpoint の死を意味する。
                    // recoverable=false で deliver_team_message に detach させ、
                    // 再 spawn 時に stale adapter が残らないようにする (PR #34 一次レビュー 🟡6)。
                    let recoverable = !matches!(
                        error,
                        crate::team_hub::inject::InjectError::NoSession
                            | crate::team_hub::inject::InjectError::SessionReplaced { .. }
                    );
                    RuntimeAdapterError::new(error.code(), error.to_string(), recoverable)
                }),
            }
        })
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        self.session()?
            .kill(TerminationReason::UserClose)
            .map_err(|error| {
                RuntimeAdapterError::new("runtime_pty_stop_failed", error.to_string(), false)
            })
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        self.disposed.store(true, Ordering::Release);
        Ok(())
    }
}
