//! Claude Agent sidecar JSONL wire types and normalized event conversion.

use crate::agent_runtime::{RuntimeAdapterError, RuntimeEventPayload};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SIDECAR_PROTOCOL: &str = "vibe-claude-agent";
pub const SIDECAR_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct SidecarRequest<'a> {
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub version: u32,
    pub id: &'a str,
    pub method: &'a str,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SidecarIncoming {
    Hello {
        version: u32,
        protocol: String,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    Response {
        version: u32,
        id: String,
        ok: bool,
        #[serde(default)]
        result: Value,
        error: Option<SidecarError>,
    },
    Event {
        version: u32,
        event: SidecarEvent,
    },
}

impl SidecarIncoming {
    pub fn version(&self) -> u32 {
        match self {
            Self::Hello { version, .. }
            | Self::Response { version, .. }
            | Self::Event { version, .. } => *version,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl SidecarError {
    pub fn into_runtime_error(self, redactor: &Redactor) -> RuntimeAdapterError {
        RuntimeAdapterError::new(self.code, redactor.redact(&self.message), self.recoverable)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SidecarEvent {
    Session {
        session_id: String,
    },
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
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
    },
    ApprovalRequest {
        request_id: String,
        method: String,
        reason: Option<String>,
    },
    Diagnostic {
        message: String,
    },
    TurnComplete {
        interrupted: bool,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
}

pub enum ConvertedSidecarEvent {
    Session(String),
    Payload(RuntimeEventPayload),
    Failure(RuntimeAdapterError),
}

impl SidecarEvent {
    pub fn convert(self, redactor: &Redactor) -> ConvertedSidecarEvent {
        let payload = match self {
            Self::Session { session_id } => {
                if session_id.is_empty()
                    || session_id.len() > 1024
                    || session_id.chars().any(char::is_control)
                {
                    return ConvertedSidecarEvent::Failure(RuntimeAdapterError::new(
                        "runtime_claude_sidecar_protocol",
                        "sidecar emitted an invalid session id",
                        false,
                    ));
                }
                return ConvertedSidecarEvent::Session(session_id);
            }
            Self::MessageDelta { delta } => RuntimeEventPayload::MessageDelta {
                delta: redactor.redact(&delta),
            },
            Self::MessageComplete { message } => RuntimeEventPayload::MessageComplete {
                message: redactor.redact(&message),
            },
            Self::ToolUse {
                tool_name,
                call_id,
                status,
                detail,
            } => RuntimeEventPayload::ToolUse {
                tool_name: bounded(tool_name, 256),
                call_id: call_id.map(|value| bounded(value, 256)),
                status: bounded(status, 128),
                detail: detail.map(|value| redactor.redact(&value)),
            },
            Self::Diff { diff } => RuntimeEventPayload::Diff {
                diff: redactor.redact(&diff),
            },
            Self::Usage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
            } => RuntimeEventPayload::Usage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
            },
            Self::ApprovalRequest {
                request_id,
                method,
                reason,
            } => RuntimeEventPayload::ApprovalRequest {
                request_id: bounded(request_id, 256),
                method: bounded(method, 256),
                reason: reason.map(|value| redactor.redact(&value)),
                // Sidecar は raw command / cwd / blocked path を protocol に載せない。
                command: None,
                cwd: None,
            },
            Self::Diagnostic { message } => RuntimeEventPayload::Diagnostic {
                message: redactor.redact(&message),
            },
            Self::TurnComplete { interrupted } => RuntimeEventPayload::TurnComplete { interrupted },
            Self::Error {
                code,
                message,
                recoverable,
            } => {
                let error = RuntimeAdapterError::new(
                    bounded(code, 256),
                    redactor.redact(&message),
                    recoverable,
                );
                if recoverable {
                    RuntimeEventPayload::Error {
                        code: error.code,
                        message: error.message,
                        recoverable: true,
                    }
                } else {
                    return ConvertedSidecarEvent::Failure(error);
                }
            }
        };
        ConvertedSidecarEvent::Payload(payload)
    }
}

#[derive(Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<_> = secrets
            .into_iter()
            .filter(|value| value.len() >= 4)
            .collect();
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        secrets.dedup();
        Self { secrets }
    }

    pub fn redact(&self, value: &str) -> String {
        let mut redacted = value.to_string();
        for secret in &self.secrets {
            redacted = redacted.replace(secret, "<redacted>");
        }
        bounded(redact_assignments(redacted), 16 * 1024)
    }
}

fn redact_assignments(mut value: String) -> String {
    for marker in ["api_key=", "api-key=", "token=", "authorization="] {
        let mut offset = 0;
        loop {
            let lower = value.to_ascii_lowercase();
            let Some(relative) = lower[offset..].find(marker) else {
                break;
            };
            let start = offset + relative + marker.len();
            let end = value[start..]
                .find(char::is_whitespace)
                .map_or(value.len(), |length| start + length);
            value.replace_range(start..end, "<redacted>");
            offset = start + "<redacted>".len();
            if offset >= value.len() {
                break;
            }
        }
    }
    value
}

fn bounded(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    let mut boundary = max;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push('…');
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redactor_removes_exact_secrets_and_assignment_values() {
        let redactor = Redactor::new(["secret-value".to_string()]);
        let result = redactor.redact("secret-value token=also-secret next");
        assert_eq!(result, "<redacted> token=<redacted> next");
    }

    #[test]
    fn redactor_removes_multiple_assignment_values() {
        let result = Redactor::default().redact("token=abc token=def");
        assert_eq!(result, "token=<redacted> token=<redacted>");
    }

    #[test]
    fn redactor_recomputes_assignment_indexes_after_shrinking() {
        let result =
            Redactor::default().redact("token=abcdefghijklmnopqrstuvwxyz token=still-secret next");
        assert_eq!(result, "token=<redacted> token=<redacted> next");
    }

    #[test]
    fn redactor_recomputes_utf8_boundaries_after_shrinking() {
        let result = Redactor::default().redact("token=abcdefghijk token=あ next");
        assert_eq!(result, "token=<redacted> token=<redacted> next");
    }

    #[test]
    fn redactor_removes_secret_before_truncating_at_boundary() {
        let secret = "secret-value";
        let input = format!("{}{}", "x".repeat(16 * 1024 - 4), secret);
        let result = Redactor::new([secret.to_string()]).redact(&input);

        assert!(!result.contains(secret));
        assert!(!result.contains("secr"));
        assert!(result.ends_with('…'));
    }
}
