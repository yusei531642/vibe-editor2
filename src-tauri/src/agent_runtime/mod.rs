//! Issue #21 / #22: agent runtime の境界。
//!
//! Phase 0 の backend 選択と capability 診断に加え、Phase 1 では adapter / endpoint
//! registry / normalized event envelope を提供する。Phase 2 では Unix の Codex app-server
//! adapter を native backend として公開し、Windows は PTY へ安全に fallback する。

#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod adapter;
pub mod claude_agent;
#[cfg(unix)]
pub mod codex;
#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod delivery;
#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod event;
#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod event_buffer;
#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod manager;
mod persistence;
mod provider_policy;
mod registry;
#[cfg_attr(not(unix), allow(dead_code))] // unix-gated codex 経路からのみ使用
mod pty_compat;

pub use adapter::{
    AgentRuntimeAdapter, RuntimeAdapterError, RuntimeApprovalResponseRequest,
    RuntimeDeliveryFuture, RuntimeDeliveryRequest, RuntimeSessionForkRequest,
    RuntimeSessionResumeRequest, RuntimeSessionSpawnRequest, RuntimeSteerRequest,
    RuntimeTurnSpawnRequest,
};
#[allow(unused_imports)]
pub use event::{
    RuntimeEventEnvelope, RuntimeEventKind, RuntimeEventPayload, RuntimeLifecycleState,
};
pub use event_buffer::{RuntimeEventBuffer, DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY};
#[allow(unused_imports)]
pub use manager::{RuntimeManager, RuntimeOperation};
pub use persistence::{PersistedRuntimeBinding, RuntimeEventPersistence, RuntimeRestoreSnapshot};
pub use registry::RuntimeEndpointRegistry;
pub use provider_policy::{
    capabilities_for, provider_declarations, resolve_node_executable,
    resolve_sidecar_entrypoint, select_provider, ProviderAvailability, ProviderSelection,
    RuntimeProvider,
    RuntimeProviderDeclaration, SystemProviderAvailability,
};
pub use pty_compat::PtyCompatAdapter;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Auto,
    Native,
    Pty,
}

impl BackendKind {
    const fn as_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Native => 1,
            Self::Pty => 2,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Native,
            2 => Self::Pty,
            _ => Self::Auto,
        }
    }
}

// Settings の既定値は PTY。settings_load / settings_save が SSOT から即時同期する。
static REQUESTED_BACKEND: AtomicU8 = AtomicU8::new(BackendKind::Pty.as_u8());

pub fn set_requested_backend_from_settings(value: &str) {
    let backend = BackendKind::try_from(value).unwrap_or(BackendKind::Pty);
    REQUESTED_BACKEND.store(backend.as_u8(), Ordering::Relaxed);
}

pub fn requested_backend() -> BackendKind {
    BackendKind::from_u8(REQUESTED_BACKEND.load(Ordering::Relaxed))
}

