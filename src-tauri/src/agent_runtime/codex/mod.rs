//! Phase 2 Codex app-server native runtime adapter (Unix only).

mod adapter;
mod client;
mod convert;
mod handshake;

pub use adapter::{CodexAdapterEvent, CodexAdapterEventSink, CodexRuntimeAdapter};

use crate::agent_runtime::RuntimeAdapterError;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};

/// Installed Codex app-server の model catalog を取得する。UI の固定リストではなく、
/// 現在の account / CLI が広告する model と effort を正本にする。
pub async fn model_catalog(codex_command: String) -> Result<Value, RuntimeAdapterError> {
    tokio::time::timeout(Duration::from_secs(15), query_model_catalog(codex_command))
        .await
        .map_err(|_| {
            runtime_error(
                "runtime_app_server_timeout",
                "Codex model catalog timed out",
            )
        })?
}

async fn query_model_catalog(codex_command: String) -> Result<Value, RuntimeAdapterError> {
    let mut command = Command::new(codex_command);
    command.args(["app-server", "--stdio"]);
    command.env("PATH", crate::pty::session::unix_path::enriched_path());
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| runtime_error("runtime_app_server_spawn", error.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| runtime_error("runtime_app_server_stdio", "Codex stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| runtime_error("runtime_app_server_stdio", "Codex stdout unavailable"))?;
    let mut lines = BufReader::new(stdout).lines();

    write_request(
        &mut stdin,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "vibe-editor2", "title": "vibe-editor 2", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": false, "requestAttestation": false }
            }
        }),
    )
    .await?;
    read_response(&mut lines, 1).await?;
    write_request(
        &mut stdin,
        &json!({ "method": "initialized", "params": {} }),
    )
    .await?;
    write_request(
        &mut stdin,
        &json!({ "id": 2, "method": "model/list", "params": { "cursor": null, "limit": 100, "includeHidden": false } }),
    )
    .await?;
    let result = read_response(&mut lines, 2).await;
    let _ = child.kill().await;
    result
}

async fn write_request(
    stdin: &mut tokio::process::ChildStdin,
    value: &Value,
) -> Result<(), RuntimeAdapterError> {
    let mut line = serde_json::to_vec(value)
        .map_err(|error| runtime_error("runtime_app_server_protocol", error.to_string()))?;
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|error| runtime_error("runtime_app_server_stdio", error.to_string()))?;
    stdin
        .flush()
        .await
        .map_err(|error| runtime_error("runtime_app_server_stdio", error.to_string()))
}

async fn read_response(
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: i64,
) -> Result<Value, RuntimeAdapterError> {
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| runtime_error("runtime_app_server_stdio", error.to_string()))?
    {
        let value: Value = serde_json::from_str(&line)
            .map_err(|error| runtime_error("runtime_app_server_protocol", error.to_string()))?;
        if value.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(runtime_error(
                "runtime_app_server_request",
                error.to_string(),
            ));
        }
        return value
            .get("result")
            .cloned()
            .ok_or_else(|| runtime_error("runtime_app_server_protocol", "response has no result"));
    }
    Err(runtime_error(
        "runtime_app_server_disconnected",
        "Codex app-server closed before responding",
    ))
}

fn runtime_error(code: impl Into<String>, message: impl Into<String>) -> RuntimeAdapterError {
    RuntimeAdapterError::new(code, message, true)
}

#[cfg(test)]
mod tests;
