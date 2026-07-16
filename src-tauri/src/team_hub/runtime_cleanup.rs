//! RuntimeEndpoint の blocking cleanup と配送error復元。

use crate::pty::session::TerminationReason;
use crate::team_hub::inject::InjectError;
use crate::team_hub::TeamHub;

impl TeamHub {
    pub async fn cleanup_agent_runtime(&self, team_id: &str, agent_id: &str) {
        let binding = {
            let mut state = self.state.lock().await;
            state
                .runtime_endpoints
                .remove(&(team_id.to_string(), agent_id.to_string()))
        };
        if let Some(binding) = binding {
            for endpoint in binding.native.into_iter().chain(binding.pty) {
                let manager = self.runtime.manager.clone();
                let endpoint_id = endpoint.endpoint_id.clone();
                match tauri::async_runtime::spawn_blocking(move || manager.stop(&endpoint_id)).await
                {
                    Ok(stop) => self.emit_runtime_events(&stop.events).await,
                    Err(error) => {
                        tracing::warn!(agent_id, "[teamhub] runtime stop task failed: {error}")
                    }
                }
                if self
                    .runtime
                    .manager
                    .registry()
                    .resolve(&endpoint.endpoint_id)
                    .is_some()
                {
                    let manager = self.runtime.manager.clone();
                    let endpoint_id = endpoint.endpoint_id.clone();
                    match tauri::async_runtime::spawn_blocking(move || {
                        manager.dispose(&endpoint_id)
                    })
                    .await
                    {
                        Ok(dispose) => self.emit_runtime_events(&dispose.events).await,
                        Err(error) => tracing::warn!(
                            agent_id,
                            "[teamhub] runtime dispose task failed: {error}"
                        ),
                    }
                }
            }
        }
        if let Some(session) = self.registry.get_by_agent(agent_id) {
            if let Err(error) = session.kill(TerminationReason::UserClose) {
                tracing::warn!(agent_id, "[teamhub] fallback PTY cleanup failed: {error}");
            }
        }
    }

    pub async fn cleanup_team_runtimes(&self, team_id: &str) {
        let agent_ids: Vec<String> = {
            let state = self.state.lock().await;
            state
                .runtime_endpoints
                .keys()
                .filter(|(candidate, _)| candidate == team_id)
                .map(|(_, agent_id)| agent_id.clone())
                .collect()
        };
        for agent_id in agent_ids {
            self.cleanup_agent_runtime(team_id, &agent_id).await;
        }
    }
}

pub(crate) fn restore_inject_error(
    error: crate::agent_runtime::RuntimeAdapterError,
) -> InjectError {
    match error.code.as_str() {
        "inject_no_session" => InjectError::NoSession,
        "inject_write_initial_failed" => InjectError::WriteInitialFailed(error.message),
        "inject_write_partial" => InjectError::WritePartial {
            written_chunks: 0,
            total_chunks: 0,
            source: error.message,
        },
        "inject_session_replaced" => InjectError::SessionReplaced {
            written_chunks: 0,
            total_chunks: 0,
        },
        "inject_final_cr_failed" => InjectError::FinalCrFailed(error.message),
        "inject_task_join_failed" => InjectError::TaskJoinFailed {
            phase: "runtime_delivery",
            source: error.message,
        },
        _ => InjectError::WriteInitialFailed(format!("{}: {}", error.code, error.message)),
    }
}
