// MCP 設定操作モジュール
//
// 旧 src/main/lib/mcp-config/{claude,codex,index}.ts の Rust 移植版。
// Claude Code (~/.claude.json) / Codex (~/.codex/config.toml) の
// `vibe-team` MCP サーバーエントリを差分マージする。

pub mod claude;
pub mod codex;

use serde_json::{json, Value};

/// Claude/Codex MCP 設定で共有する bridge エントリ
pub fn bridge_desired(socket: &str, token: &str, bridge_path: &str) -> Value {
    let normalized = bridge_path.replace('\\', "/");
    json!({
        "type": "stdio",
        "command": "node",
        "args": [normalized],
        "env": {
            "VIBE_TEAM_SOCKET": socket,
            "VIBE_TEAM_TOKEN": token,
        }
    })
}

/// Native team runtime 専用の bridge entry。
/// identity は Hub で認可済みの値だけを呼び出し元から渡し、renderer 由来の env を使わない。
pub fn team_bridge_desired(
    socket: &str,
    token: &str,
    bridge_path: &str,
    team_id: &str,
    agent_id: &str,
    role: &str,
) -> Value {
    let mut desired = bridge_desired(socket, token, bridge_path);
    if let Some(env) = desired.get_mut("env").and_then(Value::as_object_mut) {
        env.insert("VIBE_TEAM_ID".into(), json!(team_id));
        env.insert("VIBE_AGENT_ID".into(), json!(agent_id));
        env.insert("VIBE_TEAM_ROLE".into(), json!(role));
    }
    desired
}

#[cfg(test)]
mod tests {
    use super::team_bridge_desired;

    #[test]
    fn native_team_bridge_contains_complete_hub_identity() {
        let value = team_bridge_desired(
            "/tmp/hub.sock",
            "secret",
            "/tmp/bridge.js",
            "team-1",
            "leader-team-1",
            "leader",
        );
        assert_eq!(value["env"]["VIBE_TEAM_ID"], "team-1");
        assert_eq!(value["env"]["VIBE_AGENT_ID"], "leader-team-1");
        assert_eq!(value["env"]["VIBE_TEAM_ROLE"], "leader");
    }
}
