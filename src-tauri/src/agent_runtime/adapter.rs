//! Runtime backend が実装する object-safe な操作契約。

use super::{BackendKind, RuntimeCapability};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionSpawnRequest {
    pub endpoint_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTurnSpawnRequest {
    pub input: String,
    pub submit: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionResumeRequest {
    pub thread_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSessionForkRequest {
    pub thread_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSteerRequest {
    pub input: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeApprovalResponseRequest {
    pub request_id: String,
    pub decision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDeliveryRequest {
    pub data: String,
    pub from_role: String,
}

pub type RuntimeDeliveryFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), RuntimeAdapterError>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAdapterError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl RuntimeAdapterError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }
}

impl std::fmt::Display for RuntimeAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeAdapterError {}

fn unsupported(operation: &str) -> RuntimeAdapterError {
    RuntimeAdapterError::new(
        "runtime_operation_unsupported",
        format!("runtime adapter does not support {operation}"),
        true,
    )
}

/// Runtime の session / turn / input / lifecycle を上位層から分離する契約。
///
/// 現行 PTY 操作は同期 API なので object-safe な同期 trait とする。native adapter が async I/O
/// を必要とする場合は adapter 内部の task/channel へ enqueue し、この境界は変えずに保てる。
pub trait AgentRuntimeAdapter: Send + Sync {
    fn backend_kind(&self) -> BackendKind;
    #[allow(dead_code)]
    fn capabilities(&self) -> Vec<RuntimeCapability>;
    fn spawn_session(
        &self,
        request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError>;
    fn resume_session(
        &self,
        _request: &RuntimeSessionResumeRequest,
    ) -> Result<(), RuntimeAdapterError> {
        Err(unsupported("session resume"))
    }
    fn fork_session(
        &self,
        _request: &RuntimeSessionForkRequest,
    ) -> Result<(), RuntimeAdapterError> {
        Err(unsupported("session fork"))
    }
    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError>;
    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError>;
    fn inject(&self, data: &str) -> Result<(), RuntimeAdapterError> {
        self.write(data)
    }
    fn deliver<'a>(&'a self, request: &'a RuntimeDeliveryRequest) -> RuntimeDeliveryFuture<'a> {
        Box::pin(async move { self.write(&request.data) })
    }
    fn steer(&self, _request: &RuntimeSteerRequest) -> Result<(), RuntimeAdapterError> {
        Err(unsupported("turn steer"))
    }
    fn interrupt(&self) -> Result<(), RuntimeAdapterError> {
        Err(unsupported("turn interrupt"))
    }
    fn respond_approval(
        &self,
        _request: &RuntimeApprovalResponseRequest,
    ) -> Result<(), RuntimeAdapterError> {
        Err(unsupported("approval responses"))
    }
    fn stop(&self) -> Result<(), RuntimeAdapterError>;
    fn dispose(&self) -> Result<(), RuntimeAdapterError>;
}
