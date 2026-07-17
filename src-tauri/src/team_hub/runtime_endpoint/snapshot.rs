//! Issue #26: Team Card / Inspector 向け runtime binding snapshot。

use super::types::{RuntimeEndpointBackend, TeamRuntimeEndpointSnapshot};
use crate::agent_runtime::RuntimeOperation;
use crate::team_hub::TeamHub;

impl TeamHub {
    /// renderer projection の初期同期用。caller は active team authz を先に通す。
    pub(crate) async fn runtime_bindings_snapshot(
        &self,
        team_id: &str,
    ) -> Vec<TeamRuntimeEndpointSnapshot> {
        let state = self.state.lock().await;
        let mut snapshots = Vec::new();
        for ((binding_team_id, agent_id), binding) in &state.runtime_endpoints {
            if binding_team_id != team_id {
                continue;
            }
            for endpoint in binding.native.iter().chain(binding.pty.iter()) {
                snapshots.push(TeamRuntimeEndpointSnapshot {
                    team_id: binding_team_id.clone(),
                    agent_id: agent_id.clone(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                    backend: match endpoint.backend {
                        RuntimeEndpointBackend::Native => "native",
                        RuntimeEndpointBackend::Pty => "pty",
                    }
                    .to_string(),
                    session_id: endpoint.session_id.clone(),
                    task_ids: binding.task_ids.clone(),
                    live: self
                        .runtime
                        .manager
                        .registry()
                        .resolve(&endpoint.endpoint_id)
                        .is_some(),
                });
            }
        }
        snapshots.sort_by(|left, right| {
            left.agent_id
                .cmp(&right.agent_id)
                .then_with(|| left.backend.cmp(&right.backend))
        });
        snapshots
    }

    /// PTY member 操作は renderer の endpointId を信用せず、TeamHub binding から解決する。
    pub(crate) async fn control_pty_runtime(
        &self,
        team_id: &str,
        agent_id: &str,
        action: &str,
    ) -> Result<(String, RuntimeOperation), String> {
        let endpoint_id = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .get(&(team_id.to_string(), agent_id.to_string()))
                .and_then(|binding| binding.pty.as_ref())
                .map(|endpoint| endpoint.endpoint_id.clone())
                .ok_or_else(|| "PTY runtime endpoint is not bound for this member".to_string())?
        };
        if self
            .runtime
            .manager
            .registry()
            .resolve(&endpoint_id)
            .is_none()
        {
            return Err("PTY runtime endpoint is not live".to_string());
        }
        let operation = match action {
            "interrupt" => self.runtime.manager.interrupt(&endpoint_id),
            "stop" => self.runtime.manager.stop(&endpoint_id),
            _ => return Err("unsupported PTY runtime action".to_string()),
        };
        Ok((endpoint_id, operation))
    }

    /// Approval 応答は renderer の endpointId を信用せず、member binding の live endpoint を使う。
    pub(crate) async fn approval_runtime_endpoint(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        let state = self.state.lock().await;
        let binding = state
            .runtime_endpoints
            .get(&(team_id.to_string(), agent_id.to_string()))
            .ok_or_else(|| "runtime endpoint is not bound for this member".to_string())?;
        binding
            .native
            .iter()
            .chain(binding.pty.iter())
            .find(|endpoint| {
                self.runtime
                    .manager
                    .registry()
                    .resolve(&endpoint.endpoint_id)
                    .is_some()
            })
            .map(|endpoint| endpoint.endpoint_id.clone())
            .ok_or_else(|| "runtime endpoint is not live for this member".to_string())
    }
}
