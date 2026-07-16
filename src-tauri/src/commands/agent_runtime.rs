// agent_runtime.* command — Issue #21 Phase 0 runtime diagnostics.

use crate::agent_runtime::{
    select_backend, BackendKind, RuntimeCapability, SelectionReason, SystemCapabilityDetector,
};
use crate::commands::error::{CommandError, CommandResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeDiagnostics {
    pub requested_backend: BackendKind,
    pub selected_backend: BackendKind,
    pub reason: SelectionReason,
    pub capabilities: Vec<RuntimeCapability>,
}

/// Renderer の未保存 draft も診断できるよう backend を引数で受ける。
/// Phase 0 の system detector は PTY のみを報告し、native 要件不足時は必ず PTY へ戻す。
#[tauri::command]
pub async fn agent_runtime_diagnostics(
    backend: String,
) -> CommandResult<AgentRuntimeDiagnostics> {
    let requested_backend =
        BackendKind::try_from(backend.as_str()).map_err(CommandError::validation)?;
    let selection = select_backend(requested_backend, &SystemCapabilityDetector);
    Ok(AgentRuntimeDiagnostics {
        requested_backend: selection.requested_backend,
        selected_backend: selection.selected_backend,
        reason: selection.reason,
        capabilities: selection.capabilities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn diagnostics_uses_camelcase_and_safe_auto_fallback() {
        let result = agent_runtime_diagnostics("auto".to_string()).await.unwrap();
        let value = serde_json::to_value(result).unwrap();

        assert_eq!(value["requestedBackend"], json!("auto"));
        assert_eq!(value["selectedBackend"], json!("pty"));
        assert_eq!(value["reason"], json!("autoPtyFallback"));
        assert_eq!(value["capabilities"], json!(["ptyExecution"]));
    }

    #[tokio::test]
    async fn diagnostics_rejects_unknown_backend() {
        let error = agent_runtime_diagnostics("unknown".to_string())
            .await
            .unwrap_err();
        assert_eq!(error.code(), "validation");
        let serialized = serde_json::to_value(&error).unwrap();
        assert!(serialized["message"]
            .as_str()
            .unwrap()
            .contains("expected auto, native, or pty"));
    }
}