impl TryFrom<&str> for BackendKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "auto" => Ok(Self::Auto),
            "native" => Ok(Self::Native),
            "pty" => Ok(Self::Pty),
            _ => Err(format!(
                "unsupported agent runtime backend '{value}'; expected auto, native, or pty"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCapability {
    PtyExecution,
    NativeProcessExecution,
    StructuredEventStream,
    CooperativeCancellation,
    SessionResume,
    SessionFork,
    TurnSteering,
    ApprovalResponses,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionReason {
    ExplicitPty,
    ExplicitNativeAvailable,
    NativeCapabilitiesUnavailable,
    AutoNativeCapabilitiesAvailable,
    AutoPtyFallback,
}

pub trait CapabilityDetector {
    fn detected_capabilities(&self) -> Vec<RuntimeCapability>;
}

/// 実環境 detector。Unix は Codex app-server adapter の能力を、Windows は現行実装が
/// 保証できる PTY capability だけを公開する。
/// ただし Unix でも `codex` CLI が PATH 上に存在するときだけ native 能力を公開する。
/// 診断後に CLI が削除された、または daemon 起動/登録が失敗した場合、renderer は
/// native registration error を受けて PTY registration へ明示的に fallback する。
pub struct SystemCapabilityDetector;

impl CapabilityDetector for SystemCapabilityDetector {
    #[cfg(unix)]
    fn detected_capabilities(&self) -> Vec<RuntimeCapability> {
        if which::which("codex").is_err() {
            return vec![RuntimeCapability::PtyExecution];
        }
        vec![
            RuntimeCapability::PtyExecution,
            RuntimeCapability::NativeProcessExecution,
            RuntimeCapability::StructuredEventStream,
            RuntimeCapability::CooperativeCancellation,
            RuntimeCapability::SessionResume,
            RuntimeCapability::SessionFork,
            RuntimeCapability::TurnSteering,
            RuntimeCapability::ApprovalResponses,
        ]
    }

    #[cfg(not(unix))]
    fn detected_capabilities(&self) -> Vec<RuntimeCapability> {
        vec![RuntimeCapability::PtyExecution]
    }
}

/// capability 組み合わせを決定的に注入できる unit-test fixture。
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct FakeRuntime {
    capabilities: Vec<RuntimeCapability>,
}

#[cfg(test)]
impl FakeRuntime {
    pub fn new(capabilities: Vec<RuntimeCapability>) -> Self {
        Self { capabilities }
    }
}

#[cfg(test)]
impl CapabilityDetector for FakeRuntime {
    fn detected_capabilities(&self) -> Vec<RuntimeCapability> {
        self.capabilities.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSelection {
    pub requested_backend: BackendKind,
    pub selected_backend: BackendKind,
    pub reason: SelectionReason,
    pub capabilities: Vec<RuntimeCapability>,
}

const REQUIRED_NATIVE_CAPABILITIES: [RuntimeCapability; 3] = [
    RuntimeCapability::NativeProcessExecution,
    RuntimeCapability::StructuredEventStream,
    RuntimeCapability::CooperativeCancellation,
];

pub fn select_backend<D: CapabilityDetector>(
    requested_backend: BackendKind,
    detector: &D,
) -> RuntimeSelection {
    let capabilities = detector.detected_capabilities();
    let native_available = REQUIRED_NATIVE_CAPABILITIES
        .iter()
        .all(|capability| capabilities.contains(capability));

    let (selected_backend, reason) = match requested_backend {
        BackendKind::Pty => (BackendKind::Pty, SelectionReason::ExplicitPty),
        BackendKind::Native if native_available => (
            BackendKind::Native,
            SelectionReason::ExplicitNativeAvailable,
        ),
        BackendKind::Native => (
            BackendKind::Pty,
            SelectionReason::NativeCapabilitiesUnavailable,
        ),
        BackendKind::Auto if native_available => (
            BackendKind::Native,
            SelectionReason::AutoNativeCapabilitiesAvailable,
        ),
        BackendKind::Auto => (BackendKind::Pty, SelectionReason::AutoPtyFallback),
    };

    RuntimeSelection {
        requested_backend,
        selected_backend,
        reason,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_capabilities() -> Vec<RuntimeCapability> {
        vec![
            RuntimeCapability::PtyExecution,
            RuntimeCapability::NativeProcessExecution,
            RuntimeCapability::StructuredEventStream,
            RuntimeCapability::CooperativeCancellation,
        ]
    }

    #[test]
    fn fake_runtime_selects_native_for_auto_when_all_capabilities_exist() {
        let runtime = FakeRuntime::new(native_capabilities());
        let selection = select_backend(BackendKind::Auto, &runtime);

        assert_eq!(selection.selected_backend, BackendKind::Native);
        assert_eq!(
            selection.reason,
            SelectionReason::AutoNativeCapabilitiesAvailable
        );
    }

    #[test]
    fn fake_runtime_falls_back_to_pty_when_native_capabilities_are_incomplete() {
        let runtime = FakeRuntime::new(vec![
            RuntimeCapability::PtyExecution,
            RuntimeCapability::NativeProcessExecution,
        ]);
        let selection = select_backend(BackendKind::Auto, &runtime);

        assert_eq!(selection.selected_backend, BackendKind::Pty);
        assert_eq!(selection.reason, SelectionReason::AutoPtyFallback);
        assert_eq!(selection.capabilities, runtime.detected_capabilities());
    }

    #[test]
    fn explicit_native_is_guarded_when_required_capabilities_are_missing() {
        let runtime = FakeRuntime::new(vec![RuntimeCapability::PtyExecution]);
        let selection = select_backend(BackendKind::Native, &runtime);

        assert_eq!(selection.selected_backend, BackendKind::Pty);
        assert_eq!(
            selection.reason,
            SelectionReason::NativeCapabilitiesUnavailable
        );
    }
}

#[cfg(test)]
mod phase1_tests;
