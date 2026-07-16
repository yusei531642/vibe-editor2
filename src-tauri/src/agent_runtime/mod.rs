//! Issue #21 / #22: agent runtime の境界。
//!
//! Phase 0 の backend 選択と capability 診断に加え、Phase 1 では adapter / endpoint
//! registry / normalized event envelope を提供する。native backend が実装されるまでは
//! system detector は PTY 能力だけを報告するため、`auto` は安全に PTY へ fallback する。

mod adapter;
mod event;
mod event_buffer;
mod manager;
mod pty_compat;

pub use adapter::{
    AgentRuntimeAdapter, RuntimeAdapterError, RuntimeSessionSpawnRequest, RuntimeTurnSpawnRequest,
};
#[allow(unused_imports)]
pub use event::{
    RuntimeEventEnvelope, RuntimeEventKind, RuntimeEventPayload, RuntimeLifecycleState,
};
pub use event_buffer::{RuntimeEventBuffer, DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY};
#[allow(unused_imports)]
pub use manager::{RuntimeEndpointRegistry, RuntimeManager, RuntimeOperation};
pub use pty_compat::PtyCompatAdapter;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Auto,
    Native,
    Pty,
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

/// Phase 0 の実環境 detector。native runtime はまだ実行経路へ接続されていないため、
/// 現行実装が保証できる PTY capability だけを公開する。
pub struct SystemCapabilityDetector;

impl CapabilityDetector for SystemCapabilityDetector {
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
