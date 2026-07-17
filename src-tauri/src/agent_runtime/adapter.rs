//! Runtime backend が実装する object-safe な操作契約。

use super::{BackendKind, RuntimeCapability};

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
    fn spawn_turn(&self, request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError>;
    fn write(&self, data: &str) -> Result<(), RuntimeAdapterError>;
    fn stop(&self) -> Result<(), RuntimeAdapterError>;
    fn dispose(&self) -> Result<(), RuntimeAdapterError>;
}
