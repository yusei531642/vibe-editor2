//! Renderer へ送る normalized runtime event wire contract。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeEventKind {
    MessageDelta,
    MessageComplete,
    ToolUse,
    Diff,
    Usage,
    ApprovalRequest,
    Lifecycle,
    Error,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum RuntimeLifecycleState {
    Spawning,
    Ready,
    Exited,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuntimeEventPayload {
    MessageDelta {
        delta: String,
    },
    MessageComplete {
        message: String,
    },
    ToolUse {
        tool_name: String,
        call_id: Option<String>,
        status: String,
        detail: Option<String>,
    },
    Diff {
        diff: String,
    },
    Usage {
        #[ts(type = "number")]
        input_tokens: u64,
        #[ts(type = "number")]
        cached_input_tokens: u64,
        #[ts(type = "number")]
        output_tokens: u64,
    },
    ApprovalRequest {
        request_id: String,
        method: String,
        reason: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
    },
    Lifecycle {
        state: RuntimeLifecycleState,
        detail: Option<String>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    Diagnostic {
        message: String,
    },
}

impl RuntimeEventPayload {
    pub fn kind(&self) -> RuntimeEventKind {
        match self {
            Self::MessageDelta { .. } => RuntimeEventKind::MessageDelta,
            Self::MessageComplete { .. } => RuntimeEventKind::MessageComplete,
            Self::ToolUse { .. } => RuntimeEventKind::ToolUse,
            Self::Diff { .. } => RuntimeEventKind::Diff,
            Self::Usage { .. } => RuntimeEventKind::Usage,
            Self::ApprovalRequest { .. } => RuntimeEventKind::ApprovalRequest,
            Self::Lifecycle { .. } => RuntimeEventKind::Lifecycle,
            Self::Error { .. } => RuntimeEventKind::Error,
            Self::Diagnostic { .. } => RuntimeEventKind::Diagnostic,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
/// `kind` is derived from `payload`; hand-built envelopes must go through [`Self::new`] to avoid
/// inconsistency between the two discriminators.
pub struct RuntimeEventEnvelope {
    pub endpoint_id: String,
    /// endpoint registration unit. sequence is monotonic only within this epoch.
    #[ts(type = "number")]
    pub epoch: u64,
    /// JSON/JS renderer では number として扱う。endpoint ごとの process-local counter なので
    /// JavaScript の safe integer 上限へ到達する前に session lifetime が終わる。
    #[ts(type = "number")]
    pub sequence: u64,
    pub kind: RuntimeEventKind,
    pub payload: RuntimeEventPayload,
    pub timestamp: String,
}

impl RuntimeEventEnvelope {
    pub fn new(
        endpoint_id: String,
        epoch: u64,
        sequence: u64,
        payload: RuntimeEventPayload,
    ) -> Self {
        Self {
            endpoint_id,
            epoch,
            sequence,
            kind: payload.kind(),
            payload,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runtime_event_envelope_serializes_to_locked_camel_case_shape() {
        let mut event = RuntimeEventEnvelope::new(
            "endpoint-1".to_string(),
            3,
            7,
            RuntimeEventPayload::Lifecycle {
                state: RuntimeLifecycleState::Ready,
                detail: None,
            },
        );
        event.timestamp = "2026-07-16T00:00:00Z".to_string();

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "endpointId": "endpoint-1",
                "epoch": 3,
                "sequence": 7,
                "kind": "lifecycle",
                "payload": { "type": "lifecycle", "state": "ready", "detail": null },
                "timestamp": "2026-07-16T00:00:00Z"
            })
        );
    }
}

#[cfg(test)]
mod ts_bindings_tests {
    use super::*;
    use std::{fs, path::PathBuf};

    const GENERATED_PATH: &str = "../src/types/generated/runtime-events.ts";

    fn generated_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GENERATED_PATH)
    }

    fn declaration<T: TS>() -> String {
        format!("export {}\n", T::decl(&ts_rs::Config::default()))
    }

    fn render() -> String {
        [
            "// This file is generated from src-tauri/src/agent_runtime/event.rs via ts-rs.",
            "// Run `npm run generate:runtime-event-types` after changing runtime event wire types.",
            "",
            &declaration::<RuntimeEventKind>(),
            &declaration::<RuntimeLifecycleState>(),
            &declaration::<RuntimeEventPayload>(),
            &declaration::<RuntimeEventEnvelope>(),
        ]
        .join("\n")
    }

    #[test]
    fn generated_runtime_event_bindings_are_current() {
        let path = generated_path();
        let next = render();
        if std::env::var_os("UPDATE_RUNTIME_EVENT_TYPES").is_some() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create generated type directory");
            }
            fs::write(&path, next).expect("write generated runtime event types");
            return;
        }

        let current = fs::read_to_string(&path).expect("read generated runtime event types");
        assert_eq!(
            current,
            next,
            "{} is stale; run `npm run generate:runtime-event-types`",
            path.display()
        );
    }
}
