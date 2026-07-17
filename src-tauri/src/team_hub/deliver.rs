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
    let backend = hub.selected_runtime_backend();
    let mut native_error: Option<InjectError> = None;
    if let Some(result) = hub
        .try_deliver_native_message(team_id, agent_id, from_role, text, backend)
        .await
    {
        match result {
            Ok(()) => return Ok(()),
            // native 失敗時も即 PTY へ落とさず、下の legacy app-server 経路を
            // 試してから PTY fallback する (Issue #1062 の app-server 優先契約、
            // PR #34 一次レビュー 🟡5)。
            Err(error) => {
                tracing::warn!(
                    agent_id,
                    code = error.code(),
                    "[teamhub] native delivery failed; trying legacy app-server then PTY"
                );
                native_error = Some(error);
            }
        }
    }
    if !hub.prefers_legacy_codex_pty() {
        if let Some(session) = hub.registry.get_by_agent(agent_id) {
            if let Some((socket, thread_id)) =
                crate::pty::codex_app_server::target_for_session(&session)
            {
                if hub
                    .try_legacy_app_server(agent_id, &socket, &thread_id, text)
                    .await
                {
                    return Ok(());
                }
            }
        }
    }
    // native 失敗 & PTY session を持たない member (app-server 経由の native worker) で
    // 無条件に PTY へ落とすと、PtyCompatAdapter 登録が lifecycle.endpoint_id を
    // `team-pty-{agentId}` へ上書きした挙句 `inject_no_session` で失敗し、元の native
    // エラーコードを潰す (PR #34 二次レビュー 🟡)。PTY session が実在する場合のみ fallback。
    if let Some(error) = native_error {
        if hub.registry.get_by_agent(agent_id).is_none() {
            return Err(error);
        }
    }
    // app-server 未対応 / 失敗時は下の PTY 注入へフォールバック。
    hub.deliver_pty_message(team_id, agent_id, from_role, text)
        .await
}

impl TeamHub {
    async fn try_legacy_app_server(
        &self,
        agent_id: &str,
        socket: &str,
        thread_id: &str,
        text: &str,
    ) -> bool {
        #[cfg(test)]
        if let Some(result) = *self
            .runtime
            .legacy_app_server_override
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            self.runtime
                .legacy_app_server_deliveries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((
                    agent_id.to_string(),
                    socket.to_string(),
                    thread_id.to_string(),
                    text.to_string(),
                ));
            return result;
        }
        try_app_server(agent_id, socket, thread_id, text).await
    }
}

/// app-server 経路での配送を試みる。成功で `true`、失敗 (= PTY フォールバック) で `false`。
#[cfg(unix)]
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
async fn try_app_server(_agent_id: &str, _socket: &str, _thread_id: &str, _text: &str) -> bool {
    false
}
