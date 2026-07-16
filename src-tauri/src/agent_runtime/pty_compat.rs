//! Existing portable-pty sessions exposed through AgentRuntimeAdapter.

use super::{
    AgentRuntimeAdapter, BackendKind, RuntimeAdapterError, RuntimeCapability,
    RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
use crate::pty::{session::TerminationReason, SessionRegistry, UserWriteOutcome};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Ownership contract: PtyCompatAdapter does not own the session; the terminal layer owns the
/// session lifetime. Runtime endpoint disposal only detaches this adapter and must never remove
/// the wrapped session from [`SessionRegistry`].
pub struct PtyCompatAdapter {
    registry: Arc<SessionRegistry>,
    session_id: String,
    disposed: AtomicBool,
}

impl PtyCompatAdapter {
    pub fn new(registry: Arc<SessionRegistry>, session_id: impl Into<String>) -> Self {
        Self {
            registry,
            session_id: session_id.into(),
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
        self.registry.get(&self.session_id).ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_pty_session_not_found",
                format!("PTY session '{}' was not found", self.session_id),
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
        self.session().map(|_| ())
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
