//! Codex app-server notification / request から normalized runtime event への変換。

use crate::agent_runtime::RuntimeEventPayload;
use serde_json::Value;

pub fn notification(method: &str, params: &Value) -> Vec<RuntimeEventPayload> {
    if method.ends_with("agentMessage/delta") || method.ends_with("message/delta") {
        return string_at(params, &["delta", "text"])
            .map(|delta| vec![RuntimeEventPayload::MessageDelta { delta }])
            .unwrap_or_default();
    }

    if method == "item/completed" {
        if let Some(item) = params.get("item") {
            if item_type(item).is_some_and(|kind| kind.eq_ignore_ascii_case("agentMessage")) {
                if let Some(message) = string_at(item, &["text", "message", "content"]) {
                    return vec![RuntimeEventPayload::MessageComplete { message }];
                }
            }
            if let Some(tool) = tool_event(item, "completed") {
                return vec![tool];
            }
        }
    }

    if method == "item/started" {
        if let Some(tool) = params
            .get("item")
            .and_then(|item| tool_event(item, "started"))
        {
            return vec![tool];
        }
    }

    if method.contains("diff") {
        if let Some(diff) = string_at(params, &["diff", "patch", "unifiedDiff"]) {
            return vec![RuntimeEventPayload::Diff { diff }];
        }
    }

    if method.contains("tokenUsage") || method.contains("usage") {
        if let Some(usage) = usage_event(params) {
            return vec![usage];
        }
    }

    match method {
        "thread/started" | "thread/resumed" | "thread/forked" | "turn/started"
        | "turn/completed" | "turn/interrupted" => vec![RuntimeEventPayload::Diagnostic {
            message: method.to_string(),
        }],
        _ => Vec::new(),
    }
}

pub fn approval(request_id: String, method: String, params: &Value) -> RuntimeEventPayload {
    RuntimeEventPayload::ApprovalRequest {
        request_id,
        method,
        reason: string_at(params, &["reason", "message"]),
        command: command_text(params),
        cwd: string_at(params, &["cwd"]),
    }
}

pub fn thread_id(params: &Value) -> Option<String> {
    string_at(params, &["threadId"]).or_else(|| {
        params
            .get("thread")
            .and_then(|thread| string_at(thread, &["id"]))
    })
}

pub fn turn_id(params: &Value) -> Option<String> {
    string_at(params, &["turnId", "expectedTurnId"])
        .or_else(|| params.get("turn").and_then(|turn| string_at(turn, &["id"])))
}

fn tool_event(item: &Value, status: &str) -> Option<RuntimeEventPayload> {
    let kind = item_type(item)?;
    let tool_name = match kind {
        "commandExecution" => string_at(item, &["command"])
            .map(|command| format!("command: {command}"))
            .unwrap_or_else(|| "commandExecution".to_string()),
        "mcpToolCall" => string_at(item, &["tool", "name"]).unwrap_or_else(|| kind.to_string()),
        "fileChange" => "fileChange".to_string(),
        "webSearch" => "webSearch".to_string(),
        _ => return None,
    };
    let detail = item
        .get("arguments")
        .or_else(|| item.get("changes"))
        .or_else(|| item.get("output"))
        .map(compact_json);
    Some(RuntimeEventPayload::ToolUse {
        tool_name,
        call_id: string_at(item, &["id", "callId"]),
        status: status.to_string(),
        detail,
    })
}

fn usage_event(params: &Value) -> Option<RuntimeEventPayload> {
    let usage = params
        .get("tokenUsage")
        .or_else(|| params.get("usage"))
        .unwrap_or(params);
    let usage = usage.get("total").unwrap_or(usage);
    let input_tokens = u64_at(usage, &["inputTokens", "input_tokens"]);
    let cached_input_tokens = u64_at(
        usage,
        &[
            "cachedInputTokens",
            "cached_input_tokens",
            "cacheReadInputTokens",
        ],
    );
    let output_tokens = u64_at(usage, &["outputTokens", "output_tokens"]);
    if input_tokens == 0 && cached_input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    Some(RuntimeEventPayload::Usage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    })
}

fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

fn command_text(params: &Value) -> Option<String> {
    match params.get("command") {
        Some(Value::String(command)) => Some(command.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => params
            .get("item")
            .and_then(|item| string_at(item, &["command"])),
    }
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn u64_at(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_message_diff_usage_and_approval() {
        assert!(matches!(
            notification("item/agentMessage/delta", &json!({ "delta": "hi" }))[0],
            RuntimeEventPayload::MessageDelta { ref delta } if delta == "hi"
        ));
        assert!(matches!(
            notification("turn/diff/updated", &json!({ "diff": "@@" }))[0],
            RuntimeEventPayload::Diff { ref diff } if diff == "@@"
        ));
        assert!(matches!(
            notification(
                "thread/tokenUsage/updated",
                &json!({ "tokenUsage": { "total": { "inputTokens": 2, "outputTokens": 3 } } })
            )[0],
            RuntimeEventPayload::Usage {
                input_tokens: 2,
                output_tokens: 3,
                ..
            }
        ));
        assert!(matches!(
            approval("7".into(), "item/requestApproval".into(), &json!({ "command": ["git", "status"] })),
            RuntimeEventPayload::ApprovalRequest { ref command, .. } if command.as_deref() == Some("git status")
        ));
    }
}
