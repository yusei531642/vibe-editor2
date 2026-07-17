//! app-server への接続確立と initialize handshake。
//! client.rs の 500 行 ratchet を守るため接続系だけを分離した。

use super::client::{
    disconnected, fatal, map_wire_error, rpc_result, write_json, SUPPORTED_PROTOCOL_VERSION,
};
use crate::agent_runtime::RuntimeAdapterError;
use crate::team_hub::app_server::wire::WsStream;
use serde_json::{json, Value};
use tokio::net::UnixStream;

pub(super) enum ClientSource {
    Path(String),
    #[cfg(test)]
    Stream(UnixStream),
}

pub(super) async fn connect_and_initialize(
    source: ClientSource,
) -> Result<WsStream<UnixStream>, RuntimeAdapterError> {
    let stream = match source {
        ClientSource::Path(socket_path) => UnixStream::connect(socket_path)
            .await
            .map_err(|error| fatal("runtime_app_server_unreachable", error.to_string()))?,
        #[cfg(test)]
        ClientSource::Stream(stream) => stream,
    };
    let mut ws = WsStream::new(stream, true);
    ws.client_handshake().await.map_err(map_wire_error)?;
    let params = json!({
        "clientInfo": { "name": "vibe-editor2", "title": "vibe-editor 2", "version": env!("CARGO_PKG_VERSION") },
        "capabilities": { "experimentalApi": false, "requestAttestation": false }
    });
    write_json(
        &mut ws,
        &json!({ "id": 1, "method": "initialize", "params": params }),
    )
    .await?;
    let result = loop {
        let text = ws
            .read_text()
            .await
            .map_err(map_wire_error)?
            .ok_or_else(disconnected)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| fatal("runtime_app_server_protocol", error.to_string()))?;
        if value.get("id").and_then(Value::as_i64) == Some(1) && value.get("method").is_none() {
            break rpc_result(&value)?;
        }
    };
    if let Some(version) = result.get("protocolVersion") {
        let actual = version
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| version.to_string());
        if actual != SUPPORTED_PROTOCOL_VERSION {
            return Err(fatal(
                "runtime_app_server_version_mismatch",
                format!("unsupported app-server protocol version '{actual}', expected {SUPPORTED_PROTOCOL_VERSION}"),
            ));
        }
    }
    write_json(&mut ws, &json!({ "method": "initialized", "params": {} })).await?;
    Ok(ws)
}

