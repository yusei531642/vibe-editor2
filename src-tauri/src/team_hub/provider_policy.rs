//! Recruit role + Rust settings を concrete runtime provider policy へ投影する。

use crate::agent_runtime::{
    select_provider, BackendKind, ProviderSelection, SystemProviderAvailability,
};

pub(crate) async fn select_recruit_provider(
    role_profile_id: &str,
    resolved_engine: &str,
) -> ProviderSelection {
    let settings = crate::commands::settings::settings_load()
        .await
        .unwrap_or_default();
    let custom_id = role_profile_id.strip_prefix("custom:");
    let custom = custom_id.and_then(|id| {
        settings
            .custom_agents
            .as_ref()
            .and_then(|agents| agents.iter().find(|agent| agent.id == id))
    });
    let is_api_runtime = custom.is_some_and(|agent| agent.runtime == "api");
    let engine = custom
        .and_then(|agent| agent.engine.as_deref())
        .unwrap_or(resolved_engine);
    let backend =
        BackendKind::try_from(settings.agent_runtime_backend.as_str()).unwrap_or(BackendKind::Pty);
    let availability = SystemProviderAvailability::new(settings.claude_command);
    select_provider(engine, is_api_runtime, backend, &availability)
}
