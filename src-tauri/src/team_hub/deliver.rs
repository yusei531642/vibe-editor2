//! Issue #1062: `team_send` の配送経路ルータ。
//!
//! 既定は従来どおり PTY への bracketed-paste 注入 (`inject::inject`)。
//! codex セッションで `app_server_socket` と `thread_id` の両方が分かっている場合のみ、
//! codex 公式 app-server JSON-RPC (`turn/start`) で配送する。app-server 配送が失敗したら
//! PTY 注入にフォールバックするため、可用性は従来以上を保つ。
//!
//! Issue #1068: ユーザーが設定 (`codexTeamSendDelivery`) で `pty` を選んだ場合は、app-server 経路を
//! 一切使わず常に PTY 注入する (`codex_delivery::prefers_pty`)。既定の `backend` は上記の挙動。

use crate::team_hub::inject::InjectError;
use crate::team_hub::TeamHub;

/// `agent_id` 宛にメッセージを配送する。戻り値・リトライ意味論は `inject::inject` と同一。
pub async fn deliver_message(
    hub: &TeamHub,
    team_id: &str,
    agent_id: &str,
    from_role: &str,
    text: &str,
) -> Result<(), InjectError> {
    // Issue #1068: 設定で PTY を強制している場合は app-server 経路を完全にスキップする。
    // app-server 未対応 / 失敗時は下の PTY 注入へフォールバック。
    hub.deliver_agent_message(team_id, agent_id, from_role, text)
        .await
}

/// app-server 経路での配送を試みる。成功で `true`、失敗 (= PTY フォールバック) で `false`。
#[cfg(unix)]
#[allow(dead_code)]
async fn try_app_server(agent_id: &str, socket: &str, thread_id: &str, text: &str) -> bool {
    match crate::team_hub::app_server::deliver(socket, thread_id, text).await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(
                "[deliver] app-server delivery failed for agent {agent_id} \
                 (code={}); falling back to PTY inject",
                err.code()
            );
            false
        }
    }
}

/// Windows では app-server (unix socket) 配送は未対応のため常に PTY 注入へフォールバックする。
#[cfg(not(unix))]
#[allow(dead_code)]
async fn try_app_server(_agent_id: &str, _socket: &str, _thread_id: &str, _text: &str) -> bool {
    false
}
